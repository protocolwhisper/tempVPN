# Natural Language Discovery Specification

## Purpose

Translate ordinary-language VPN requests into deterministic, privacy-preserving
discovery filters and an exact eligible node selection before any payment.

## Requirements

### Requirement: Natural-language intent normalization

The skill SHALL normalize a user's request into an action, an optional duration
in seconds, optional structured location filters, and a selection policy without
forwarding the raw prompt to the indexer.

#### Scenario: Country and duration are normalized

- **GIVEN** the user requests `Connect 30 minutes to Germany`
- **WHEN** the skill resolves the request
- **THEN** it produces the `connect` action, a duration of `1800` seconds, and country code `DE`
- **AND** sends only structured discovery filters to the indexer

#### Scenario: Fastest is interpreted as a ranking policy

- **GIVEN** the user requests the fastest VPN in France
- **WHEN** the skill resolves the request
- **THEN** it produces country code `FR` and the `lowest_latency` selection policy
- **AND** does not treat `fastest` as a location or node identifier

#### Scenario: Fastest is requested without a location

- **GIVEN** the user requests the fastest VPN without naming a location
- **WHEN** the skill resolves the request
- **THEN** it applies the `lowest_latency` policy to eligible nodes globally

#### Scenario: Location is ambiguous

- **GIVEN** a location name has more than one reasonable country interpretation
- **WHEN** the skill cannot resolve one country code confidently
- **THEN** it asks the user to clarify
- **AND** does not initiate payment

#### Scenario: Required duration is missing

- **GIVEN** a connect request does not specify a session duration
- **WHEN** paid session creation requires that duration
- **THEN** the skill asks the user for it
- **AND** does not initiate payment

### Requirement: Structured indexer discovery

The skill and supported clients SHALL query the indexer with normalized catalog
filters rather than sending natural-language input to it. The indexer SHALL
return only active nodes matching every supplied location and availability
filter.

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

### Requirement: User-relative latency ranking

For the `lowest_latency` selection policy, the supported client SHALL measure
candidate latency from the user's device and select the healthy candidate with
the lowest median latency. Indexer proximity SHALL NOT be used as a substitute
for user-relative latency.

#### Scenario: Eligible candidates are latency ranked

- **GIVEN** the indexer returns multiple eligible candidates
- **WHEN** the client applies the `lowest_latency` policy
- **THEN** it probes the candidates concurrently from the user's device
- **AND** selects the healthy candidate with the lowest median latency

#### Scenario: No candidate passes probing

- **WHEN** none of the eligible candidates passes client-side health probing
- **THEN** selection fails before payment

### Requirement: Pre-payment eligibility recheck

The client SHALL recheck the selected node's health and advertised availability
immediately before payment. Catalog availability SHALL be treated as a snapshot,
not as a capacity reservation.

#### Scenario: Selected node becomes unavailable

- **GIVEN** a node was selected from an eligible catalog response
- **WHEN** its final health or availability check fails
- **THEN** the client does not pay that node
- **AND** may perform a new discovery and selection attempt

### Requirement: Exact-node execution binding

Discovery SHALL produce one exact normalized node API URL, and payment, session
activation, heartbeat, status, pause, and disconnect operations SHALL remain
bound to that node.

#### Scenario: Selection is handed to session creation

- **WHEN** discovery selects a node
- **THEN** the selection records its normalized API URL and advertised location
- **AND** session creation and payment use that exact API URL

#### Scenario: Paid response identifies another node

- **WHEN** a paid response identifies a node other than the selected node
- **THEN** the client rejects the response
- **AND** does not migrate the session implicitly

### Requirement: Post-connect result verification

After connection, the skill SHALL report success only after client status and
exit-IP verification succeed.

#### Scenario: Connection is verified

- **WHEN** the selected node connects successfully
- **THEN** the skill verifies active client status
- **AND** verifies that the public exit IP changed to the selected node's expected exit IP when one was advertised
- **AND** reports the selected node, advertised location, expiry, and verified exit IP

#### Scenario: Exit verification fails

- **WHEN** the public exit IP does not match the selected node's advertised exit IP
- **THEN** the skill does not claim that the requested VPN connection is active
- **AND** follows the safe failure or disconnect behavior defined by the client workflow

### Requirement: Discovery security boundary

Discovery SHALL use only public catalog and health data and SHALL NOT expose or
request node-admin credentials, registry-write credentials, session bearer
tokens, payment credentials, or client private keys.

#### Scenario: Public discovery is performed

- **WHEN** the skill discovers and ranks nodes
- **THEN** it uses public indexer and health interfaces
- **AND** secrets remain confined to their existing client-side or administrative boundaries
