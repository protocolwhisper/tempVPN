## MODIFIED Requirements

### Requirement: Activate access only after verified funding
The node SHALL create or activate a WireGuard peer only after it has verified a Session v2 channel operation that funds at least one billing unit for the requested client key. Voucher verification SHALL accept the primitive secp256k1, P256, and WebAuthn signature encodings permitted by TIP-1034 and TIP-1020 only when the signature is well formed, cryptographically valid, and resolves to the channel descriptor's effective signer. The node MUST reject stateful keychain wrapper signatures and unsupported or malformed signature encodings.

#### Scenario: Valid funded channel
- **WHEN** the node verifies a current Session v2 credential with sufficient available value
- **THEN** it creates the VPN session and emits its connection details on the authenticated stream

#### Scenario: Standard-wallet primitive voucher
- **WHEN** a standard wallet submits a valid P256 or WebAuthn primitive voucher bound to the descriptor's effective signer
- **THEN** the node accepts the voucher under the same funding and replay rules as a valid secp256k1 voucher

#### Scenario: Invalid session credential
- **WHEN** channel identity, signature, chain, currency, recipient, or available value validation fails
- **THEN** the node rejects the request and does not create or retain a WireGuard peer

#### Scenario: Unsupported or wrapped signature
- **WHEN** a voucher contains a malformed encoding, unknown signature type, signer mismatch, high-`s` signature, or stateful keychain wrapper
- **THEN** the node rejects the credential without advancing channel state or creating a WireGuard peer
