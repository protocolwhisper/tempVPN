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

### Requirement: Structured node location metadata

A node advertisement MAY include an ISO 3166-1 alpha-2 country code and optional
subdivision and city fields. The registry SHALL validate structured location
fields rather than inferring a country from the legacy free-form region field.

#### Scenario: Structured location is registered

- **GIVEN** a node advertises uppercase country code `DE` and city `Frankfurt`
- **WHEN** the registry accepts its lease
- **THEN** the live catalog preserves those structured fields

#### Scenario: Country code is invalid

- **WHEN** an advertisement supplies a country value that is not a supported ISO 3166-1 alpha-2 code
- **THEN** the registry rejects the advertisement without changing the catalog

#### Scenario: Legacy node has no country metadata

- **GIVEN** an active legacy advertisement has only a region value
- **WHEN** an unfiltered catalog is requested
- **THEN** the node remains eligible for unfiltered discovery
- **BUT WHEN** a country or city filter is requested
- **THEN** the registry does not guess a location for that node and excludes it

### Requirement: Node availability advertisement

A node advertisement SHALL expose whether the node is accepting new sessions
and its currently available tunnel-address capacity. A node SHALL refresh these
values with its lease, and the catalog SHALL treat them as advisory snapshots
rather than transactional reservations.

#### Scenario: Node has available capacity

- **GIVEN** a node is accepting sessions and has at least one allocatable tunnel address
- **WHEN** it refreshes its advertisement
- **THEN** the catalog marks it eligible for new-session discovery

#### Scenario: Node is draining

- **GIVEN** a node is configured not to accept new sessions
- **WHEN** it refreshes its advertisement
- **THEN** the catalog excludes it from discovery requiring availability

#### Scenario: Address capacity is exhausted

- **GIVEN** a node has no allocatable tunnel address
- **WHEN** it refreshes its advertisement
- **THEN** it reports zero available capacity
- **AND** the catalog excludes it from discovery requiring availability

### Requirement: Active node catalog

The live catalog SHALL expose only nodes with unexpired leases, SHALL support
optional structured country, city, legacy region, and availability filters, and
SHALL be available only when registry mode is enabled. When multiple filters are
provided, a returned node SHALL match all of them.

#### Scenario: Client requests the live catalog

- **GIVEN** registry mode is enabled
- **WHEN** a client requests `GET /nodes`
- **THEN** the registry removes expired entries
- **AND** returns active nodes ordered by node identifier

#### Scenario: Client requests eligible nodes in a country

- **GIVEN** registry mode is enabled
- **WHEN** a client requests active and available nodes with country code `DE`
- **THEN** every returned node advertises country code `DE`
- **AND** is accepting sessions with nonzero advertised capacity

#### Scenario: Client combines location filters

- **WHEN** a client requests country `DE` and city `Frankfurt`
- **THEN** every returned node matches both structured fields

#### Scenario: Catalog filter is invalid

- **WHEN** a client supplies an invalid country code or unsupported filter value
- **THEN** the registry rejects the request as invalid rather than returning an unfiltered catalog

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
explicit node URL, selection SHALL consider catalog nodes that match every
requested structured country, city, and legacy region filter and SHALL choose
the healthy candidate with the lowest measured median latency. Location filters
determine eligibility at the indexer; latency ranking SHALL occur on the user's
client.

#### Scenario: Select from the registry

- **GIVEN** the registry returns eligible candidate nodes
- **WHEN** the client optionally filters them by country, city, or region
- **THEN** it probes candidate health endpoints concurrently
- **AND** measures three requests per candidate with a two-second request timeout
- **AND** selects a candidate with the lowest median latency

#### Scenario: Explicit node bypasses discovery

- **GIVEN** the user supplies a node URL
- **WHEN** the client selects a node
- **THEN** it bypasses registry ranking
- **AND** health-checks the explicit node before accepting it

#### Scenario: No eligible or healthy candidate exists

- **WHEN** no candidate matches the requested filters or passes its health probes
- **THEN** selection fails before payment

#### Scenario: Selected candidate fails its final check

- **GIVEN** a catalog candidate passed latency ranking
- **WHEN** its immediate pre-payment health or availability check fails
- **THEN** the client does not pay the candidate
- **AND** may restart discovery without reusing the failed snapshot

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

Each fixed paid entitlement SHALL receive at most one unique peer address from
its logical node's configured tunnel network, independent of which generation
currently owns its peer.

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

A tunnel address SHALL belong to one logical-node entitlement at a time.
Pausing and generation reassignment SHALL preserve the reservation, while
terminal cleanup SHALL release it.

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

A reconnecting entitlement SHALL authorize only its current client public key
for its reserved address across all generations.

#### Scenario: Client public key changes across generations

- **GIVEN** a session was previously connected with one public key
- **WHEN** it reconnects using a different public key
- **THEN** the old generation confirms the previous peer is absent
- **AND** the accepting generation authorizes only the new public key for the reserved address
