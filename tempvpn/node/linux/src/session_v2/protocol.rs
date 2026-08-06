use std::str::FromStr;

use alloy::{
    primitives::{Address, B256},
    signers::Signature,
};
use mpp::protocol::{
    methods::tempo::{
        compute_precompile_channel_id_with_escrow, precompile_voucher_signing_hash_with_escrow,
        session::ChannelDescriptor, PRECOMPILE_MAX_CUMULATIVE_AMOUNT,
    },
    traits::VerificationError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedDescriptor {
    pub payer: Address,
    pub payee: Address,
    pub operator: Address,
    pub token: Address,
    pub salt: B256,
    pub authorized_signer: Address,
    pub expiring_nonce_hash: B256,
}

impl TryFrom<&ChannelDescriptor> for ParsedDescriptor {
    type Error = VerificationError;

    fn try_from(value: &ChannelDescriptor) -> Result<Self, Self::Error> {
        Ok(Self {
            payer: parse_address("payer", &value.payer)?,
            payee: parse_address("payee", &value.payee)?,
            operator: parse_address("operator", &value.operator)?,
            token: parse_address("token", &value.token)?,
            salt: parse_b256("salt", &value.salt)?,
            authorized_signer: parse_address("authorizedSigner", &value.authorized_signer)?,
            expiring_nonce_hash: parse_b256("expiringNonceHash", &value.expiring_nonce_hash)?,
        })
    }
}

impl ParsedDescriptor {
    pub fn channel_id(self, reserve: Address, chain_id: u64) -> B256 {
        compute_precompile_channel_id_with_escrow(
            self.payer,
            self.payee,
            self.operator,
            self.token,
            self.salt,
            self.authorized_signer,
            self.expiring_nonce_hash,
            reserve,
            chain_id,
        )
    }

    pub fn validate_binding(
        self,
        claimed_channel_id: &str,
        reserve: Address,
        chain_id: u64,
        expected_payee: Address,
        expected_token: Address,
        expected_operator: Address,
    ) -> Result<B256, VerificationError> {
        if self.payee != expected_payee {
            return Err(VerificationError::credential_mismatch(
                "descriptor payee does not match the challenged recipient",
            ));
        }
        if self.token != expected_token {
            return Err(VerificationError::credential_mismatch(
                "descriptor token does not match the challenged currency",
            ));
        }
        if self.operator != expected_operator {
            return Err(VerificationError::credential_mismatch(
                "descriptor operator does not match the server operator",
            ));
        }
        if self.payer == Address::ZERO || self.expiring_nonce_hash == B256::ZERO {
            return Err(VerificationError::invalid_payload(
                "descriptor payer and expiring nonce hash must be non-zero",
            ));
        }
        let claimed = parse_b256("channelId", claimed_channel_id)?;
        let derived = self.channel_id(reserve, chain_id);
        if claimed != derived {
            return Err(VerificationError::credential_mismatch(
                "channelId does not match the TIP-1034 descriptor",
            ));
        }
        Ok(derived)
    }

    pub fn effective_signer(self) -> Address {
        if self.authorized_signer == Address::ZERO {
            self.payer
        } else {
            self.authorized_signer
        }
    }
}

pub fn parse_amount(field: &str, amount: &str) -> Result<u128, VerificationError> {
    let amount = amount.parse::<u128>().map_err(|_| {
        VerificationError::invalid_payload(format!("{field} must be a base-10 integer"))
    })?;
    if amount > PRECOMPILE_MAX_CUMULATIVE_AMOUNT {
        return Err(VerificationError::invalid_payload(format!(
            "{field} exceeds Tempo's uint96 maximum"
        )));
    }
    Ok(amount)
}

pub fn decode_signature(signature: &str) -> Result<Vec<u8>, VerificationError> {
    let signature = signature.strip_prefix("0x").unwrap_or(signature);
    alloy::hex::decode(signature)
        .map_err(|_| VerificationError::invalid_signature("voucher signature must be valid hex"))
}

pub fn verify_voucher_signature(
    reserve: Address,
    chain_id: u64,
    channel_id: B256,
    cumulative_amount: u128,
    signature_bytes: &[u8],
    expected_signer: Address,
) -> Result<(), VerificationError> {
    if signature_bytes.len() != 65 {
        return Err(VerificationError::invalid_signature(
            "TIP-1034 vouchers require a 65-byte ECDSA signature",
        ));
    }
    let hash = precompile_voucher_signing_hash_with_escrow(
        channel_id,
        cumulative_amount,
        reserve,
        chain_id,
    )
    .map_err(|error| VerificationError::invalid_payload(error.to_string()))?;
    let signature = Signature::try_from(signature_bytes).map_err(|_| {
        VerificationError::invalid_signature("voucher signature could not be decoded")
    })?;
    let recovered = signature
        .recover_address_from_prehash(&hash)
        .map_err(|_| VerificationError::invalid_signature("voucher signer recovery failed"))?;
    if recovered != expected_signer {
        return Err(VerificationError::invalid_signature(
            "voucher was not signed by the descriptor's authorized signer",
        ));
    }
    Ok(())
}

fn parse_address(field: &str, value: &str) -> Result<Address, VerificationError> {
    Address::from_str(value).map_err(|_| {
        VerificationError::invalid_payload(format!("descriptor {field} is not an address"))
    })
}

pub fn parse_b256(field: &str, value: &str) -> Result<B256, VerificationError> {
    B256::from_str(value).map_err(|_| {
        VerificationError::invalid_payload(format!("{field} is not a 32-byte hex value"))
    })
}

#[cfg(test)]
mod tests {
    use alloy::{primitives::Bytes, signers::local::PrivateKeySigner};
    use mpp::protocol::methods::tempo::sign_precompile_voucher_with_escrow;

    use super::*;

    const RESERVE: &str = "0x4d50500000000000000000000000000000000000";

    fn golden_descriptor() -> ChannelDescriptor {
        ChannelDescriptor {
            payer: "0x3d6885f89100445ca9869d1b0a49c97cfdbafeee".into(),
            payee: "0xda2390fEE8d9744b39A8A855675649e95617aCd8".into(),
            operator: format!("{:#x}", Address::ZERO),
            token: "0x20C0000000000000000000000000000000000000".into(),
            salt: "0xfb05173ba9285aef8a91f275930f68ad3565a491edb810c07baa60b643fdd378".into(),
            authorized_signer: "0xFE9d3D9cBb5f6FBe495b03f7Ec90d4Adc22126f5".into(),
            expiring_nonce_hash:
                "0x4e40183cda8c676032af4f7b038178505d877ae1c36b374239fe20ac3485c3ab".into(),
        }
    }

    #[test]
    fn descriptor_matches_mppx_golden_channel_id() {
        let parsed = ParsedDescriptor::try_from(&golden_descriptor()).expect("descriptor");
        assert_eq!(
            parsed.channel_id(RESERVE.parse().unwrap(), 42_431),
            "0xb3946b996bd166db3b61fba0f6af2918b6687bc054e2f4bae979edffc7bd0b4d"
                .parse::<B256>()
                .unwrap()
        );
    }

    #[test]
    fn descriptor_binding_rejects_a_different_operator() {
        let parsed = ParsedDescriptor::try_from(&golden_descriptor()).expect("descriptor");
        let result = parsed.validate_binding(
            "0xb3946b996bd166db3b61fba0f6af2918b6687bc054e2f4bae979edffc7bd0b4d",
            RESERVE.parse().unwrap(),
            42_431,
            parsed.payee,
            parsed.token,
            Address::repeat_byte(0x99),
        );
        assert!(result.is_err());
    }

    #[test]
    fn amount_rejects_uint96_overflow() {
        assert_eq!(
            parse_amount("amount", &PRECOMPILE_MAX_CUMULATIVE_AMOUNT.to_string()).unwrap(),
            PRECOMPILE_MAX_CUMULATIVE_AMOUNT
        );
        assert!(parse_amount(
            "amount",
            &(PRECOMPILE_MAX_CUMULATIVE_AMOUNT + 1).to_string()
        )
        .is_err());
    }

    #[tokio::test]
    async fn voucher_recovery_is_bound_to_signer_chain_and_reserve() {
        let signer = PrivateKeySigner::random();
        let channel_id = B256::repeat_byte(0xab);
        let reserve: Address = RESERVE.parse().unwrap();
        let signature: Bytes =
            sign_precompile_voucher_with_escrow(&signer, channel_id, 1_000, reserve, 42_431)
                .await
                .unwrap();

        verify_voucher_signature(
            reserve,
            42_431,
            channel_id,
            1_000,
            signature.as_ref(),
            signer.address(),
        )
        .unwrap();
        assert!(verify_voucher_signature(
            reserve,
            4_217,
            channel_id,
            1_000,
            signature.as_ref(),
            signer.address(),
        )
        .is_err());
        assert!(verify_voucher_signature(
            reserve,
            42_431,
            channel_id,
            1_000,
            signature.as_ref(),
            Address::repeat_byte(0x12),
        )
        .is_err());
    }
}
