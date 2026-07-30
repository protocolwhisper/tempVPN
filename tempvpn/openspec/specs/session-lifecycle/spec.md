# Session Lifecycle Specification

## Purpose

Define the node-bound, paid usage-balance lifecycle from session creation through
activation, pause, status reporting, expiration, and administrative removal.

## Requirements

### Requirement: Paid session creation

The node SHALL create a session only after the `POST /sessions` request satisfies
the node's Tempo MPP charge. The requested duration SHALL be at least one second
and SHALL NOT exceed the node's configured maximum duration.

#### Scenario: Valid paid request creates a paused balance

- **GIVEN** a paid request with a duration within the configured bounds
- **WHEN** the node creates the session
- **THEN** it returns a unique `sess_`-prefixed session identifier
- **AND** it records the selected node URL, total seconds, remaining seconds, and grace deadline
- **AND** the initial state is `paused`
- **AND** the remaining seconds equal the purchased duration
- **AND** no client public key or tunnel address is assigned

#### Scenario: Invalid duration is rejected

- **WHEN** a client requests zero seconds or more than the configured maximum
- **THEN** the node rejects the request without creating a session

### Requirement: Node-bound activation

A paid session SHALL be activated only on the node that created it. Activation
SHALL require a non-empty client public key and SHALL NOT require or accept the
client private key.

#### Scenario: Connect a valid paused session

- **GIVEN** a paused session whose grace deadline has not passed
- **AND** the session has remaining connected time
- **WHEN** the client submits its public key to that session's connect endpoint
- **THEN** the node assigns or reuses the session's tunnel address
- **AND** it authorizes the public key as a WireGuard peer
- **AND** it sets the state to `active`
- **AND** it records the connection and heartbeat timestamps
- **AND** it returns the node URL, assigned address, server public key, endpoint, expected exit IP, remaining seconds, and grace deadline

#### Scenario: Activation cannot configure the peer

- **GIVEN** a connectable paid session
- **WHEN** the node cannot add its WireGuard peer
- **THEN** activation fails
- **AND** the session is not left active
- **AND** any newly reserved tunnel address is released
- **AND** the failed client public key and address are not returned as an active assignment

#### Scenario: Expired balance cannot reconnect

- **GIVEN** a session with no remaining time or a passed grace deadline
- **WHEN** a client attempts to connect it
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
deadline passes. An expired session SHALL NOT authorize VPN traffic.

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

#### Scenario: Cleanup removes expired resources

- **GIVEN** an expired session
- **WHEN** periodic cleanup processes it
- **THEN** the node removes its WireGuard peer
- **AND** releases its tunnel-address reservation
- **AND** removes the session from the in-memory store

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

### Requirement: In-memory session ownership

The current node daemon SHALL treat its in-memory store as the authority for
session state and SHALL clean up managed WireGuard peers during graceful
shutdown.

#### Scenario: Daemon shuts down gracefully

- **GIVEN** sessions with configured WireGuard peers
- **WHEN** the daemon performs shutdown cleanup
- **THEN** it attempts to remove every managed peer
- **AND** clears its in-memory sessions and address reservations

#### Scenario: Daemon restarts

- **WHEN** the daemon process restarts
- **THEN** sessions from the prior process are no longer available

