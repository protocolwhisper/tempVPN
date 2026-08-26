## Why

The advertised streaming-session method and the node runtime disagree: discovery clients are directed to `POST /sessions/stream`, while the daemon currently exposes a payable `GET` variant that can bind a challenge to a scanner placeholder instead of a usable WireGuard key. A client can therefore pay for a tunnel it cannot activate.

## What Changes

- **BREAKING**: Make `POST /sessions/stream` the only method that creates or resumes a metered streaming VPN session.
- Reject `GET /sessions/stream` with a non-payable `405 Method Not Allowed` response.
- Validate the caller's WireGuard public key and requested duration before issuing any MPP challenge.
- Keep `HEAD /sessions/stream` exclusively for authenticated Session v2 management operations.
- Reserve the static streaming path even when streaming payments are disabled so it cannot fall through to a dynamic session route.
- Align tracked endpoint documentation and add regression coverage for method, validation, and challenge behavior.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `vpn-streaming-payments`: Define the safe HTTP method contract and require validation before a payable challenge is issued.

## Impact

- Node daemon: Axum routing, streaming request extraction, validation, and route tests.
- Registry/discovery: the external registry, OpenAPI, and `llms.txt` must advertise `POST`; those artifacts are not present in this checkout and must be updated by their owner.
- Documentation: the repository README changes from GET to POST semantics.
- Clients: no tracked Linux or macOS streaming client currently calls this route; one-time `POST /sessions` remains unchanged.
- Payment behavior changes by preventing challenges for unsupported methods or unusable keys. Credential verification, expiry, and network routing remain unchanged.
- Compatibility: legacy GET callers receive 405 and must migrate to POST. Rollback can restore the prior handler, but doing so reopens the payable-placeholder failure and is not recommended.
