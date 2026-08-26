## MODIFIED Requirements

### Requirement: Negotiate a current Tempo payment session
The node SHALL accept streaming-session creation or resumption only through `POST /sessions/stream` with a valid WireGuard public key and duration in the request body. It SHALL validate those fields before challenging an unpaid request with MPP method `tempo`, intent `session`, and method detail `sessionProtocol: "v2"`. The challenge SHALL identify the configured currency and recipient, price access in time units, include a suggested reserve sufficient for more than one billing unit, and bind the exact validated key and duration into its opaque data.

#### Scenario: Client starts without a credential
- **WHEN** a client posts a valid streaming VPN request without an MPP credential
- **THEN** the node responds with HTTP 402 and a current Tempo Session v2 challenge bound to the supplied key and duration

#### Scenario: Legacy session client attempts payment
- **WHEN** a credential uses the legacy contract-backed session protocol
- **THEN** the node rejects it without creating a VPN peer

#### Scenario: Placeholder or malformed public key
- **WHEN** a client posts a public key that is not a canonical WireGuard public key
- **THEN** the node responds with HTTP 400 without a payment challenge or VPN peer

#### Scenario: Invalid duration
- **WHEN** a client posts a zero duration or a duration above the node limit
- **THEN** the node responds with HTTP 400 without a payment challenge or VPN peer

## ADDED Requirements

### Requirement: Separate stream creation from management
The node SHALL reserve `/sessions/stream` as a static endpoint. `POST` SHALL be the only method that creates or resumes a metered stream, `HEAD` SHALL be limited to authenticated Session v2 management, and `GET` SHALL return HTTP 405 without issuing a payment challenge.

#### Scenario: Discovery probe uses GET
- **WHEN** a caller sends `GET /sessions/stream`
- **THEN** the node responds with HTTP 405, advertises the allowed methods, and does not include an MPP payment challenge

#### Scenario: Management request uses HEAD
- **WHEN** a caller sends a valid authenticated Session v2 management operation with `HEAD /sessions/stream`
- **THEN** the node performs only that management operation and does not create a new VPN peer

#### Scenario: Streaming payments are disabled
- **WHEN** a caller sends POST or HEAD to `/sessions/stream` while streaming payments are disabled
- **THEN** the node returns a non-payable streaming-disabled response and does not dispatch the request as a dynamic session identifier
