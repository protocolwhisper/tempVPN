use std::{future::Future, sync::Arc};

use alloy::primitives::{Address, B256};
use chrono::Utc;
use mpp::protocol::{
    core::{PaymentCredential, Receipt},
    intents::SessionRequest,
    methods::tempo::{
        session::{
            ChannelDescriptor, SessionCredentialPayload, TempoSessionMethodDetails,
            SESSION_PROTOCOL_TIP1034,
        },
        session_method::{ChannelState, ChannelStore},
        INTENT_SESSION, METHOD_NAME,
    },
    traits::{SessionMethod, VerificationError},
};

use super::{
    chain::{CloseOperation, OpenOperation, ReserveChain, TopUpOperation},
    protocol::{decode_signature, parse_amount, verify_voucher_signature, ParsedDescriptor},
    store::SessionStore,
};

#[derive(Debug, Clone)]
pub struct SessionV2Config {
    pub reserve: Address,
    pub chain_id: u64,
    pub operator: Address,
    pub payee: Address,
    pub token: Address,
    pub unit_amount: u128,
    pub min_voucher_delta: u128,
}

#[derive(Clone)]
pub struct TempoSessionV2Method {
    chain: Arc<dyn ReserveChain>,
    store: Arc<SessionStore>,
    config: SessionV2Config,
}

impl TempoSessionV2Method {
    pub fn new(
        chain: Arc<dyn ReserveChain>,
        store: Arc<SessionStore>,
        config: SessionV2Config,
    ) -> Self {
        Self {
            chain,
            store,
            config,
        }
    }

    pub fn store(&self) -> Arc<SessionStore> {
        self.store.clone()
    }

    async fn verify(
        &self,
        credential: PaymentCredential,
        request: SessionRequest,
    ) -> Result<Receipt, VerificationError> {
        self.validate_challenge(&credential, &request)?;
        let payload: SessionCredentialPayload = credential.payload_as().map_err(|error| {
            VerificationError::invalid_payload(format!(
                "expected a Tempo session credential payload: {error}"
            ))
        })?;

        match payload {
            SessionCredentialPayload::Open {
                channel_id,
                transaction,
                descriptor,
                cumulative_amount,
                signature,
                ..
            } => {
                let descriptor = descriptor.ok_or_else(|| {
                    VerificationError::invalid_payload(
                        "TIP-1034 open credential requires a channel descriptor",
                    )
                })?;
                let (parsed, channel_id) = self.validate_descriptor(&descriptor, &channel_id)?;
                self.validate_source(&credential, parsed.payer)?;
                let cumulative = parse_amount("cumulativeAmount", &cumulative_amount)?;
                if cumulative < self.config.unit_amount {
                    return Err(VerificationError::insufficient_balance(
                        "initial voucher does not fund one billing unit",
                    ));
                }
                let signature_bytes = decode_signature(&signature)?;
                verify_voucher_signature(
                    self.config.reserve,
                    self.config.chain_id,
                    channel_id,
                    cumulative,
                    &signature_bytes,
                    parsed.effective_signer(),
                )?;

                let result = self
                    .chain
                    .open(OpenOperation {
                        transaction,
                        descriptor: descriptor.clone(),
                        claimed_channel_id: channel_id,
                        required_deposit: cumulative,
                    })
                    .await?;
                if cumulative > result.state.deposit || cumulative < result.state.settled {
                    return Err(VerificationError::amount_exceeds_deposit(
                        "initial voucher is outside the funded on-chain range",
                    ));
                }
                let existing = self.store.get_stored(&channel_id.to_string()).await?;
                let spent = existing
                    .as_ref()
                    .map(|row| row.accounting.spent)
                    .unwrap_or(result.state.settled)
                    .max(result.state.settled);
                let units = existing
                    .as_ref()
                    .map(|row| row.accounting.units)
                    .unwrap_or(0);
                if cumulative < spent {
                    return Err(VerificationError::insufficient_balance(
                        "initial voucher is below durable channel spend",
                    ));
                }
                let state = ChannelState {
                    channel_id: channel_id.to_string(),
                    chain_id: self.config.chain_id,
                    escrow_contract: self.config.reserve,
                    payer: parsed.payer,
                    payee: parsed.payee,
                    token: parsed.token,
                    authorized_signer: parsed.effective_signer(),
                    deposit: result.state.deposit,
                    settled_on_chain: result.state.settled,
                    highest_voucher_amount: cumulative,
                    highest_voucher_signature: Some(signature_bytes),
                    spent,
                    units,
                    finalized: result.state.finalized,
                    closing: false,
                    close_requested_at: result.state.close_requested_at,
                    created_at: existing
                        .map(|row| row.accounting.created_at)
                        .unwrap_or_else(|| Utc::now().to_rfc3339()),
                };
                self.store.upsert_verified(state, descriptor).await?;
                Ok(Receipt::success(METHOD_NAME, result.tx_hash))
            }
            SessionCredentialPayload::TopUp {
                channel_id,
                transaction,
                descriptor,
                additional_deposit,
                ..
            } => {
                let descriptor = self.resolve_descriptor(&channel_id, descriptor).await?;
                let (parsed, channel_id_b256) =
                    self.validate_descriptor(&descriptor, &channel_id)?;
                self.validate_source(&credential, parsed.payer)?;
                let additional_deposit = parse_amount("additionalDeposit", &additional_deposit)?;
                if additional_deposit == 0 {
                    return Err(VerificationError::invalid_payload(
                        "additionalDeposit must be greater than zero",
                    ));
                }
                let result = self
                    .chain
                    .top_up(TopUpOperation {
                        transaction,
                        descriptor,
                        claimed_channel_id: channel_id_b256,
                        additional_deposit,
                    })
                    .await?;
                let deposit = result.state.deposit;
                let settled = result.state.settled;
                let close_requested_at = result.state.close_requested_at;
                self.store
                    .update_channel(
                        &channel_id,
                        Box::new(move |current| {
                            let current = current.ok_or_else(|| {
                                VerificationError::channel_not_found(
                                    "top-up channel is missing from durable state",
                                )
                            })?;
                            if deposit <= current.deposit {
                                return Err(VerificationError::new(
                                    "top-up did not increase durable deposit",
                                ));
                            }
                            Ok(Some(ChannelState {
                                deposit,
                                settled_on_chain: current.settled_on_chain.max(settled),
                                spent: current.spent.max(settled),
                                close_requested_at,
                                ..current
                            }))
                        }),
                    )
                    .await?;
                Ok(Receipt::success(METHOD_NAME, result.tx_hash))
            }
            SessionCredentialPayload::Voucher {
                channel_id,
                descriptor,
                cumulative_amount,
                signature,
            } => {
                let descriptor = self.resolve_descriptor(&channel_id, descriptor).await?;
                let (parsed, channel_id_b256) =
                    self.validate_descriptor(&descriptor, &channel_id)?;
                self.validate_source(&credential, parsed.effective_signer())?;
                let cumulative = parse_amount("cumulativeAmount", &cumulative_amount)?;
                let signature_bytes = decode_signature(&signature)?;
                verify_voucher_signature(
                    self.config.reserve,
                    self.config.chain_id,
                    channel_id_b256,
                    cumulative,
                    &signature_bytes,
                    parsed.effective_signer(),
                )?;
                let on_chain = self.chain.read(channel_id_b256, descriptor).await?;
                let min_delta = self.config.min_voucher_delta;
                self.store
                    .update_channel(
                        &channel_id,
                        Box::new(move |current| {
                            let current = current.ok_or_else(|| {
                                VerificationError::channel_not_found("voucher channel not found")
                            })?;
                            if current.finalized || current.closing || on_chain.finalized {
                                return Err(VerificationError::channel_closed(
                                    "voucher channel is closing or finalized",
                                ));
                            }
                            if on_chain.close_requested_at != 0 {
                                return Err(VerificationError::channel_closed(
                                    "voucher channel has an on-chain close request",
                                ));
                            }
                            if cumulative == current.highest_voucher_amount {
                                if current.highest_voucher_signature.as_deref()
                                    != Some(signature_bytes.as_slice())
                                {
                                    return Err(VerificationError::invalid_signature(
                                        "replayed cumulative amount has a different signature",
                                    ));
                                }
                                return Ok(Some(ChannelState {
                                    deposit: on_chain.deposit,
                                    settled_on_chain: current
                                        .settled_on_chain
                                        .max(on_chain.settled),
                                    spent: current.spent.max(on_chain.settled),
                                    close_requested_at: on_chain.close_requested_at,
                                    ..current
                                }));
                            }
                            if cumulative < current.highest_voucher_amount {
                                return Err(VerificationError::new(
                                    "voucher cumulativeAmount must increase monotonically",
                                ));
                            }
                            if cumulative.saturating_sub(current.highest_voucher_amount) < min_delta
                            {
                                return Err(VerificationError::new(
                                    "voucher increase is below the configured minimum delta",
                                ));
                            }
                            if cumulative > on_chain.deposit || cumulative < on_chain.settled {
                                return Err(VerificationError::amount_exceeds_deposit(
                                    "voucher is outside the on-chain funded range",
                                ));
                            }
                            Ok(Some(ChannelState {
                                deposit: on_chain.deposit,
                                settled_on_chain: current.settled_on_chain.max(on_chain.settled),
                                spent: current.spent.max(on_chain.settled),
                                highest_voucher_amount: cumulative,
                                highest_voucher_signature: Some(signature_bytes),
                                close_requested_at: on_chain.close_requested_at,
                                ..current
                            }))
                        }),
                    )
                    .await?;
                Ok(Receipt::success(METHOD_NAME, channel_id))
            }
            SessionCredentialPayload::Close {
                channel_id,
                descriptor,
                cumulative_amount,
                signature,
            } => {
                let descriptor = self.resolve_descriptor(&channel_id, descriptor).await?;
                let (parsed, channel_id_b256) =
                    self.validate_descriptor(&descriptor, &channel_id)?;
                self.validate_source(&credential, parsed.effective_signer())?;
                let cumulative = parse_amount("cumulativeAmount", &cumulative_amount)?;
                let signature_bytes = decode_signature(&signature)?;
                verify_voucher_signature(
                    self.config.reserve,
                    self.config.chain_id,
                    channel_id_b256,
                    cumulative,
                    &signature_bytes,
                    parsed.effective_signer(),
                )?;
                let before = self.store.get_channel(&channel_id).await?.ok_or_else(|| {
                    VerificationError::channel_not_found("close channel not found")
                })?;
                if cumulative < before.spent.max(before.settled_on_chain)
                    || cumulative > before.deposit
                {
                    return Err(VerificationError::amount_exceeds_deposit(
                        "close voucher cannot cover durable spend or exceeds deposit",
                    ));
                }
                let capture = before.spent.max(before.settled_on_chain);
                self.set_closing(&channel_id, true).await?;
                let result = self
                    .chain
                    .close(CloseOperation {
                        descriptor,
                        claimed_channel_id: channel_id_b256,
                        cumulative_amount: cumulative,
                        capture_amount: capture,
                        signature,
                    })
                    .await;
                let result = match result {
                    Ok(result) => result,
                    Err(error) => {
                        let _ = self.set_closing(&channel_id, false).await;
                        return Err(error);
                    }
                };
                self.store
                    .update_channel(
                        &channel_id,
                        Box::new(move |current| {
                            let current = current.ok_or_else(|| {
                                VerificationError::channel_not_found("close channel not found")
                            })?;
                            Ok(Some(ChannelState {
                                finalized: true,
                                closing: false,
                                settled_on_chain: capture,
                                highest_voucher_amount: current
                                    .highest_voucher_amount
                                    .max(cumulative),
                                highest_voucher_signature: Some(signature_bytes),
                                ..current
                            }))
                        }),
                    )
                    .await?;
                Ok(Receipt::success(METHOD_NAME, result.tx_hash))
            }
        }
    }

    fn validate_challenge(
        &self,
        credential: &PaymentCredential,
        request: &SessionRequest,
    ) -> Result<(), VerificationError> {
        if credential.challenge.method.as_str() != METHOD_NAME
            || credential.challenge.intent.as_str() != INTENT_SESSION
        {
            return Err(VerificationError::credential_mismatch(
                "credential must use method tempo and intent session",
            ));
        }
        if request.amount.parse::<u128>().ok() != Some(self.config.unit_amount) {
            return Err(VerificationError::credential_mismatch(
                "session unit price differs from server configuration",
            ));
        }
        let recipient = request
            .recipient
            .as_deref()
            .ok_or_else(|| VerificationError::invalid_payload("session recipient is required"))?
            .parse::<Address>()
            .map_err(|_| VerificationError::invalid_payload("session recipient is invalid"))?;
        let token = request.currency.parse::<Address>().map_err(|_| {
            VerificationError::invalid_payload("session currency is not an address")
        })?;
        if recipient != self.config.payee || token != self.config.token {
            return Err(VerificationError::credential_mismatch(
                "session recipient or currency differs from server configuration",
            ));
        }
        let details: TempoSessionMethodDetails = request
            .method_details
            .clone()
            .ok_or_else(|| VerificationError::invalid_payload("session methodDetails are required"))
            .and_then(|details| {
                serde_json::from_value(details).map_err(|error| {
                    VerificationError::invalid_payload(format!(
                        "invalid Tempo session methodDetails: {error}"
                    ))
                })
            })?;
        let reserve = details.escrow_contract.parse::<Address>().map_err(|_| {
            VerificationError::invalid_payload("session reserve address is invalid")
        })?;
        let operator = details
            .operator
            .as_deref()
            .ok_or_else(|| VerificationError::invalid_payload("session operator is required"))?
            .parse::<Address>()
            .map_err(|_| VerificationError::invalid_payload("session operator is invalid"))?;
        if details.session_protocol.as_deref() != Some(SESSION_PROTOCOL_TIP1034)
            || details.chain_id != Some(self.config.chain_id)
            || reserve != self.config.reserve
            || operator != self.config.operator
        {
            return Err(VerificationError::credential_mismatch(
                "credential is not bound to the configured TIP-1034 v2 session",
            ));
        }
        Ok(())
    }

    fn validate_descriptor(
        &self,
        descriptor: &ChannelDescriptor,
        channel_id: &str,
    ) -> Result<(ParsedDescriptor, B256), VerificationError> {
        let parsed = ParsedDescriptor::try_from(descriptor)?;
        let channel_id = parsed.validate_binding(
            channel_id,
            self.config.reserve,
            self.config.chain_id,
            self.config.payee,
            self.config.token,
            self.config.operator,
        )?;
        Ok((parsed, channel_id))
    }

    async fn resolve_descriptor(
        &self,
        channel_id: &str,
        supplied: Option<ChannelDescriptor>,
    ) -> Result<ChannelDescriptor, VerificationError> {
        let stored = self
            .store
            .get_stored(channel_id)
            .await?
            .ok_or_else(|| VerificationError::channel_not_found("channel not found"))?;
        let stored_descriptor = stored.descriptor.ok_or_else(|| {
            VerificationError::invalid_payload("durable channel is missing its v2 descriptor")
        })?;
        if supplied
            .as_ref()
            .is_some_and(|supplied| supplied != &stored_descriptor)
        {
            return Err(VerificationError::credential_mismatch(
                "credential descriptor differs from durable channel descriptor",
            ));
        }
        Ok(stored_descriptor)
    }

    fn validate_source(
        &self,
        credential: &PaymentCredential,
        expected: Address,
    ) -> Result<(), VerificationError> {
        let expected = PaymentCredential::evm_did(self.config.chain_id, &expected.to_string());
        let source = credential.source.as_deref().ok_or_else(|| {
            VerificationError::credential_mismatch("session credential source DID is required")
        })?;
        if !source.eq_ignore_ascii_case(&expected) {
            return Err(VerificationError::credential_mismatch(
                "session credential source DID does not match the operation signer",
            ));
        }
        Ok(())
    }

    async fn set_closing(&self, channel_id: &str, closing: bool) -> Result<(), VerificationError> {
        self.store
            .update_channel(
                channel_id,
                Box::new(move |current| {
                    let current = current
                        .ok_or_else(|| VerificationError::channel_not_found("channel not found"))?;
                    if closing && (current.closing || current.finalized) {
                        return Err(VerificationError::channel_closed(
                            "channel is already closing or finalized",
                        ));
                    }
                    Ok(Some(ChannelState { closing, ..current }))
                }),
            )
            .await?;
        Ok(())
    }
}

impl SessionMethod for TempoSessionV2Method {
    fn method(&self) -> &str {
        METHOD_NAME
    }

    fn verify_session(
        &self,
        credential: &PaymentCredential,
        request: &SessionRequest,
    ) -> impl Future<Output = Result<Receipt, VerificationError>> + Send {
        self.verify(credential.clone(), request.clone())
    }

    fn challenge_method_details(&self) -> Option<serde_json::Value> {
        serde_json::to_value(TempoSessionMethodDetails {
            escrow_contract: format!("{:#x}", self.config.reserve),
            channel_id: None,
            min_voucher_delta: Some(self.config.min_voucher_delta.to_string()),
            chain_id: Some(self.config.chain_id),
            fee_payer: Some(false),
            operator: Some(format!("{:#x}", self.config.operator)),
            session_protocol: Some(SESSION_PROTOCOL_TIP1034.into()),
            session_snapshot: None,
        })
        .ok()
    }

    fn respond(
        &self,
        credential: &PaymentCredential,
        receipt: &Receipt,
    ) -> Option<serde_json::Value> {
        let payload: SessionCredentialPayload = credential.payload_as().ok()?;
        match payload {
            SessionCredentialPayload::Voucher { .. } => None,
            _ => Some(serde_json::json!({
                "status": "ok",
                "reference": receipt.reference,
            })),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use alloy::signers::local::PrivateKeySigner;
    use mpp::protocol::{
        core::{Base64UrlJson, ChallengeEcho},
        methods::tempo::{
            compute_precompile_channel_id_with_escrow, sign_precompile_voucher_with_escrow,
        },
    };

    use crate::config::ChannelStoreConfig;

    use super::*;
    use crate::session_v2::chain::{ChainFuture, ChainResult, ReserveState};

    struct MockChain {
        states: Mutex<HashMap<B256, ReserveState>>,
    }

    impl MockChain {
        fn funded(channel_id: B256) -> Arc<Self> {
            Arc::new(Self {
                states: Mutex::new(HashMap::from([(
                    channel_id,
                    ReserveState {
                        deposit: 10_000,
                        settled: 0,
                        close_requested_at: 0,
                        finalized: false,
                    },
                )])),
            })
        }
    }

    impl ReserveChain for MockChain {
        fn open(&self, operation: OpenOperation) -> ChainFuture<ChainResult> {
            let state = self.states.lock().unwrap()[&operation.claimed_channel_id].clone();
            Box::pin(async move {
                Ok(ChainResult {
                    state,
                    tx_hash: "0xopen".into(),
                })
            })
        }

        fn top_up(&self, _operation: TopUpOperation) -> ChainFuture<ChainResult> {
            Box::pin(async {
                Err(VerificationError::transaction_failed(
                    "not used by this test",
                ))
            })
        }

        fn read(
            &self,
            channel_id: B256,
            _descriptor: ChannelDescriptor,
        ) -> ChainFuture<ReserveState> {
            let state = self.states.lock().unwrap()[&channel_id].clone();
            Box::pin(async move { Ok(state) })
        }

        fn close(&self, operation: CloseOperation) -> ChainFuture<ChainResult> {
            Box::pin(async move {
                Ok(ChainResult {
                    state: ReserveState {
                        deposit: 0,
                        settled: operation.capture_amount,
                        close_requested_at: 0,
                        finalized: true,
                    },
                    tx_hash: "0xclose".into(),
                })
            })
        }
    }

    fn request(config: &SessionV2Config, protocol: &str) -> SessionRequest {
        SessionRequest {
            amount: config.unit_amount.to_string(),
            unit_type: Some("minute".into()),
            currency: config.token.to_string(),
            recipient: Some(config.payee.to_string()),
            suggested_deposit: Some("10000".into()),
            method_details: Some(serde_json::json!({
                "escrowContract": config.reserve.to_string(),
                "chainId": config.chain_id,
                "operator": config.operator.to_string(),
                "sessionProtocol": protocol,
                "minVoucherDelta": config.min_voucher_delta.to_string(),
            })),
            ..Default::default()
        }
    }

    fn credential(
        request: &SessionRequest,
        source: Address,
        chain_id: u64,
        payload: SessionCredentialPayload,
    ) -> PaymentCredential {
        let echo = ChallengeEcho {
            id: "challenge".into(),
            realm: "vpn.test".into(),
            method: METHOD_NAME.into(),
            intent: INTENT_SESSION.into(),
            request: Base64UrlJson::from_typed(request).unwrap(),
            expires: None,
            digest: None,
            opaque: None,
        };
        PaymentCredential::with_source(
            echo,
            PaymentCredential::evm_did(chain_id, &source.to_string()),
            payload,
        )
    }

    #[tokio::test]
    async fn v2_open_and_voucher_are_verified_and_monotonic() {
        let signer = PrivateKeySigner::random();
        let config = SessionV2Config {
            reserve: "0x4d50500000000000000000000000000000000000"
                .parse()
                .unwrap(),
            chain_id: 42_431,
            operator: Address::repeat_byte(0x33),
            payee: Address::repeat_byte(0x22),
            token: Address::repeat_byte(0x44),
            unit_amount: 1_000,
            min_voucher_delta: 500,
        };
        let descriptor = ChannelDescriptor {
            payer: signer.address().to_string(),
            payee: config.payee.to_string(),
            operator: config.operator.to_string(),
            token: config.token.to_string(),
            salt: B256::repeat_byte(0x55).to_string(),
            authorized_signer: signer.address().to_string(),
            expiring_nonce_hash: B256::repeat_byte(0x77).to_string(),
        };
        let channel_id = compute_precompile_channel_id_with_escrow(
            signer.address(),
            config.payee,
            config.operator,
            config.token,
            B256::repeat_byte(0x55),
            signer.address(),
            B256::repeat_byte(0x77),
            config.reserve,
            config.chain_id,
        );
        let chain = MockChain::funded(channel_id);
        let store = SessionStore::open(&ChannelStoreConfig::Memory)
            .await
            .unwrap();
        let method = TempoSessionV2Method::new(chain, store.clone(), config.clone());
        let request = request(&config, SESSION_PROTOCOL_TIP1034);
        let open_signature = sign_precompile_voucher_with_escrow(
            &signer,
            channel_id,
            2_000,
            config.reserve,
            config.chain_id,
        )
        .await
        .unwrap();
        let open = SessionCredentialPayload::Open {
            payload_type: "transaction".into(),
            channel_id: channel_id.to_string(),
            transaction: "0x76".into(),
            descriptor: Some(descriptor.clone()),
            authorized_signer: Some(signer.address().to_string()),
            cumulative_amount: "2000".into(),
            signature: alloy::hex::encode_prefixed(open_signature),
        };
        method
            .verify_session(
                &credential(&request, signer.address(), config.chain_id, open),
                &request,
            )
            .await
            .unwrap();

        let attacker = PrivateKeySigner::random();
        let invalid_signature = sign_precompile_voucher_with_escrow(
            &attacker,
            channel_id,
            3_000,
            config.reserve,
            config.chain_id,
        )
        .await
        .unwrap();
        let invalid_voucher = SessionCredentialPayload::Voucher {
            channel_id: channel_id.to_string(),
            descriptor: None,
            cumulative_amount: "3000".into(),
            signature: alloy::hex::encode_prefixed(invalid_signature),
        };
        assert!(method
            .verify_session(
                &credential(
                    &request,
                    attacker.address(),
                    config.chain_id,
                    invalid_voucher
                ),
                &request,
            )
            .await
            .is_err());

        let top_up = SessionCredentialPayload::TopUp {
            payload_type: "transaction".into(),
            channel_id: channel_id.to_string(),
            transaction: "0x76".into(),
            descriptor: Some(descriptor.clone()),
            additional_deposit: "1000".into(),
        };
        assert!(method
            .verify_session(
                &credential(&request, signer.address(), config.chain_id, top_up),
                &request,
            )
            .await
            .is_err());

        let voucher_signature = sign_precompile_voucher_with_escrow(
            &signer,
            channel_id,
            4_000,
            config.reserve,
            config.chain_id,
        )
        .await
        .unwrap();
        let voucher = SessionCredentialPayload::Voucher {
            channel_id: channel_id.to_string(),
            descriptor: None,
            cumulative_amount: "4000".into(),
            signature: alloy::hex::encode_prefixed(voucher_signature),
        };
        let voucher_credential = credential(&request, signer.address(), config.chain_id, voucher);
        method
            .verify_session(&voucher_credential, &request)
            .await
            .unwrap();
        method
            .verify_session(&voucher_credential, &request)
            .await
            .unwrap();
        let lower_signature = sign_precompile_voucher_with_escrow(
            &signer,
            channel_id,
            3_000,
            config.reserve,
            config.chain_id,
        )
        .await
        .unwrap();
        let lower = SessionCredentialPayload::Voucher {
            channel_id: channel_id.to_string(),
            descriptor: None,
            cumulative_amount: "3000".into(),
            signature: alloy::hex::encode_prefixed(lower_signature),
        };
        assert!(method
            .verify_session(
                &credential(&request, signer.address(), config.chain_id, lower),
                &request,
            )
            .await
            .is_err());
        assert_eq!(
            store
                .get_channel(&channel_id.to_string())
                .await
                .unwrap()
                .unwrap()
                .highest_voucher_amount,
            4_000
        );
    }

    #[tokio::test]
    async fn legacy_challenge_is_rejected_before_chain_access() {
        let config = SessionV2Config {
            reserve: "0x4d50500000000000000000000000000000000000"
                .parse()
                .unwrap(),
            chain_id: 42_431,
            operator: Address::repeat_byte(0x33),
            payee: Address::repeat_byte(0x22),
            token: Address::repeat_byte(0x44),
            unit_amount: 1_000,
            min_voucher_delta: 500,
        };
        let request = request(&config, "v1");
        let store = SessionStore::open(&ChannelStoreConfig::Memory)
            .await
            .unwrap();
        let method = TempoSessionV2Method::new(
            Arc::new(MockChain {
                states: Mutex::new(HashMap::new()),
            }),
            store,
            config.clone(),
        );
        let payload = SessionCredentialPayload::Voucher {
            channel_id: B256::ZERO.to_string(),
            descriptor: None,
            cumulative_amount: "1".into(),
            signature: "0x00".into(),
        };
        assert!(method
            .verify_session(
                &credential(
                    &request,
                    Address::repeat_byte(0x11),
                    config.chain_id,
                    payload
                ),
                &request,
            )
            .await
            .is_err());
    }

    #[tokio::test]
    async fn challenge_advertises_v2_only() {
        let config = SessionV2Config {
            reserve: "0x4d50500000000000000000000000000000000000"
                .parse()
                .unwrap(),
            chain_id: 42_431,
            operator: Address::repeat_byte(0x33),
            payee: Address::repeat_byte(0x22),
            token: Address::repeat_byte(0x44),
            unit_amount: 1_000,
            min_voucher_delta: 500,
        };
        let details = TempoSessionV2Method::new(
            Arc::new(MockChain {
                states: Mutex::new(HashMap::new()),
            }),
            SessionStore::open(&ChannelStoreConfig::Memory)
                .await
                .unwrap(),
            config,
        )
        .challenge_method_details()
        .unwrap();
        assert_eq!(details["sessionProtocol"], "v2");
    }
}
