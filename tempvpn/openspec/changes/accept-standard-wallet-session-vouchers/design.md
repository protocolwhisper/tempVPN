## Context

See `proposal.md` for motivation. The custom Rust Session v2 verifier computes the current TIP-1034 voucher digest correctly, but then assumes the signature is a 65-byte recoverable secp256k1 value. TIP-1034 instead requires the primitive signature encodings and verification rules defined by TIP-1020.

The workspace already depends on `tempo-primitives`, whose `PrimitiveSignature` parser and recovery implementation enforce the protocol's encoding, signature-type, low-`s`, P256, and WebAuthn rules. Voucher validation occurs before channel state mutation and WireGuard peer creation.

## Goals / Non-Goals

**Goals:**

- Match TIP-1034/TIP-1020 primitive voucher verification semantics.
- Preserve descriptor signer binding and fail closed on unsupported encodings.
- Keep the verifier local and deterministic on the paid-request path.
- Cover the standard-wallet P256 path and rejection boundaries with focused tests.

**Non-Goals:**

- Verifying stateful keychain wrappers, which TIP-1034 explicitly rejects for direct vouchers.
- Changing channel IDs, voucher digests, payment amounts, settlement, or durable state.
- Changing client key handling, session expiry, WireGuard routing, or infrastructure.

## Decisions

### Parse vouchers as Tempo primitive signatures

Decode the hex payload, parse it with `tempo_primitives::transaction::PrimitiveSignature::from_bytes`, recover its signer from the existing TIP-1034 EIP-712 digest, and compare that address with the descriptor's effective signer.

This reuses the same primitive formats as Tempo transactions and the TIP-1020 precompile. A hand-written P256/WebAuthn verifier would duplicate security-sensitive parsing and malleability checks. Calling the on-chain precompile via RPC would add latency and availability dependence to every off-chain voucher.

### Reject keychain wrappers before state mutation

`PrimitiveSignature` accepts only secp256k1, P256, and WebAuthn. Keychain prefixes and unknown types fail parsing, matching TIP-1034's requirement that delegated voucher signing use `authorizedSigner` directly rather than a keychain wrapper.

### Preserve raw verified bytes

The channel store continues retaining the original accepted signature bytes. Replay equality therefore remains byte-exact and settlement receives the same protocol encoding that was verified.

### Keep client behavior unchanged

Linux and macOS clients do not implement this verifier and require no differences. The fix is entirely on the node's credential boundary; private keys and wallet material remain client-owned.

## Risks / Trade-offs

- **[Library behavior diverges from the deployed Tempo hardfork]** → Keep the pinned Tempo dependency and add format-specific vectors; dependency upgrades remain explicit workspace changes.
- **[Broader parsing accidentally accepts keychain wrappers]** → Use `PrimitiveSignature`, not `TempoSignature`, and assert keychain/unknown prefixes are rejected.
- **[Malformed P256 or WebAuthn input consumes excessive work]** → Rely on the protocol parser's strict length bounds before cryptographic verification.

## Migration Plan

1. Add secp256k1 compatibility, P256 acceptance, and invalid-format regression tests.
2. Replace the secp256k1-only verifier with primitive signature parsing and recovery.
3. Run node tests and strict OpenSpec validation.
4. Deploy the rebuilt daemon through the existing fleet rollout and validate a standard-wallet Session voucher before closing GitHub issue #1.

Rollback deploys the preceding daemon artifact. No database, channel, client, configuration, or infrastructure migration is required.
