## Context

See `proposal.md` for motivation. The Axum router currently registers GET and HEAD only when streaming is enabled. Both handlers take the client key and duration from the query string, and validation accepts any non-empty key before constructing an MPP challenge. When streaming is disabled, the unreserved path can match `/sessions/{session_id}`.

The tracked Linux and macOS clients use the one-time session API and do not currently consume the streaming route. No OpenAPI or `llms.txt` source is tracked in this checkout.

## Goals / Non-Goals

**Goals:**

- Give session creation state-changing POST semantics while preserving an SSE response.
- Guarantee unsupported methods and invalid keys cannot produce payable challenges.
- Preserve HEAD-based Session v2 management and one-time session behavior.
- Keep the path deterministic whether streaming is enabled or disabled.

**Non-Goals:**

- Redesigning Session v2 credential verification, accounting, or settlement.
- Changing client private-key ownership, WireGuard routing, or session expiry.
- Editing external registry, OpenAPI, or `llms.txt` artifacts that are absent from this repository.

## Decisions

### Use JSON POST for stream creation

`POST /sessions/stream` accepts `client_public_key` and `duration_seconds` as JSON and returns the existing SSE body after successful authentication. POST accurately represents peer creation and metering side effects. A redirect from GET is rejected because it cannot safely synthesize or preserve the required JSON request.

### Fail GET explicitly and reserve the route unconditionally

The router always registers the static path. GET returns 405 with `Allow: POST, HEAD` and never invokes payment code. When streaming is disabled, POST and HEAD return the existing non-payable disabled response rather than falling through to `/sessions/{session_id}`.

### Validate WireGuard public keys before payment negotiation

The daemon strictly decodes standard base64 and requires exactly 32 decoded bytes, which is the WireGuard public-key size. Validation happens before opaque construction or challenge issuance. This rejects scanner placeholders and malformed keys while leaving the private key entirely client-owned.

HEAD continues to carry its binding values in the query because Session v2 management operations have no JSON body in the current protocol integration. It receives the same pre-challenge validation.

### Keep payment binding unchanged after validation

The exact validated public-key string and duration remain HMAC-bound in the MPP opaque. No durable-state or secret ownership changes are introduced.

## Risks / Trade-offs

- [Legacy callers using GET stop working] → Return a clear 405 and update every discoverable contract to POST; do not retain a payable compatibility path.
- [Strict validation breaks tests or callers using dummy keys] → Replace route-level fixtures with canonical 32-byte base64 test keys and document the constraint.
- [External discovery metadata remains stale] → Treat registry/OpenAPI/`llms.txt` correction and reindexing as a deployment follow-up that is required to close the production issue.
- [Disabled nodes expose a slightly different static response] → Prefer a deterministic non-payable response over accidental dynamic-route dispatch.

## Migration Plan

1. Deploy the POST handler, GET gate, static disabled behavior, and validation together.
2. Probe GET and malformed POST without credentials and confirm neither returns 402 or `WWW-Authenticate`.
3. Probe a valid POST and confirm its 402 opaque contains the exact submitted key and duration.
4. Update and reindex the external registry, OpenAPI, and `llms.txt` to advertise POST.
5. Run one real-wallet canary and then roll out to remaining streaming-enabled nodes.

Rollback may restore the prior binary, but that re-enables payable GET challenges and should be used only while the endpoint is removed from discovery or streaming is disabled.
