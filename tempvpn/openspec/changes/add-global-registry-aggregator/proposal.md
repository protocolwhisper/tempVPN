## Why

The two live registry nodes expose separate three-node catalogs, so a user or agent configured with one registry cannot discover the complete six-node fleet. The public launch also needs a stable HTTPS discovery URL under `tempvpn.xyz` that browsers and agents can use without receiving server credentials.

## What Changes

- Add a stateless global registry aggregator that fetches the Americas and Europe/Asia catalogs and returns one deduplicated, availability-filtered node list.
- Expose health and node-discovery endpoints with bounded upstream timeouts, partial-failure behavior, and browser-safe CORS.
- Containerize and deploy the aggregator to Google Cloud Run with Terraform-managed runtime configuration and public unauthenticated read access.
- Map `registry.tempvpn.xyz` to the service with Google-managed HTTPS, leaving the apex landing-page domain untouched.
- Make the tempVPN agent skill use the global HTTPS registry by default while preserving an explicit registry override.
- Document Namecheap DNS verification/records and rollback to either direct regional registry.
- Do not alter node payment recipients, MPP payment flow, daemon credentials, paid-session lifecycle, WireGuard routing, or client private-key handling.

## Capabilities

### New Capabilities

- `global-registry-discovery`: A single public HTTPS catalog combines healthy leased nodes from both regional registries without exposing write or administrative credentials.

### Modified Capabilities

- `natural-language-discovery`: Agent-driven selection defaults to the production global registry while retaining an operator-supplied registry override.

## Impact

- New aggregator service and container build artifacts.
- `infra/terraform`: Cloud Run, Artifact Registry/build delivery, public invocation, domain mapping, outputs, and runbook updates.
- `agent/SKILL.md`: default production discovery endpoint and override behavior.
- Namecheap DNS for the `registry` subdomain only; `tempvpn.xyz` and `www.tempvpn.xyz` remain unchanged.
- Linux and macOS client protocols remain compatible because the aggregator preserves the existing `/nodes` response shape.
- Rollback changes the agent registry URL back to a regional endpoint and removes only aggregator resources after DNS is reverted.
