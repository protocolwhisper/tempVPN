# TempVPN global registry aggregator

The aggregator combines the configured regional node catalogs and publishes the machine-readable TempVPN API contract.

## Public routes

- `GET /health` reports regional upstream reachability.
- `GET /nodes` returns the merged active node catalog.
- `GET /openapi.json` describes discovery and the node-hosted fixed and streaming session lifecycle.

The registry does not proxy payments or VPN lifecycle calls. After selecting a node from `/nodes`, clients must substitute that record's HTTPS `api_url` for the `{api_url}` server variable in session operations.

`POST /sessions` creates a paid but paused fixed-duration balance. Pricing is $0.01 per minute; duration must be a whole number of minutes. Clients must then call `POST /sessions/{session_id}/connect` with a locally generated WireGuard public key. They should call `POST /sessions/{session_id}/pause` when disconnecting so connected-time consumption stops while unused balance remains available.

After deploying an OpenAPI change, update the separately maintained MPP directory entry with the same methods and paths, reindex it, and verify the directory copy still includes connect and pause. Preserve the existing production ownership of `/docs` and `/llms.txt` when rolling out a new aggregator image.
