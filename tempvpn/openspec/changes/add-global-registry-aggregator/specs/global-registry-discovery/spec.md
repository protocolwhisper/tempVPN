## Purpose

Provide one stable public HTTPS discovery endpoint that combines the independently leased regional tempVPN catalogs for browser and agent consumers.

## ADDED Requirements

### Requirement: Global catalog aggregation

The global registry SHALL query every configured regional registry concurrently and SHALL return one catalog containing the active node records available from those registries.

#### Scenario: Both regional registries are healthy
- **WHEN** a consumer requests the global `/nodes` endpoint
- **THEN** the service queries the Americas and Europe/Asia registries
- **AND** returns nodes from both catalogs in the existing node-record JSON shape

#### Scenario: Structured filters are supplied
- **WHEN** a consumer supplies supported country, city, region, or availability filters
- **THEN** the global registry forwards only those structured filters to each regional registry
- **AND** returns only records matching the combined filter criteria

### Requirement: Stable deduplication

The global registry SHALL return at most one record per node ID and SHALL produce deterministic ordering independent of upstream response order.

#### Scenario: The same node appears in multiple catalogs
- **WHEN** two regional responses contain the same node ID
- **THEN** the global response includes that node exactly once
- **AND** retains the record with the later lease expiry

### Requirement: Bounded partial-failure behavior

The global registry SHALL bound each upstream request and SHALL continue serving a successful regional catalog when another configured registry is unavailable.

#### Scenario: One regional registry fails
- **WHEN** one upstream times out or returns an unsuccessful response and another succeeds
- **THEN** `/nodes` returns the successful catalog with HTTP 200
- **AND** marks the response as degraded without fabricating nodes

#### Scenario: Every regional registry fails
- **WHEN** no configured upstream returns a valid catalog
- **THEN** `/nodes` returns HTTP 503 with a non-sensitive error body

### Requirement: Public HTTPS discovery boundary

The production global registry SHALL be available at `https://registry.tempvpn.xyz`, SHALL permit browser GET requests through CORS, and SHALL expose only public discovery and health data.

#### Scenario: Browser requests the global catalog
- **WHEN** a browser sends a GET or preflight request for `/nodes`
- **THEN** the service returns compatible CORS headers for public read access
- **AND** does not request or expose registry-write, daemon-admin, MPP-signing, payment-account, session, or client-private-key credentials

#### Scenario: Landing page domain remains independent
- **WHEN** the registry subdomain is configured
- **THEN** DNS and certificate changes affect only `registry.tempvpn.xyz`
- **AND** do not replace `tempvpn.xyz` or `www.tempvpn.xyz`

### Requirement: Global registry health

The global registry SHALL expose health state that distinguishes a fully available catalog, a partially available catalog, and total upstream failure.

#### Scenario: Health is inspected
- **WHEN** a consumer requests `/health`
- **THEN** the response reports each configured upstream's reachability
- **AND** contains no upstream credentials or internal exception details
