# Node Allocation Specification

## Purpose

Define how VPN nodes advertise availability, how clients select a node, how
payment remains bound to that selection, and how the selected node assigns a
unique tunnel address to a session.

## Requirements

### Requirement: Leased node registration

A registry-mode daemon SHALL accept node advertisements only through the
authenticated registry-write endpoint. The registry credential SHALL be
separate from the daemon-admin credential.

#### Scenario: Node registers a valid lease

- **GIVEN** registry mode is enabled
- **AND** the caller supplies the configured registry token
- **WHEN** a node advertisement has an identifier matching the URL path
- **AND** the identifier contains only letters, digits, `.`, `_`, or `-`
- **THEN** the registry stores or replaces that node record
- **AND** assigns a lease expiration based on the configured lease duration
- **AND** normalizes a trailing slash from the advertised API URL

#### Scenario: Invalid advertisement is rejected

- **WHEN** the advertised identifier differs from the URL path or contains unsupported characters
- **THEN** the registry rejects the advertisement without changing the catalog

#### Scenario: Registry write is unauthorized

- **WHEN** a caller registers or removes a node without the configured registry token
- **THEN** the registry rejects the request as unauthorized

### Requirement: Active node catalog

The live catalog SHALL expose only nodes with unexpired leases and SHALL be
available only when registry mode is enabled.

#### Scenario: Client requests the live catalog

- **GIVEN** registry mode is enabled
- **WHEN** a client requests `GET /nodes`
- **THEN** the registry removes expired entries
- **AND** returns active nodes ordered by node identifier

#### Scenario: Catalog is requested from a non-registry node

- **GIVEN** registry mode is disabled
- **WHEN** a client requests `GET /nodes`
- **THEN** the daemon returns not found

#### Scenario: Node unregisters

- **GIVEN** an active leased node
- **WHEN** an authorized caller removes its registry record
- **THEN** the node no longer appears in the live catalog

### Requirement: Node lease maintenance

A node configured with a registry URL and token SHALL refresh its advertisement
periodically without coupling registry availability to its VPN service.

#### Scenario: Lease refresh succeeds

- **WHEN** a node successfully refreshes its lease
- **THEN** it waits the configured refresh interval before refreshing again
- **AND** resets its retry backoff

#### Scenario: Registry is unavailable

- **WHEN** a refresh request fails or is rejected
- **THEN** the node keeps its VPN service online
- **AND** retries with exponential backoff capped at 60 seconds

#### Scenario: Node shuts down gracefully

- **WHEN** a registered node shuts down
- **THEN** it attempts to remove its registry lease

### Requirement: Health-based client selection

A client SHALL select a node before payment. Unless the user supplies an
explicit node URL, selection SHALL consider catalog nodes that match the
requested region and SHALL choose the healthy candidate with the lowest
measured median latency.

#### Scenario: Select from the registry

- **GIVEN** the registry returns candidate nodes
- **WHEN** the client optionally filters them by region
- **THEN** it probes candidate health endpoints concurrently
- **AND** measures three requests per candidate with a two-second request timeout
- **AND** selects a candidate with the lowest median latency

#### Scenario: Explicit node bypasses discovery

- **GIVEN** the user supplies a node URL
- **WHEN** the client selects a node
- **THEN** it bypasses registry ranking
- **AND** health-checks the explicit node before accepting it

#### Scenario: No candidate is healthy

- **WHEN** no region-matching candidate passes its health probes
- **THEN** selection fails before payment

### Requirement: Bounded catalog fallback

Supported clients SHALL cache a successfully fetched catalog and MAY use that
cache during a temporary registry outage only within the configured cache
lifetime.

#### Scenario: Registry fetch succeeds

- **WHEN** a client receives a live catalog
- **THEN** it records the catalog and fetch time in its local cache

#### Scenario: Registry fetch fails with a fresh cache

- **GIVEN** the catalog cache is within its allowed lifetime
- **WHEN** the live registry cannot be fetched
- **THEN** the client may health-check cached nodes, including nodes whose recorded lease has since expired
- **AND** selects only a node whose health endpoint responds successfully

#### Scenario: Registry fetch fails without a fresh cache

- **WHEN** the registry is unavailable and no cache exists within its allowed lifetime
- **THEN** node selection fails before payment

### Requirement: Selected-node payment binding

The node chosen during selection SHALL be the node used for payment, session
activation, heartbeats, status, and pause operations.

#### Scenario: Paid session is imported on the selected node

- **GIVEN** a paid response identifies its creating node URL
- **WHEN** the client imports that response with a selected-node URL
- **THEN** the client normalizes the URLs and continues only if they identify the same node

#### Scenario: Paid response belongs to another node

- **WHEN** the paid response's node URL differs from the selected-node URL
- **THEN** the client rejects the import without attempting to migrate the session

### Requirement: Unique tunnel-address allocation

Each activated session SHALL receive a unique peer address from the selected
node's configured tunnel network.

#### Scenario: First address is allocated

- **GIVEN** a supported `/24` tunnel network with no reservations
- **WHEN** a session is activated
- **THEN** the allocator chooses the first available host address from `.2` through `.254`
- **AND** returns it as a `/32` peer address

#### Scenario: Existing addresses are skipped

- **GIVEN** one or more addresses are already reserved
- **WHEN** another session is activated
- **THEN** the allocator chooses an unreserved address

#### Scenario: Address pool is exhausted

- **GIVEN** every allocatable address from `.2` through `.254` is reserved
- **WHEN** a session without an address attempts activation
- **THEN** activation fails without duplicating an assignment

#### Scenario: Unsupported tunnel prefix is configured

- **WHEN** the daemon is configured with a tunnel prefix other than `/24`
- **THEN** startup rejects the unsupported allocator configuration

### Requirement: Session-scoped address reservation

A tunnel address SHALL belong to one session at a time. Pausing SHALL preserve
the reservation for reconnection, while terminal cleanup SHALL release it.

#### Scenario: Paused session reconnects

- **GIVEN** a paused session with a reserved tunnel address
- **WHEN** that session reconnects before expiration
- **THEN** it reuses the same tunnel address

#### Scenario: New activation fails

- **GIVEN** activation reserved a new address
- **WHEN** WireGuard peer configuration fails
- **THEN** the node releases that reservation

#### Scenario: Session reaches terminal cleanup

- **WHEN** an expired session is swept, an administrator deletes it, or daemon shutdown clears it
- **THEN** its tunnel address becomes available for another session

### Requirement: Reconnection replaces peer identity

A reconnecting session SHALL authorize only its current client public key for
its reserved address.

#### Scenario: Client public key changes

- **GIVEN** a session was previously connected with one public key
- **WHEN** it reconnects using a different public key
- **THEN** the node removes the previous peer
- **AND** authorizes the new public key for the session's reserved address

