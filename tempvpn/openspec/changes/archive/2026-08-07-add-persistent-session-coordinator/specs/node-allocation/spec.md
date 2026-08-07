## MODIFIED Requirements

### Requirement: Unique tunnel-address allocation

Each fixed paid entitlement SHALL receive at most one unique peer address from its logical node's configured tunnel network, independent of which generation currently owns its peer.

#### Scenario: First address is allocated
- **GIVEN** a supported `/24` logical-node tunnel network with no reservations
- **WHEN** a session first activates
- **THEN** the coordinator chooses the first available host address from `.2` through `.254`
- **AND** returns it as a `/32` peer address

#### Scenario: Existing addresses are skipped
- **GIVEN** one or more logical-node addresses are reserved by active or paused entitlements
- **WHEN** another session activates
- **THEN** the coordinator chooses an unreserved address

#### Scenario: Address pool is exhausted
- **GIVEN** every allocatable address from `.2` through `.254` is reserved
- **WHEN** a session without an address attempts activation
- **THEN** activation fails without duplicating an assignment

#### Scenario: Unsupported tunnel prefix is configured
- **WHEN** a generation is configured with a tunnel prefix other than the logical node's supported `/24`
- **THEN** registration or startup rejects the incompatible allocator configuration

### Requirement: Session-scoped address reservation

A tunnel address SHALL belong to one logical-node entitlement at a time. Pausing and generation reassignment SHALL preserve the reservation, while terminal cleanup SHALL release it.

#### Scenario: Paused session reconnects on another generation
- **GIVEN** a paused entitlement with a reserved address
- **WHEN** it reconnects on the accepting generation before expiration
- **THEN** it reuses the same tunnel address

#### Scenario: First activation fails
- **GIVEN** activation created a new address reservation
- **WHEN** WireGuard peer configuration fails
- **THEN** the coordinator releases that new reservation

#### Scenario: Session reaches terminal cleanup
- **WHEN** an expired entitlement is swept or an administrator terminates it
- **THEN** its tunnel address becomes available for another entitlement

#### Scenario: Generation shuts down
- **WHEN** a generation removes its peers during shutdown
- **THEN** paused entitlement reservations remain durable at the coordinator

### Requirement: Reconnection replaces peer identity

A reconnecting entitlement SHALL authorize only its current client public key for its reserved address across all generations.

#### Scenario: Client public key changes across generations
- **GIVEN** a session was previously connected with one public key
- **WHEN** it reconnects using a different public key
- **THEN** the old generation confirms the previous peer is absent
- **AND** the accepting generation authorizes only the new public key for the reserved address
