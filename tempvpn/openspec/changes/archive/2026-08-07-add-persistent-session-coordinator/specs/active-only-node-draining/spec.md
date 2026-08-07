## Purpose

Define blue/green generation admission, active-session pinning, peer handoff, and the observable safety gates required before an old node generation can be removed.

## ADDED Requirements

### Requirement: Single accepting generation

Each logical node SHALL have at most one generation accepting new purchases and paused-session activations.

#### Scenario: Green is promoted
- **GIVEN** green is healthy and registered as standby
- **WHEN** an authorized operator promotes green
- **THEN** green becomes the sole accepting generation
- **AND** blue becomes draining and cannot claim new purchases or paused sessions

### Requirement: Active sessions remain generation-pinned

An active session SHALL remain owned by the generation that configured its WireGuard peer and SHALL NOT be forcibly migrated during drain.

#### Scenario: Blue session remains active during promotion
- **GIVEN** a session has an active blue peer
- **WHEN** green is promoted
- **THEN** the tunnel continues using blue's WireGuard server key and endpoint
- **AND** green does not configure a peer for that session

#### Scenario: Draining session heartbeats
- **WHEN** an active blue-owned session sends a valid heartbeat after promotion
- **THEN** its usage and lease are renewed against blue
- **AND** its tunnel remains active

### Requirement: Paused sessions follow current admission

A paused entitlement SHALL have no active generation owner and SHALL be activatable by the current accepting generation for its logical node.

#### Scenario: Paused blue entitlement resumes on green
- **GIVEN** a previously blue-owned session is paused with unused time
- **WHEN** it reconnects after green is promoted
- **THEN** it retains its remaining balance and tunnel address
- **AND** the response supplies green's current WireGuard public key and endpoint
- **AND** the active owner becomes green only after green confirms the peer

### Requirement: Reassignment waits for peer release

The coordinator SHALL NOT activate a session on a new generation until the previous generation has acknowledged peer removal or its last confirmed peer lease has expired.

#### Scenario: Pause removal is still pending
- **WHEN** a paused session attempts to resume before its old peer is confirmed absent
- **THEN** activation returns a retryable response
- **AND** the same session address is not active on both generations

### Requirement: Authoritative peer reconciliation

Each generation SHALL reconcile its actual managed peers to a coordinator-provided desired set and acknowledge the applied revision and actual peer count.

#### Scenario: Generation restarts with stale peers
- **WHEN** a generation restarts or reconnects to the coordinator
- **THEN** it removes peers absent from the desired set
- **AND** configures missing desired peers
- **AND** acknowledges the resulting revision and peer count

#### Scenario: Peer command fails
- **WHEN** a generation cannot apply a desired peer change
- **THEN** it does not acknowledge that revision as applied
- **AND** a pending activation is not exposed as active

### Requirement: Safe drain completion

A draining generation SHALL be eligible for deletion only after it owns zero active or transitional sessions, its desired and applied peer revisions match, and it reports zero managed peers.

#### Scenario: Paused entitlements remain
- **GIVEN** blue owns no active sessions or peers but logical-node paused entitlements still exist
- **WHEN** drain status is evaluated
- **THEN** blue is eligible for deletion
- **AND** the paused entitlements remain available through green

#### Scenario: Peer remains after final session pauses
- **WHEN** blue has zero publicly active sessions but still reports a peer or unapplied removal revision
- **THEN** blue is not eligible for deletion

### Requirement: Bounded active leases

Active peer leases SHALL be renewed every 30 seconds and SHALL become stale after 90 seconds without a successful renewal.

#### Scenario: Client abandons an active tunnel
- **WHEN** no valid heartbeat renews the session for 90 seconds
- **THEN** usage is charged only through the stale-timeout cutoff
- **AND** peer removal is required before the entitlement becomes reassignable
