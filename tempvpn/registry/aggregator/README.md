# TempVPN global registry aggregator

The aggregator combines the configured regional node catalogs and publishes the machine-readable TempVPN API contract.

## Public routes

- `GET /health` reports regional upstream reachability.
- `GET /nodes` returns the merged active node catalog.
- `POST /sessions` purchases a paused fixed-session balance on a selected node.
- `POST /sessions/{session_id}/connect` activates a paused balance on a selected node.
- `GET /sessions/{session_id}/status`, `POST .../heartbeat`, and `POST .../pause` manage the fixed-session lifecycle.
- `POST /sessions/stream` proxies the separate node-bound Session v2 SSE product without imposing a total response timeout.
- `HEAD /sessions/stream` proxies voucher, top-up, resume, and close operations to the same node selected by `node_id`.
- `GET /openapi.json` describes the complete registry-hosted API.
- `GET /docs` redirects human readers to `https://tempvpn.xyz/docs/`; `GET /docs/markdown` serves the agent-readable workflow (`/docs/markdown.md` remains an alias).

The registry is the public control-plane origin for discovery, payment, and lifecycle calls. Clients select a node from `/nodes`, send its `id` as `node_id` when purchasing or connecting, and keep all HTTP requests at the registry origin. A node's `api_url` is diagnostic metadata, not a client payment origin.

`POST /sessions` creates a paid but paused fixed-duration balance. Fixed access costs $0.01 per minute: requested duration uses seconds and must be a positive multiple of 60 (`$0.01 × (duration_seconds / 60)`). The runtime MPP 402 challenge is authoritative for payment details. Clients must then call `POST /sessions/{session_id}/connect` with a selected `node_id` and locally generated WireGuard public key. They should call `POST /sessions/{session_id}/pause` when disconnecting so connected-time consumption stops while unused balance remains portable to another available node.

After deploying an OpenAPI change, update the separately maintained MPP directory entry with the same methods and paths, reindex it, and verify the directory copy includes status, connect, pause, and heartbeat. The separately maintained website docs must mirror this registry-first workflow. Streaming remains a separate, non-portable metered product.
