# Persistent Session Coordination Specification

## Purpose

Define the durable coordinator contracts that preserve fixed paid entitlements,
prevent duplicate payment redemption, and authorize node generations without exposing client secrets.

## Requirements

### Requirement: Durable fixed-session authority

The coordinator SHALL be the durable authority for fixed paid-session state, remaining balance, grace deadline, tunnel-address reservation, client public key, logical node, and optional active generation.

#### Scenario: Coordinator restarts
- **GIVEN** fixed sessions and generation records have been committed
- **WHEN** the coordinator process or VM restarts using the same persistent data disk
- **THEN** every committed entitlement, balance, address reservation, generation owner, and reconciliation revision remains available

#### Scenario: Node daemon restarts
- **WHEN** a node generation restarts
- **THEN** it reconstructs its managed peer set from coordinator state
- **AND** does not erase paused entitlements or address reservations

### Requirement: Atomic connected-time accounting

Every state-changing coordinator operation SHALL account elapsed active usage in the same durable transaction and SHALL never reduce remaining time below zero.

#### Scenario: Concurrent lifecycle mutations
- **WHEN** heartbeat, pause, status refresh, or expiry operations race for one active session
- **THEN** committed usage is monotonic
- **AND** the same elapsed interval is not charged more than once

### Requirement: Replay-safe paid entitlement creation

The coordinator SHALL persist a payment intent before issuing a fixed-purchase challenge and SHALL permit one successful payment transaction reference to create exactly one entitlement.

#### Scenario: Paid response is lost
- **GIVEN** payment redemption committed but the client did not receive the success response
- **WHEN** the same payment intent and transaction reference are retried
- **THEN** the coordinator returns the same session identifier and entitlement
- **AND** does not create or charge a second entitlement

#### Scenario: Transaction is reused for another request
- **WHEN** a redeemed transaction reference is submitted with a different intent, logical node, or duration fingerprint
- **THEN** the coordinator rejects the request without changing either entitlement

### Requirement: Session-token confidentiality

The coordinator SHALL index sessions only by a one-way lookup hash and SHALL keep any recoverable session token encrypted with a versioned key stored outside the database.

#### Scenario: Database contents are inspected
- **WHEN** an unauthorized party obtains only the SQLite files
- **THEN** no plaintext session token, client private key, mTLS private key, or WireGuard private key is present

#### Scenario: Client activates a session
- **WHEN** the client submits its session identifier and WireGuard public key through the selected node API
- **THEN** the system performs lookup without requesting or transmitting the client's WireGuard private key

### Requirement: Authenticated generation coordination

Private coordinator operations SHALL authenticate node generations with short-lived certificates scoped to one logical node and generation. Initial enrollment SHALL require a time-limited, single-use token.

#### Scenario: Generation enrolls successfully
- **GIVEN** a valid unused enrollment token scoped to the generation
- **WHEN** the generation presents its locally generated certificate public key
- **THEN** the coordinator consumes the token and issues a generation-scoped certificate

#### Scenario: Generation crosses its authorization scope
- **WHEN** a generation attempts to acknowledge peers, renew leases, or mutate sessions owned by another logical node or generation
- **THEN** the coordinator rejects the operation without changing durable state

### Requirement: Fail-closed coordinator mutations

Nodes SHALL reject new purchases, activations, pauses, and other durable mutations when the coordinator cannot confirm them.

#### Scenario: Short coordinator outage
- **WHEN** the coordinator is temporarily unavailable
- **THEN** existing peers remain configured only through their last confirmed lease
- **AND** new durable mutations return a retryable unavailable response

#### Scenario: Outage exceeds the active lease
- **WHEN** a node cannot renew an active peer lease before its deadline
- **THEN** the node removes the peer locally
- **AND** the coordinator accounts and pauses or expires the session when authority is restored
