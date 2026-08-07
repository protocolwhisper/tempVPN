## MODIFIED Requirements

### Requirement: Structured indexer discovery

The skill and supported clients SHALL query the indexer with normalized catalog filters rather than sending natural-language input to it. Agent-driven discovery SHALL use `https://registry.tempvpn.xyz` when no explicit registry override is supplied. The indexer SHALL return only active nodes matching every supplied location and availability filter.

#### Scenario: Production registry default is used
- **GIVEN** the tempVPN agent performs discovery without an explicit registry override
- **WHEN** it invokes the supported platform client
- **THEN** it configures `https://registry.tempvpn.xyz` as the registry URL
- **AND** the client queries the combined global catalog

#### Scenario: Explicit registry override is used
- **GIVEN** an operator supplies a registry URL explicitly
- **WHEN** the agent performs discovery
- **THEN** it preserves that URL instead of replacing it with the production default

#### Scenario: Country-filtered discovery succeeds
- **GIVEN** the normalized request contains country code `DE`
- **WHEN** the client queries the indexer
- **THEN** it requests eligible nodes using the structured country filter `DE`
- **AND** receives no node advertised for another country

#### Scenario: No eligible location match exists
- **WHEN** the indexer returns no eligible node matching the normalized filters
- **THEN** the skill reports that no node is currently available for the requested location
- **AND** does not silently substitute another country
- **AND** does not initiate payment
