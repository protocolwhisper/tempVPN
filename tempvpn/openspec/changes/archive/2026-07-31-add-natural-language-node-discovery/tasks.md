## 1. Node Metadata and Configuration

- [x] 1.1 Add optional country code, subdivision code, and city configuration to the Rust node's file and environment loaders, plus a draining/accept-new-sessions setting.
- [x] 1.2 Add canonical ISO country validation and optional-text normalization, with unit tests for valid, lowercase-normalized, empty, unsupported, and legacy values.
- [x] 1.3 Extend node advertisement and catalog models with backward-compatible location, `accepting_sessions`, and `available_slots` fields and update JSON fixtures.
- [x] 1.4 Update example node configuration, Terraform/startup inputs, and deployment output so operators can advertise structured location without exposing registry or admin secrets.

## 2. Availability and Indexer Filtering

- [x] 2.1 Add a session-store capacity snapshot derived under its lock from allocatable `/24` addresses and current session-scoped reservations.
- [x] 2.2 Extend `/health` with live `accepting_sessions` and `available_slots` values and test draining, positive-capacity, and exhausted-capacity responses.
- [x] 2.3 Rebuild each registry advertisement on every lease refresh so location and dynamic availability are current, while preserving retry and shutdown behavior.
- [x] 2.4 Add validated `country`, `city`, `region`, and `available` query extraction to `GET /nodes`, applying conjunctive filters and stable node-ID ordering.
- [x] 2.5 Add registry and route tests for combined filters, invalid filters returning `400`, expired leases, unavailable nodes, and legacy-node behavior in filtered and unfiltered catalogs.

## 3. Linux Client Discovery

- [x] 3.1 Add `--country`, `--city`, and selection-policy inputs to the Rust CLI while preserving explicit-node and legacy-region workflows.
- [x] 3.2 Send normalized structured query parameters to the indexer, decode additive catalog metadata, and apply identical filters to fresh cached catalogs.
- [x] 3.3 Bound concurrent three-sample health probes and select the lowest median RTT from the user's device with deterministic tie behavior.
- [x] 3.4 Add an immediate live health/availability recheck between selection and the `mppx` paid request, failing or rediscovering without payment when eligibility changed.
- [x] 3.5 Add Rust client tests for Germany/France filters, global fastest selection, combined location filters, ambiguous data exclusion, cached discovery, latency ranking, and final-check failure before payment.

## 4. macOS Client Discovery

- [x] 4.1 Extend Swift `Codable` catalog and health models and `tempvpnctl` argument parsing with the same country, city, region, and selection-policy contract as Linux.
- [x] 4.2 Build structured indexer queries with `URLComponents`, apply the same cached-catalog eligibility rules, and preserve explicit-node selection.
- [x] 4.3 Implement bounded concurrent `URLSession` probes, three-sample median ranking, deterministic ties, and the final live availability recheck before invoking the paid request.
- [x] 4.4 Preserve the selected API URL and advertised location through profile handoff to the signed host app without moving private-key generation or Keychain ownership into discovery.
- [x] 4.5 Add Swift tests using stubbed catalog and health responses for filtered/global selection, no matches, unhealthy candidates, cache behavior, exact-node binding, and no payment after final-check failure.

## 5. Agent Skill and User Workflow

- [x] 5.1 Update the TempVPN skill with the normalized intent fields for action, duration seconds, ISO country code, optional city/region, and `lowest_latency`, without embedding an exhaustive country phrase list.
- [x] 5.2 Document and test representative interpretations including `Connect 30 mins to Germany`, `Use a German VPN for one hour`, `Get me the fastest VPN in France`, global `fastest`, status, and disconnect.
- [x] 5.3 Add clarification and no-payment rules for ambiguous locations, missing required duration, invalid ISO codes, no eligible matches, and candidates that fail probing or final recheck.
- [x] 5.4 Ensure the skill sends only structured filters to the local client/indexer, never logs or forwards raw prompts to discovery infrastructure, and keeps all existing credential and private-key boundaries.
- [x] 5.5 Report the exact selected node, advertised location, expiry, and verified exit IP only after connection/status verification succeeds; document safe failure behavior for an exit-IP mismatch.

## 6. Documentation and Verification

- [x] 6.1 Update protocol and operator documentation with the catalog schema, query examples, ISO metadata guidance, advisory-availability limitation, rollout order, and rollback behavior.
- [x] 6.2 Run Rust formatting and the node/Linux-client test suites, fixing all regressions.
- [x] 6.3 Run the macOS Swift test suite and CLI build checks, fixing all regressions on a supported signed-development environment.
- [x] 6.4 Run the Codex skill validator for both skill entry paths and exercise dry-run natural-language discovery cases without paying or changing the active VPN.
- [x] 6.5 Run strict OpenSpec validation and confirm every scenario preserves pre-payment failure, exact-node binding, session lifecycle, and secret-ownership invariants.
