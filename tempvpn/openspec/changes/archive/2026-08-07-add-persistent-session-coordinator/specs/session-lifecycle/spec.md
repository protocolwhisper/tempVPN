## MODIFIED Requirements

### Requirement: Paid session creation

The selected logical node SHALL create a durable session only after `POST /sessions` satisfies its Tempo MPP charge. The requested duration SHALL be at least one second and SHALL NOT exceed the configured maximum duration.

#### Scenario: Valid paid request creates a paused balance
- **GIVEN** a paid request with a duration within configured bounds
- **WHEN** the payment transaction is redeemed for the selected logical node
- **THEN** the system returns a unique `sess_`-prefixed session identifier
- **AND** durably records the stable logical node URL, total seconds, remaining seconds, and grace deadline
- **AND** the initial public state is `paused`
- **AND** the remaining seconds equal the purchased duration
- **AND** no active generation or WireGuard peer is assigned

#### Scenario: Invalid duration is rejected
- **WHEN** a client requests zero seconds or more than the configured maximum
- **THEN** the node rejects the request without creating or redeeming an entitlement

### Requirement: Automatic expiration and cleanup

A session SHALL expire when its remaining seconds reach zero or its grace deadline passes. An expired session SHALL NOT authorize VPN traffic, but its terminal payment and entitlement record SHALL remain durable.

#### Scenario: Connected time is exhausted
- **GIVEN** an active session
- **WHEN** accounting consumes its final remaining second
- **THEN** its state becomes `expired`
- **AND** its remaining seconds become zero

#### Scenario: Grace deadline passes with unused balance
- **GIVEN** an active or paused session whose grace deadline passes
- **WHEN** the session is refreshed or periodic cleanup runs
- **THEN** its state becomes `expired`
- **AND** its remaining seconds become zero

#### Scenario: Cleanup releases terminal resources
- **GIVEN** an expired session
- **WHEN** cleanup processes it
- **THEN** its WireGuard peer is removed
- **AND** its tunnel-address reservation is released
- **AND** its terminal record remains available for payment idempotency and administration

## ADDED Requirements

### Requirement: Logical-node activation

A paid session SHALL belong to the logical node selected at purchase and MAY activate on that logical node's current accepting generation. Activation SHALL require a non-empty client public key and SHALL NOT require or accept the client private key.

#### Scenario: Connect a valid paused session
- **GIVEN** an unowned paused session before its grace deadline with remaining time
- **WHEN** the client submits its public key through the logical node's connect endpoint
- **THEN** the accepting generation reserves or reuses the session's tunnel address
- **AND** authorizes the submitted key as a WireGuard peer
- **AND** the session becomes `active` only after peer acknowledgement
- **AND** the response returns the stable logical node URL and the owning generation's address, server key, endpoint, expected exit IP, balance, and grace deadline

#### Scenario: Activation cannot configure the peer
- **WHEN** the accepting generation cannot configure or acknowledge the peer
- **THEN** activation fails
- **AND** the session is not exposed as active
- **AND** any newly created address reservation is released

#### Scenario: Expired balance cannot reconnect
- **WHEN** a session has no remaining time or its grace deadline has passed
- **THEN** activation is rejected

### Requirement: Durable logical-node ownership

The coordinator SHALL preserve paid entitlements independently of any node daemon process and SHALL record a generation owner only while a reconciled peer is active or being released.

#### Scenario: Active generation shuts down gracefully
- **WHEN** a generation begins graceful shutdown
- **THEN** it stops admission
- **AND** reconciles removal of every managed peer
- **AND** does not delete durable paused balances or address reservations

#### Scenario: Node process restarts
- **WHEN** a node daemon restarts
- **THEN** committed paid sessions remain queryable
- **AND** the generation reconciles its peers before reporting readiness

## REMOVED Requirements

### Requirement: Node-bound activation

**Reason**: A fixed entitlement now belongs to a logical node and paused sessions may activate on a newer accepting generation.

**Migration**: Clients continue using the stable logical node API URL and consume the generation-specific WireGuard metadata returned by connect.

### Requirement: In-memory session ownership

**Reason**: Process-local ownership loses paid balances during restart and cannot support safe blue/green draining.

**Migration**: Cut over only after the development fleet stops purchases and contains no existing active or paused fixed sessions.
