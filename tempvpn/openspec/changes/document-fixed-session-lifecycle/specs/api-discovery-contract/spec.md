## Purpose

Define the machine-readable contract agents use to discover a node and complete the full fixed-duration VPN session lifecycle without relying on separate human documentation.

## ADDED Requirements

### Requirement: Publish the fixed-session lifecycle
The registry SHALL publish an OpenAPI 3.1 document that includes node discovery, paid fixed-session creation, session activation, and session pause operations with their exact HTTP methods and paths.

#### Scenario: Agent discovers the lifecycle
- **WHEN** an agent requests `GET /openapi.json`
- **THEN** the document includes `GET /nodes`, `POST /sessions`, `POST /sessions/{session_id}/connect`, and `POST /sessions/{session_id}/pause`

#### Scenario: Agent selects the execution server
- **WHEN** an agent follows a session lifecycle operation after node discovery
- **THEN** the document directs it to use the selected node's HTTPS `api_url` rather than the registry host

### Requirement: Describe session activation
The OpenAPI operation for `POST /sessions/{session_id}/connect` SHALL require a session identifier path parameter and a JSON body containing the caller's WireGuard public key. It SHALL describe the active session response, including the assigned tunnel address and connection parameters, and the retryable reconciliation response.

#### Scenario: Paid session starts paused
- **WHEN** paid creation returns a session whose state is `paused` with null `client_public_key` and `assigned_ip`
- **THEN** an agent can derive the connect request and required public-key field solely from the OpenAPI document

#### Scenario: Activation is still reconciling
- **WHEN** activation returns a retryable transition response
- **THEN** the document describes HTTP 409 and its retry semantics

### Requirement: Describe session pause
The OpenAPI operation for `POST /sessions/{session_id}/pause` SHALL require the session identifier path parameter, require no request body or daemon-admin credential, and describe the paused session response and not-found behavior.

#### Scenario: Agent preserves unused balance
- **WHEN** an agent no longer needs an active tunnel
- **THEN** it can derive the pause request solely from the OpenAPI document and stop connected-time consumption without deleting the session

### Requirement: Prevent lifecycle documentation drift
Automated contract validation SHALL fail when a required fixed-session operation, parameter, request schema, or session-state response is absent from the published document.

#### Scenario: Required operation is removed
- **WHEN** a change removes connect or pause from the OpenAPI document
- **THEN** automated tests fail before deployment
