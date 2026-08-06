use std::{future::Future, pin::Pin, str::FromStr, sync::Arc};

use alloy::{
    consensus::transaction::SignerRecoverable,
    network::ReceiptResponse,
    primitives::{Address, Bytes, TxKind, Uint, B256},
    providers::Provider,
    sol_types::SolCall,
};
use mpp::protocol::{methods::tempo::session::ChannelDescriptor, traits::VerificationError};
use tempo_alloy::{contracts::precompiles::ITIP20ChannelReserve, TempoNetwork};
use tempo_primitives::{transaction::TEMPO_TX_TYPE_ID, AASigned};

use super::protocol::{decode_signature, ParsedDescriptor};

type U96 = Uint<96, 2>;
pub type ChainFuture<T> = Pin<Box<dyn Future<Output = Result<T, VerificationError>> + Send>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReserveState {
    pub deposit: u128,
    pub settled: u128,
    pub close_requested_at: u64,
    pub finalized: bool,
}

#[derive(Debug, Clone)]
pub struct OpenOperation {
    pub transaction: String,
    pub descriptor: ChannelDescriptor,
    pub claimed_channel_id: B256,
    pub required_deposit: u128,
}

#[derive(Debug, Clone)]
pub struct TopUpOperation {
    pub transaction: String,
    pub descriptor: ChannelDescriptor,
    pub claimed_channel_id: B256,
    pub additional_deposit: u128,
}

#[derive(Debug, Clone)]
pub struct CloseOperation {
    pub descriptor: ChannelDescriptor,
    pub claimed_channel_id: B256,
    pub cumulative_amount: u128,
    pub capture_amount: u128,
    pub signature: String,
}

#[derive(Debug, Clone)]
pub struct ChainResult {
    pub state: ReserveState,
    pub tx_hash: String,
}

/// Boundary around Tempo RPC behavior so protocol handling can be tested without a node.
pub trait ReserveChain: Send + Sync {
    fn open(&self, operation: OpenOperation) -> ChainFuture<ChainResult>;
    fn top_up(&self, operation: TopUpOperation) -> ChainFuture<ChainResult>;
    fn read(&self, channel_id: B256, descriptor: ChannelDescriptor) -> ChainFuture<ReserveState>;
    fn close(&self, operation: CloseOperation) -> ChainFuture<ChainResult>;
}

#[derive(Clone)]
pub struct TempoReserveChain<P> {
    provider: Arc<P>,
    reserve: Address,
    chain_id: u64,
    close_signer: Address,
}

impl<P> TempoReserveChain<P> {
    pub fn new(provider: P, reserve: Address, chain_id: u64, close_signer: Address) -> Self {
        Self {
            provider: Arc::new(provider),
            reserve,
            chain_id,
            close_signer,
        }
    }
}

impl<P> ReserveChain for TempoReserveChain<P>
where
    P: Provider<TempoNetwork> + Clone + Send + Sync + 'static,
{
    fn open(&self, operation: OpenOperation) -> ChainFuture<ChainResult> {
        let provider = self.provider.clone();
        let reserve = self.reserve;
        let chain_id = self.chain_id;
        Box::pin(async move {
            let raw = validate_open_transaction(&operation, reserve, chain_id)?;
            let tx_hash = broadcast(&*provider, &raw, "open").await?;
            let state = read_reserve(
                &*provider,
                reserve,
                operation.claimed_channel_id,
                &operation.descriptor,
            )
            .await?;
            if state.deposit == 0 || state.finalized || state.close_requested_at != 0 {
                return Err(VerificationError::channel_closed(
                    "opened channel is not active and funded on-chain",
                ));
            }
            Ok(ChainResult { state, tx_hash })
        })
    }

    fn top_up(&self, operation: TopUpOperation) -> ChainFuture<ChainResult> {
        let provider = self.provider.clone();
        let reserve = self.reserve;
        let chain_id = self.chain_id;
        Box::pin(async move {
            let before = read_reserve(
                &*provider,
                reserve,
                operation.claimed_channel_id,
                &operation.descriptor,
            )
            .await?;
            let raw = validate_top_up_transaction(&operation, reserve, chain_id)?;
            let tx_hash = broadcast(&*provider, &raw, "top-up").await?;
            let state = read_reserve(
                &*provider,
                reserve,
                operation.claimed_channel_id,
                &operation.descriptor,
            )
            .await?;
            let expected = before
                .deposit
                .checked_add(operation.additional_deposit)
                .ok_or_else(|| {
                    VerificationError::invalid_payload("top-up deposit overflows uint96")
                })?;
            if state.deposit < expected {
                return Err(VerificationError::transaction_failed(
                    "top-up receipt succeeded but reserve deposit did not increase as declared",
                ));
            }
            Ok(ChainResult { state, tx_hash })
        })
    }

    fn read(&self, _channel_id: B256, descriptor: ChannelDescriptor) -> ChainFuture<ReserveState> {
        let provider = self.provider.clone();
        let reserve = self.reserve;
        let chain_id = self.chain_id;
        Box::pin(async move {
            let claimed = ParsedDescriptor::try_from(&descriptor)?.channel_id(reserve, chain_id);
            read_reserve(&*provider, reserve, claimed, &descriptor).await
        })
    }

    fn close(&self, operation: CloseOperation) -> ChainFuture<ChainResult> {
        let provider = self.provider.clone();
        let reserve = self.reserve;
        let close_signer = self.close_signer;
        let chain_id = self.chain_id;
        Box::pin(async move {
            let parsed = ParsedDescriptor::try_from(&operation.descriptor)?;
            if close_signer != parsed.payee
                && (parsed.operator == Address::ZERO || close_signer != parsed.operator)
            {
                return Err(VerificationError::credential_mismatch(
                    "configured close signer is neither channel payee nor operator",
                ));
            }
            let derived = parsed.channel_id(reserve, chain_id);
            if derived != operation.claimed_channel_id {
                return Err(VerificationError::credential_mismatch(
                    "close descriptor does not match channelId",
                ));
            }
            let descriptor = abi_descriptor(&operation.descriptor)?;
            let signature = Bytes::from(decode_signature(&operation.signature)?);
            let contract = ITIP20ChannelReserve::new(reserve, &*provider);
            let pending = contract
                .close(
                    descriptor,
                    U96::from(operation.cumulative_amount),
                    U96::from(operation.capture_amount),
                    signature,
                )
                .send()
                .await
                .map_err(|error| {
                    VerificationError::network_error(format!(
                        "failed to submit reserve close transaction: {error}"
                    ))
                })?;
            let receipt = pending.get_receipt().await.map_err(|error| {
                VerificationError::network_error(format!(
                    "failed waiting for reserve close receipt: {error}"
                ))
            })?;
            if !receipt.status() {
                return Err(VerificationError::transaction_failed(
                    "reserve close transaction reverted",
                ));
            }
            let tx_hash = receipt.transaction_hash().to_string();
            Ok(ChainResult {
                state: ReserveState {
                    deposit: 0,
                    settled: operation.capture_amount,
                    close_requested_at: 0,
                    finalized: true,
                },
                tx_hash,
            })
        })
    }
}

pub fn validate_open_transaction(
    operation: &OpenOperation,
    reserve: Address,
    chain_id: u64,
) -> Result<Bytes, VerificationError> {
    let (raw, signed, sender) = decode_tempo_transaction(&operation.transaction, chain_id)?;
    let descriptor = ParsedDescriptor::try_from(&operation.descriptor)?;
    if sender != descriptor.payer {
        return Err(VerificationError::credential_mismatch(
            "open transaction signer does not match descriptor payer",
        ));
    }
    if signed.expiring_nonce_hash(sender) != descriptor.expiring_nonce_hash {
        return Err(VerificationError::credential_mismatch(
            "open transaction expiring nonce hash does not match descriptor",
        ));
    }
    let call = single_reserve_call(&signed, reserve, "open")?;
    let decoded = ITIP20ChannelReserve::openCall::abi_decode(&call.input).map_err(|error| {
        VerificationError::invalid_payload(format!("invalid reserve open calldata: {error}"))
    })?;
    if decoded.payee != descriptor.payee
        || decoded.operator != descriptor.operator
        || decoded.token != descriptor.token
        || decoded.salt != descriptor.salt
        || decoded.authorizedSigner != descriptor.authorized_signer
    {
        return Err(VerificationError::credential_mismatch(
            "open transaction arguments do not match descriptor",
        ));
    }
    if decoded.deposit.to::<u128>() < operation.required_deposit {
        return Err(VerificationError::insufficient_balance(
            "open transaction deposit is below the signed initial voucher",
        ));
    }
    if descriptor.channel_id(reserve, chain_id) != operation.claimed_channel_id {
        return Err(VerificationError::credential_mismatch(
            "open descriptor does not derive the claimed channelId",
        ));
    }
    Ok(raw)
}

pub fn validate_top_up_transaction(
    operation: &TopUpOperation,
    reserve: Address,
    chain_id: u64,
) -> Result<Bytes, VerificationError> {
    let (raw, signed, sender) = decode_tempo_transaction(&operation.transaction, chain_id)?;
    let descriptor = ParsedDescriptor::try_from(&operation.descriptor)?;
    if sender != descriptor.payer {
        return Err(VerificationError::credential_mismatch(
            "top-up transaction signer does not match descriptor payer",
        ));
    }
    if descriptor.channel_id(reserve, chain_id) != operation.claimed_channel_id {
        return Err(VerificationError::credential_mismatch(
            "top-up descriptor does not derive the claimed channelId",
        ));
    }
    let call = single_reserve_call(&signed, reserve, "topUp")?;
    let decoded = ITIP20ChannelReserve::topUpCall::abi_decode(&call.input).map_err(|error| {
        VerificationError::invalid_payload(format!("invalid reserve topUp calldata: {error}"))
    })?;
    if decoded.descriptor != abi_descriptor(&operation.descriptor)?
        || decoded.additionalDeposit.to::<u128>() != operation.additional_deposit
    {
        return Err(VerificationError::credential_mismatch(
            "top-up transaction arguments do not match the credential",
        ));
    }
    Ok(raw)
}

fn decode_tempo_transaction(
    transaction: &str,
    expected_chain_id: u64,
) -> Result<(Bytes, AASigned, Address), VerificationError> {
    let raw = Bytes::from_str(transaction)
        .map_err(|_| VerificationError::invalid_payload("transaction must be 0x-prefixed hex"))?;
    if raw.first().copied() != Some(TEMPO_TX_TYPE_ID) {
        return Err(VerificationError::invalid_payload(
            "session operation must use a Tempo 0x76 transaction",
        ));
    }
    let mut payload = &raw[1..];
    let signed = AASigned::rlp_decode(&mut payload).map_err(|error| {
        VerificationError::invalid_payload(format!("failed to decode Tempo transaction: {error}"))
    })?;
    if !payload.is_empty() {
        return Err(VerificationError::invalid_payload(
            "Tempo transaction contains trailing bytes",
        ));
    }
    if signed.tx().chain_id != expected_chain_id {
        return Err(VerificationError::credential_mismatch(
            "Tempo transaction chain does not match the challenge",
        ));
    }
    let sender = signed.recover_signer().map_err(|error| {
        VerificationError::invalid_signature(format!(
            "failed to recover Tempo transaction signer: {error}"
        ))
    })?;
    Ok((raw, signed, sender))
}

fn single_reserve_call<'a>(
    signed: &'a AASigned,
    reserve: Address,
    action: &str,
) -> Result<&'a tempo_primitives::transaction::Call, VerificationError> {
    if signed.tx().calls.len() != 1 {
        return Err(VerificationError::invalid_payload(format!(
            "TIP-1034 {action} transaction must contain exactly one call"
        )));
    }
    let call = &signed.tx().calls[0];
    if call.to != TxKind::Call(reserve) {
        return Err(VerificationError::credential_mismatch(format!(
            "TIP-1034 {action} transaction does not target the configured reserve"
        )));
    }
    Ok(call)
}

async fn broadcast<P: Provider<TempoNetwork>>(
    provider: &P,
    transaction: &Bytes,
    action: &str,
) -> Result<String, VerificationError> {
    let pending = provider
        .send_raw_transaction(transaction)
        .await
        .map_err(|error| {
            VerificationError::network_error(format!(
                "failed to broadcast reserve {action} transaction: {error}"
            ))
        })?;
    let receipt = pending.get_receipt().await.map_err(|error| {
        VerificationError::network_error(format!(
            "failed waiting for reserve {action} receipt: {error}"
        ))
    })?;
    if !receipt.status() {
        return Err(VerificationError::transaction_failed(format!(
            "reserve {action} transaction reverted"
        )));
    }
    Ok(receipt.transaction_hash().to_string())
}

async fn read_reserve<P: Provider<TempoNetwork>>(
    provider: &P,
    reserve: Address,
    claimed_channel_id: B256,
    descriptor: &ChannelDescriptor,
) -> Result<ReserveState, VerificationError> {
    let parsed = ParsedDescriptor::try_from(descriptor)?;
    let contract = ITIP20ChannelReserve::new(reserve, provider);
    let channel = contract
        .getChannel(abi_descriptor(descriptor)?)
        .call()
        .await
        .map_err(|error| {
            VerificationError::network_error(format!(
                "failed to read TIP-1034 reserve channel: {error}"
            ))
        })?;
    if channel.descriptor != abi_descriptor(descriptor)? {
        return Err(VerificationError::credential_mismatch(
            "on-chain channel descriptor differs from the credential",
        ));
    }
    if parsed.channel_id(
        reserve,
        provider.get_chain_id().await.map_err(|error| {
            VerificationError::network_error(format!("failed to read provider chain id: {error}"))
        })?,
    ) != claimed_channel_id
    {
        return Err(VerificationError::credential_mismatch(
            "on-chain descriptor does not match claimed channelId",
        ));
    }
    Ok(ReserveState {
        deposit: channel.state.deposit.to::<u128>(),
        settled: channel.state.settled.to::<u128>(),
        close_requested_at: u64::from(channel.state.closeRequestedAt),
        finalized: channel.state.deposit == U96::ZERO && channel.state.settled == U96::ZERO,
    })
}

fn abi_descriptor(
    descriptor: &ChannelDescriptor,
) -> Result<ITIP20ChannelReserve::ChannelDescriptor, VerificationError> {
    let parsed = ParsedDescriptor::try_from(descriptor)?;
    Ok(ITIP20ChannelReserve::ChannelDescriptor {
        payer: parsed.payer,
        payee: parsed.payee,
        operator: parsed.operator,
        token: parsed.token,
        salt: parsed.salt,
        authorizedSigner: parsed.authorized_signer,
        expiringNonceHash: parsed.expiring_nonce_hash,
    })
}

#[cfg(test)]
mod tests {
    use alloy::{
        primitives::{Signature as AlloySignature, U256},
        signers::{local::PrivateKeySigner, SignerSync},
    };
    use tempo_primitives::transaction::{
        Call, PrimitiveSignature, TempoSignature, TempoTransaction,
    };

    use super::*;
    use mpp::protocol::methods::tempo::compute_precompile_channel_id_with_escrow;

    #[allow(clippy::too_many_arguments)]
    fn signed_open(
        signer: &PrivateKeySigner,
        reserve: Address,
        chain_id: u64,
        payee: Address,
        operator: Address,
        token: Address,
        salt: B256,
        authorized_signer: Address,
        deposit: u128,
    ) -> (String, ChannelDescriptor, B256) {
        let call = ITIP20ChannelReserve::openCall::new((
            payee,
            operator,
            token,
            U96::from(deposit),
            salt,
            authorized_signer,
        ));
        let tx = TempoTransaction {
            chain_id,
            fee_token: Some(token),
            max_priority_fee_per_gas: 1,
            max_fee_per_gas: 1,
            gas_limit: 1_000_000,
            calls: vec![Call {
                to: TxKind::Call(reserve),
                value: U256::ZERO,
                input: Bytes::from(call.abi_encode()),
            }],
            access_list: Default::default(),
            nonce_key: U256::ZERO,
            nonce: 1,
            fee_payer_signature: None,
            valid_before: None,
            valid_after: None,
            key_authorization: None,
            tempo_authorization_list: vec![],
        };
        let signature: AlloySignature = signer
            .sign_hash_sync(&tx.signature_hash())
            .expect("signature");
        let signed = AASigned::new_unhashed(
            tx,
            TempoSignature::Primitive(PrimitiveSignature::Secp256k1(signature)),
        );
        let expiring_nonce_hash = signed.expiring_nonce_hash(signer.address());
        let descriptor = ChannelDescriptor {
            payer: signer.address().to_string(),
            payee: payee.to_string(),
            operator: operator.to_string(),
            token: token.to_string(),
            salt: salt.to_string(),
            authorized_signer: authorized_signer.to_string(),
            expiring_nonce_hash: expiring_nonce_hash.to_string(),
        };
        let channel_id = compute_precompile_channel_id_with_escrow(
            signer.address(),
            payee,
            operator,
            token,
            salt,
            authorized_signer,
            expiring_nonce_hash,
            reserve,
            chain_id,
        );
        let mut bytes = Vec::new();
        signed.eip2718_encode(&mut bytes);
        (alloy::hex::encode_prefixed(bytes), descriptor, channel_id)
    }

    #[test]
    fn open_transaction_binds_sender_nonce_chain_and_call() {
        let signer = PrivateKeySigner::random();
        let reserve: Address = "0x4d50500000000000000000000000000000000000"
            .parse()
            .unwrap();
        let (transaction, descriptor, channel_id) = signed_open(
            &signer,
            reserve,
            42_431,
            Address::repeat_byte(0x22),
            Address::repeat_byte(0x33),
            Address::repeat_byte(0x44),
            B256::repeat_byte(0x55),
            signer.address(),
            5_000,
        );
        let operation = OpenOperation {
            transaction,
            descriptor,
            claimed_channel_id: channel_id,
            required_deposit: 5_000,
        };
        assert!(validate_open_transaction(&operation, reserve, 42_431).is_ok());
        assert!(validate_open_transaction(&operation, reserve, 4_217).is_err());

        let mut wrong = operation.clone();
        wrong.descriptor.operator = Address::repeat_byte(0x99).to_string();
        assert!(validate_open_transaction(&wrong, reserve, 42_431).is_err());
        let mut wrong = operation.clone();
        wrong.descriptor.payee = Address::repeat_byte(0x98).to_string();
        assert!(validate_open_transaction(&wrong, reserve, 42_431).is_err());
        let mut wrong = operation.clone();
        wrong.descriptor.token = Address::repeat_byte(0x97).to_string();
        assert!(validate_open_transaction(&wrong, reserve, 42_431).is_err());
        let mut wrong = operation.clone();
        wrong.descriptor.expiring_nonce_hash = B256::repeat_byte(0x96).to_string();
        assert!(validate_open_transaction(&wrong, reserve, 42_431).is_err());
        let mut wrong = operation.clone();
        wrong.claimed_channel_id = B256::repeat_byte(0x95);
        assert!(validate_open_transaction(&wrong, reserve, 42_431).is_err());
        assert!(validate_open_transaction(&operation, Address::repeat_byte(0x88), 42_431).is_err());
    }
}
