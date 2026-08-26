## Why

The published OpenAPI document omits the `connect` and `pause` operations required to use and safely stop a fixed-duration VPN session. An agent following only the machine-readable contract can pay for a paused session but cannot attach its WireGuard key or preserve unused balance.

## What Changes

- Make the registry aggregator the source-controlled publisher of the TempVPN OpenAPI document.
- Add `POST /sessions/{session_id}/connect` with its path parameter, WireGuard public-key request, session response, and retryable transition response.
- Add `POST /sessions/{session_id}/pause` with its path parameter, session response, and idempotent lifecycle semantics.
- Document that paid session creation initially returns a paused session without a client key or assigned address.
- Add contract tests that require the purchase, connect, and pause operations and their shared schemas to remain present.

## Capabilities

### New Capabilities

- `api-discovery-contract`: Publish a machine-readable API contract that describes every operation required to discover, purchase, activate, and pause a fixed-duration VPN session.

### Modified Capabilities

None.

## Impact

- Registry aggregator: adds a public, static `GET /openapi.json` route and contract tests.
- Node daemon: runtime behavior is unchanged; the document mirrors its existing public routes and response model.
- Directory: its separate endpoint list must add connect and pause after this document is deployed; that repository is outside this change.
- Linux and macOS clients, agent skill, configuration, and infrastructure behavior remain unchanged.
- Payment amounts, credentials, session expiry, and network routing do not change.
- Compatibility and rollback: this is additive. Rolling back removes machine-readable lifecycle coverage but does not alter live node endpoints.
