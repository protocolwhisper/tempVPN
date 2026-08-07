## 1. Aggregator service

- [ ] 1.1 Add a workspace Rust service with environment-driven upstreams, bounded concurrent fetches, structured-filter forwarding, deterministic deduplication, and `/nodes` and `/health` routes.
- [ ] 1.2 Add CORS and degraded/total-failure responses without exposing upstream or platform secrets.
- [ ] 1.3 Add automated tests covering six-node merge, filter forwarding, duplicate resolution, partial failure, total failure, health, and CORS.
- [ ] 1.4 Add a minimal production container image and local container health verification.

## 2. Terraform deployment

- [ ] 2.1 Extend provider/API configuration for Artifact Registry, Cloud Build, and Cloud Run.
- [ ] 2.2 Add a content-addressed Terraform build step, container repository, Cloud Run service, dedicated runtime identity, and public read invocation.
- [ ] 2.3 Add optional `registry.tempvpn.xyz` domain mapping, outputs, Namecheap record instructions, and aggregator-only rollback documentation.
- [ ] 2.4 Validate a refreshed plan does not replace or destroy any daemon-fleet resource.

## 3. Live rollout and agent integration

- [ ] 3.1 Apply the reviewed aggregator plan and verify the default Cloud Run URL returns both live regional catalogs.
- [ ] 3.2 Determine Google domain-verification and Namecheap mapping records, apply the mapping when ownership permits, and verify managed HTTPS when DNS is available.
- [ ] 3.3 Update the tempVPN agent skill to default to `https://registry.tempvpn.xyz` only after that endpoint is healthy, while preserving explicit overrides and direct regional fallbacks.
- [ ] 3.4 Run strict OpenSpec validation, Terraform drift validation, service tests, global live discovery, and final local-branch review.
