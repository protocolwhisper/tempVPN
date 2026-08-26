## ADDED Requirements

### Requirement: Production payment challenges use explicit deployment identity
Every production node SHALL issue Tempo challenges using the configured production chain, currency, recipient, and realm. A production node MUST fail startup instead of silently using development payment defaults when any required production payment setting is absent.

#### Scenario: Production node issues a challenge
- **WHEN** an unpaid client requests a fixed or streaming session from a production node
- **THEN** the challenge identifies Tempo mainnet chain `4217`
- **AND** it identifies the configured production currency and recipient
- **AND** its realm equals the operator-configured `tempvpn.xyz` protection space

#### Scenario: Production payment configuration is incomplete
- **WHEN** a node starts in production streaming mode without its durable store, close signer, chain, currency, recipient, or realm
- **THEN** startup fails before the node accepts paid traffic

### Requirement: Standard wallet Session v2 vouchers remain supported
The streaming endpoint SHALL accept well-formed secp256k1, P256, and WebAuthn primitive voucher signatures that resolve to the channel descriptor's effective signer, while rejecting malformed, unsupported, or keychain-wrapped signatures.

#### Scenario: Standard Tempo wallet pays for streaming access
- **WHEN** a client submits a valid P256 primitive voucher produced by a standard Tempo wallet
- **THEN** the node verifies it under the same funding and replay rules as a valid secp256k1 voucher

### Requirement: Streaming discovery and runtime use POST-only creation
The directory and runtime SHALL advertise and accept `POST /sessions/stream` as the only streaming-session creation method. `GET` MUST return a non-payable method error, and `HEAD` SHALL remain limited to authenticated Session v2 management.

#### Scenario: Directory client discovers streaming access
- **WHEN** a client reads the TempVPN directory entry or OpenAPI document
- **THEN** it is directed to `POST /sessions/stream`

#### Scenario: Scanner sends GET
- **WHEN** an unauthenticated scanner requests `GET /sessions/stream`
- **THEN** the node returns `405 Method Not Allowed`
- **AND** it does not issue an MPP challenge
