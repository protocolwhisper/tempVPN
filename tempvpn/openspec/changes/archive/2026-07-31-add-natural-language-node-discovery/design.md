## Context

See `proposal.md` for motivation. Today a node lease contains `id`, `name`, a
free-form `region`, connection endpoints, and expected exit IP. The registry
returns all unexpired leases, and each client filters by region and probes node
health before payment. The registry and sessions are in-memory, and the `/24`
allocator owns addresses `.2` through `.254`.

The agent skill accepts natural language, but the Linux Rust CLI and macOS Swift
CLI are deterministic executors. This separation must remain clear: natural
language is interpreted outside the public indexer, while the indexer accepts
validated query parameters and the clients perform network-relative ranking.

The security and lifecycle invariants in the project context remain unchanged.
In particular, discovery is public and read-only; the client private key,
payment credentials, session bearer data, registry token, and node-admin token
do not enter the discovery request.

## Goals / Non-Goals

**Goals:**

- Give the skill a small normalized intent contract covering action, duration,
  location, and ranking policy.
- Make country and city selection generic through ISO country codes instead of
  hard-coded examples or free-form region guesses.
- Keep eligibility authoritative at the indexer and latency measurement
  authoritative at the user's device.
- Keep Linux and macOS selection semantics equivalent while using their native
  HTTP, concurrency, and command-line implementations.
- Fail safely before payment when parsing, matching, probing, or the final
  availability check fails.

**Non-Goals:**

- Running an LLM, accepting raw prompts, or parsing natural language in the
  indexer, Linux CLI, or macOS CLI.
- Producing or maintaining a phrase for every country in the skill.
- Geolocating nodes from IP addresses or inferring country from `region`.
- Reserving node capacity transactionally during catalog lookup. A paid quote
  or reservation protocol would be a separate change.
- Changing MPP payment, WireGuard key ownership, session expiry, pause/delete
  behavior, or Network Extension packaging.

## Decisions

### 1. Normalize intent before calling infrastructure

The skill resolves input into an internal object shaped like:

```text
action: connect | disconnect | status
duration_seconds: integer | absent
country_code: ISO alpha-2 | absent
city: string | absent
region: string | absent
selection_policy: lowest_latency
```

For example, `Connect 30 mins to the fastest node in Germany` becomes
`connect`, `1800`, `DE`, and `lowest_latency`. The indexer receives only
structured query parameters. `fastest` without a location means global
eligible discovery. An ambiguous country name or missing required duration is
resolved with the user before any paid operation.

Rationale: this keeps the public API deterministic, testable, language-agnostic,
and free of raw user prompts. It also allows any capable agent to resolve common
country names without expanding the skill into a country phrasebook.

Alternative considered: send the raw prompt to the indexer and parse it there.
This was rejected because it couples registry availability to an NLP runtime,
creates privacy and ambiguity concerns, and makes payment-safety behavior harder
to test.

### 2. Add structured, optional location fields without replacing region

`NodeAdvertisement` gains optional `country_code`, `subdivision_code`, and
`city` fields. `country_code` is canonical uppercase ISO 3166-1 alpha-2.
`region` remains required during this change because it represents existing
operator/deployment grouping and is not necessarily a geopolitical country.

The registry validates country values against a small versioned ISO code set in
the node implementation. It trims optional text fields and rejects supplied
empty or invalid values. It never derives structured fields from `region` or an
IP address.

Rationale: additive optional JSON fields allow old nodes and clients to coexist.
A country-filtered query excludes records without `country_code`; an unfiltered
query preserves legacy behavior.

Alternative considered: interpret `region` values such as `eu-west` as country
aliases. This was rejected because the mapping is lossy and can route a paid
session through the wrong jurisdiction.

### 3. Advertise availability as a lease snapshot

Advertisements and `/health` gain `accepting_sessions` and `available_slots`.
`accepting_sessions` is false when the operator has placed a node in draining
mode. `available_slots` is derived under the `Sessions` store lock from the `/24`
allocator's usable address count minus current session-scoped reservations.
The registration loop rebuilds the advertisement on every refresh rather than
capturing it once at startup.

The registry's wire model accepts both availability fields as absent for legacy
leases, but `available=true` matches only explicit `true` plus a positive slot
count. Newly updated nodes always send both fields. This preserves unfiltered
compatibility without interpreting unknown capacity as available capacity.

`Sessions` owns reservation state and its cleanup. The registry owns only the
latest authenticated snapshot and lease expiry; it does not reserve capacity.
The node configuration owns the operator's draining flag. No availability data
is persisted by this change because both the registry and session catalog are
already intentionally in-memory.

Rationale: eligibility becomes much better than lease liveness while preserving
the independent-node architecture. The selected node still performs a final
live `/health` check before payment.

Alternative considered: reserve a slot at the indexer. This was rejected for
this change because the indexer does not own node session state, and a correct
reservation would require expiry, authentication, payment binding, and replay
semantics.

### 4. Keep the indexer API structured and conjunctive

The catalog endpoint accepts optional `country`, `city`, `region`, and
`available` query parameters:

```text
GET /nodes?country=DE&city=Frankfurt&available=true
```

Country is normalized to uppercase and validated. Text matching is trimmed and
case-insensitive. Every supplied filter is combined with AND semantics. Invalid
filters return `400`; they never fall back to the unfiltered catalog. Results
remain ordered by node ID. The indexer does not rank by its own ping because its
network path is not the user's path.

Rationale: explicit filters are predictable and reusable by the Rust CLI, Swift
CLI, scripts, and other agents.

Alternative considered: add a `/discover` endpoint accepting a bespoke intent
JSON document. This was rejected as unnecessary while the only server-side
operation is catalog filtering.

### 5. Use indexer eligibility followed by client-side latency ranking

The skill passes normalized location options to the local client. The client
queries the indexer with `available=true`, health-checks matching candidates
from the user's device, measures three samples with the existing two-second
timeout, and selects the lowest median RTT. Probe concurrency is bounded to
avoid creating unbounded local work as the catalog grows.

Immediately before invoking `mppx`, the client requests the selected node's
live health/availability response again. If the node is no longer eligible, the
operation stops without paying and may rediscover from a fresh catalog.

Cached catalog fallback remains allowed within its existing TTL, including for
location filters, but a cached candidate must pass the same final live health
and availability check. Missing structured metadata never matches a structured
filter.

Rationale: the indexer efficiently narrows jurisdiction and basic capacity,
while only the user's device can identify the fastest route for that user.

Alternative considered: have the indexer continuously ping and rank nodes. This
was rejected as a final ranking mechanism because it measures the indexer's
latency rather than the user's latency. Such measurements could later be used
only as a coarse shortlist hint.

### 6. Preserve exact-node and post-connect verification boundaries

Selection returns the normalized node API URL together with node ID, advertised
location, and expected exit IP. Existing selected-node URL checks remain the
authority for binding payment and lifecycle calls. After connection, the skill
uses client status and exit-IP verification before reporting success and shows
the selected advertised location; it does not make a stronger geolocation claim
than the node metadata and exit-IP check support.

Rationale: a correct location match is insufficient if a later request silently
targets another node or the host never actually routes through the tunnel.

### 7. Keep platform differences below a shared behavior contract

The Linux client adds Rust model fields and structured selection flags, performs
HTTP probes with `reqwest`, and continues to activate through `wg-quick`. The
macOS `tempvpnctl` adds equivalent Codable fields and flags, performs probes with
`URLSession`, and continues to hand the exact selected profile to the signed
host app and Network Extension. Neither platform moves private-key generation
or storage into discovery.

Rationale: sharing wire-level field names, CLI semantics, fixtures, and expected
results gives parity without forcing Swift networking and Apple tunnel control
into Rust or duplicating the natural-language layer in both clients.

## Risks / Trade-offs

- **[Availability can change between the final check and paid creation]** → Keep
  the interval minimal, reject activation safely when capacity is gone, and
  document that complete elimination requires a future expiring reservation
  bound to payment.
- **[Self-advertised location can be false or stale]** → Treat it as operator
  metadata, verify the advertised exit IP after connection, and avoid claiming
  independent geolocation attestation.
- **[Agent country interpretation can be ambiguous]** → Require an unambiguous
  ISO code, validate it at the indexer, and ask the user instead of guessing.
- **[A global query can create many client probes]** → Bound concurrent probes
  and preserve per-probe timeouts; add server-side pagination or shortlist hints
  later if catalog scale requires them.
- **[Legacy nodes disappear from location-specific results during rollout]** →
  Keep them in unfiltered discovery and deploy location-aware advertisements
  before promoting location-specific skill examples.
- **[Cached availability becomes stale]** → Require a live selected-node health
  and availability check before payment even when discovery used the cache.

## Migration Plan

1. Deploy the registry/indexer with additive deserialization, location
   validation, filters, and availability fields while retaining unfiltered
   legacy behavior.
2. Deploy node configuration and dynamic advertisement refresh. Populate
   country metadata and verify health/catalog values per node.
3. Release Linux and macOS clients with matching structured flags, models,
   ranking, cache, and final-check behavior.
4. Update the skill to emit normalized structured options and enable the new
   natural-language examples only after compatible nodes and clients exist.
5. Monitor no-match, no-healthy-node, and final-check failure rates without
   logging raw prompts or credentials.

Rollback disables the skill's structured route and returns clients to region or
explicit-URL selection. Additive catalog fields may remain; removing optional
location values makes nodes legacy-visible again. Existing sessions require no
migration and continue using their already selected node.
