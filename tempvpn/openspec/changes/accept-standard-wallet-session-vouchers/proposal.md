## Why

The node daemon currently rejects valid Tempo Session vouchers produced by the standard wallet because it assumes every TIP-1034 voucher is a 65-byte secp256k1 signature. TIP-1034 delegates voucher verification to TIP-1020, which also permits primitive P256 and WebAuthn signatures, so streaming payment is incompatible with the default wallet path tracked in GitHub issue #1.

## What Changes

- Verify TIP-1034 voucher signatures using Tempo's primitive signature parser and recovery rules instead of a secp256k1-only length check.
- Accept valid secp256k1, P256, and WebAuthn voucher encodings when they recover the descriptor's effective signer.
- Continue rejecting malformed signatures, signer mismatches, high-`s` signatures, unknown signature types, and stateful keychain wrapper signatures.
- Add regression coverage for the standard-wallet P256 path and for rejection boundaries.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `vpn-streaming-payments`: Current Tempo Session credentials must accept every primitive voucher signature encoding permitted by TIP-1034/TIP-1020 while preserving signer binding and fail-closed validation.

## Impact

- **Node daemon:** `node/linux/src/session_v2/protocol.rs` and its focused tests.
- **Payment and credentials:** Broadens accepted valid voucher encodings without changing amounts, channel derivation, replay rules, settlement, or MPP challenge fields.
- **Unaffected surfaces:** Registry, Linux client, macOS client, agent skill, configuration, infrastructure, session expiry, and network routing.
- **Compatibility:** Existing 65-byte secp256k1 vouchers remain valid; standard-wallet P256 and WebAuthn primitive vouchers become valid. Keychain wrappers remain invalid for direct voucher verification as required by TIP-1034.
- **Rollback:** Reverting the verifier change restores secp256k1-only behavior but reintroduces issue #1; no state or configuration migration is required.
