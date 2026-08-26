## 1. Voucher Verification Tests

- [x] 1.1 Add a regression test proving a valid TIP-1020 P256 voucher from the standard-wallet signature path resolves to its authorized signer.
- [x] 1.2 Add focused rejection tests for malformed, unknown, keychain-wrapped, high-`s`, and signer-mismatched voucher signatures while preserving secp256k1 compatibility.

## 2. Node Verifier

- [x] 2.1 Replace the fixed 65-byte secp256k1 verifier with strict Tempo primitive-signature parsing and recovery against the existing TIP-1034 digest.
- [x] 2.2 Preserve original verified signature bytes for replay equality and settlement without changing channel state or WireGuard activation behavior.

## 3. Validation

- [x] 3.1 Run formatting, focused node tests, and the relevant workspace test suite.
- [x] 3.2 Run strict OpenSpec validation and review the final diff for credential leakage or unrelated changes.
