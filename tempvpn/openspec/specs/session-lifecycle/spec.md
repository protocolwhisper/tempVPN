# Session Lifecycle Specification

## Purpose

Define the node-bound, paid usage-balance lifecycle from session creation through
activation, pause, status reporting, expiration, and administrative removal.

## Requirements

### Requirement: Paid session creation

The selected logical node SHALL create a durable session only after
`POST /sessions` satisfies its Tempo MPP charge. The requested duration SHALL
be at least one second and SHALL NOT exceed the configured maximum duration.

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

### Requirement: Logical-node activation

A paid session SHALL belong to the logical node selected at purchase and MAY
activate on that logical node's current accepting generation. Activation SHALL
require a non-empty client public key and SHALL NOT require or accept the client
private key.

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
- **THEN** the node rejects activation

### Requirement: Connected-time accounting

The node SHALL consume purchased seconds only while a session is active. Usage
accounting SHALL be monotonic and SHALL NOT reduce the balance below zero.

#### Scenario: Active time consumes balance

- **GIVEN** an active session
- **WHEN** its status is refreshed, it sends a heartbeat, it is paused, or cleanup runs
- **THEN** elapsed active time is subtracted from its remaining seconds
- **AND** its accounting timestamp advances to the accounted instant

#### Scenario: Paused time does not consume balance

- **GIVEN** a paused session with remaining seconds
- **WHEN** time passes before its grace deadline
- **THEN** its remaining seconds do not decrease

### Requirement: Heartbeat handling

The node SHALL use heartbeats to distinguish a live active client from an
abandoned connection.

#### Scenario: Active heartbeat refreshes liveness

- **GIVEN** an active session with remaining time
- **WHEN** the node accepts a heartbeat
- **THEN** it accounts for elapsed active time
- **AND** it records the heartbeat time
- **AND** it returns the refreshed state, remaining seconds, and grace deadline

#### Scenario: Stale session is paused

- **GIVEN** an active session whose last heartbeat is older than the configured stale timeout
- **WHEN** periodic cleanup evaluates the session
- **THEN** usage is charged only through the stale-timeout cutoff
- **AND** the session becomes `paused`
- **AND** it no longer has an authorized WireGuard peer

### Requirement: Pause preserves unused balance

Pausing a session SHALL stop connected-time consumption without revoking the
unused balance.

#### Scenario: Pause an active session

- **GIVEN** an active session with remaining time
- **WHEN** the client calls the pause endpoint
- **THEN** elapsed active time is accounted
- **AND** the state becomes `paused`
- **AND** the connection and heartbeat timestamps are cleared
- **AND** the client public key is removed from WireGuard
- **AND** the unused remaining seconds are preserved

#### Scenario: Repeated pause is safe

- **GIVEN** an existing session that is already paused
- **WHEN** the client calls the pause endpoint again
- **THEN** the session remains paused with the same unused balance, subject to its grace deadline

### Requirement: Automatic expiration and cleanup

A session SHALL expire when its remaining seconds reach zero or its grace
deadline passes. An expired session SHALL NOT authorize VPN traffic, but its
terminal payment and entitlement record SHALL remain durable.

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

### Requirement: Public status and privileged administration

Paid clients SHALL be able to read their session status and perform normal
connect, heartbeat, and pause operations without a daemon-admin credential.
Administrative inspection and deletion SHALL require the configured admin
credential.

#### Scenario: Client reads session status

- **GIVEN** an existing session identifier
- **WHEN** a client requests the public status endpoint
- **THEN** the node returns its refreshed lifecycle state, remaining seconds, and grace deadline

#### Scenario: Unknown session is queried

- **WHEN** a client requests status for a session absent from the node's store
- **THEN** the node returns a not-found response

#### Scenario: Administrative endpoint lacks authorization

- **WHEN** a caller reads or deletes a session through an administrative endpoint without the configured admin credential
- **THEN** the node rejects the request as unauthorized

#### Scenario: Normal client disconnects

- **WHEN** a paid client disconnects
- **THEN** it calls the pause operation
- **AND** it does not call an administrative deletion endpoint

### Requirement: Durable logical-node ownership

The coordinator SHALL preserve paid entitlements independently of any node
daemon process and SHALL record a generation owner only while a reconciled peer
is active or being released.

#### Scenario: Active generation shuts down gracefully

- **WHEN** a generation begins graceful shutdown
- **THEN** it stops admission
- **AND** reconciles removal of every managed peer
- **AND** does not delete durable paused balances or address reservations

#### Scenario: Node process restarts

- **WHEN** a node daemon restarts
- **THEN** committed paid sessions remain queryable
- **AND** the generation reconciles its peers before reporting readiness
