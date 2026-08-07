## Context

The live fleet has two public in-memory registry catalogs: US East advertises the three Americas nodes and Belgium advertises the three Europe/Asia nodes. Existing clients accept one registry URL and already understand the `/nodes` response shape and structured query parameters. DNS for `tempvpn.xyz` is hosted at Namecheap, and the apex is reserved for a future landing page.

## Goals / Non-Goals

**Goals:**

- Preserve both regional registries as independent sources and direct fallbacks.
- Give browsers and agents one stable, managed-HTTPS URL containing all discoverable nodes.
- Keep aggregation stateless, secretless, inexpensive at MVP traffic, and independently removable.
- Preserve node-bound payment and connection behavior after discovery.

**Non-Goals:**

- Synchronizing or writing to either regional registry.
- Proxying payment, session, WireGuard, or administrative traffic through the aggregator.
- Adding HTTPS to every node API as part of this change.
- Changing Linux or macOS tunnel behavior.
- Taking ownership of the apex or `www` DNS records.

## Decisions

### Stateless Rust HTTP service

A small Rust/Axum service will query the two configured upstream `/nodes` endpoints concurrently, forward only the supported structured filters, validate JSON arrays, deduplicate by node ID using the later lease expiry, and sort by node ID. It will expose `/nodes` and `/health` with permissive CORS limited to public read methods. This reuses repository language and response semantics while avoiding a new persistent database.

An HTTP reverse proxy or round-robin DNS was rejected because neither can merge two JSON catalogs. Changing every client to understand multiple registries remains a possible future enhancement but would require coordinated Linux and macOS releases.

### Partial results with an explicit degraded signal

Each upstream receives a short request timeout. One successful upstream is enough for HTTP 200, accompanied by an `X-TempVPN-Degraded: true` header when another fails. Total failure returns 503. This keeps available regions discoverable without inventing stale entries; the client still performs its existing pre-payment node health check.

### Cloud Run with source built by the Terraform workflow

The service will run on Cloud Run with scale-to-zero, a small instance limit, public unauthenticated invocation, and upstream URLs supplied as non-secret environment variables. Terraform enables Cloud Run, Artifact Registry, and Cloud Build APIs, creates a dedicated image repository, invokes a content-addressed Cloud Build for the local aggregator source, and deploys the resulting immutable image tag. No registry-write, daemon-admin, MPP, wallet, or client key enters the service.

Cloud Run was selected over another VM to avoid fixed idle compute and certificate maintenance. Hosting on a registry VM was rejected because it would couple global discovery lifecycle to a regional daemon and require operating a separate TLS proxy there.

### Subdomain mapping and manual Namecheap verification

The production name is `registry.tempvpn.xyz`. Google manages the TLS certificate. Terraform manages the Cloud Run domain mapping after Google recognizes domain ownership; Namecheap remains authoritative for DNS. If ownership verification or mapping records require manual DNS, deployment pauses after the default Cloud Run URL is healthy and reports the exact TXT/CNAME records for the user to add. No Namecheap password or API key is stored.

### Agent default with override

The tempVPN skill will supply `https://registry.tempvpn.xyz` when no registry is explicitly configured. An existing `VPN_CLIENT_REGISTRY_URL` or equivalent explicit option wins, preserving development and regional fallback workflows. Linux and macOS binaries remain protocol-compatible and unchanged.

## Risks / Trade-offs

- [Custom-domain verification requires a manual Namecheap record] → Deploy and verify the default Cloud Run URL first, then surface exact ownership/mapping records and resume after DNS propagates.
- [One upstream failure yields an incomplete catalog] → Mark the response degraded and never fabricate or cache failed records.
- [Cloud Run cold starts add discovery latency] → Keep the service small, requests concurrent, and timeout bounded; MVP traffic favors scale-to-zero cost.
- [Regional node APIs still use HTTP] → The aggregator is suitable for HTTPS discovery and landing-page display; direct browser payment remains blocked until node APIs receive HTTPS.
- [Terraform local-exec depends on authenticated `gcloud`] → Validate the active project/account and use a content hash so unchanged source does not rebuild.

## Migration Plan

1. Build and unit-test aggregation, filtering, deduplication, CORS, and failure behavior locally.
2. Apply Artifact Registry, build, Cloud Run, and public invocation resources while leaving the live agent default unchanged.
3. Verify the default Cloud Run URL returns both three-node catalogs.
4. Verify `tempvpn.xyz` ownership with Google and add only the required Namecheap records for `registry`.
5. Wait for managed TLS, verify `https://registry.tempvpn.xyz/nodes`, then update the skill default.
6. Run discovery against the global endpoint and retain both regional URLs in the runbook.

Rollback sets the skill or operator override to either regional registry, removes the `registry` DNS record after its TTL, and destroys only the aggregator/domain resources. Regional registries, payments, sessions, and VPN nodes remain untouched.
