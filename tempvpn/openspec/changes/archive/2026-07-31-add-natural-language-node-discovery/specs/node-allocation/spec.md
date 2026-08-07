## ADDED Requirements

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

## MODIFIED Requirements

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
