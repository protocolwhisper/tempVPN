## Why

Users should be able to request a temporary VPN in ordinary language, such as
“Connect 30 minutes to Germany,” without the skill containing a phrase or region
mapping for every country. The current free-form `region` catalog can rank
healthy nodes, but it cannot reliably translate a country/city request into the
correct eligible nodes or distinguish a usable node from one that is leased but
not accepting another session.

## What Changes

- Add structured, ISO-based location metadata to node advertisements and the
  indexer catalog: country code, optional subdivision/region, and optional city.
- Add dynamic availability metadata so the indexer can exclude nodes that are
  draining or have no tunnel-address capacity before the user pays.
- Add country/city filtering to the indexer and equivalent `select` options to
  the Linux and macOS clients.
- Keep latency ranking on the user's client: the indexer determines eligibility,
  then the client health-checks and selects the fastest eligible node from the
  user's network perspective.
- Update the TempVPN skill to normalize natural-language locations into ISO
  country codes, parse durations and ranking words such as `fastest`, detect
  ambiguous locations, query the indexer, and stop before payment when no
  eligible match exists. The skill/client sends structured filters to the
  indexer; it does not forward the user's raw prompt.
- Recheck the selected node's health and availability immediately before
  payment, then bind payment and all lifecycle calls to that exact node.
- Preserve unfiltered selection for legacy nodes while excluding nodes without
  structured location metadata from country/city-specific requests.

## Capabilities

### New Capabilities

- `natural-language-discovery`: Interpret duration and location intent, resolve
  it through structured indexer metadata, and produce a safe node selection or a
  no-payment failure.

### Modified Capabilities

- `node-allocation`: Extend node advertisements, catalog filtering, availability
  reporting, and client selection from free-form regions to structured
  location-aware eligibility followed by client-side latency ranking.

## Impact

- **Node daemon and registry:** advertisement/catalog models, configuration,
  lease refresh data, health/capacity reporting, and `GET /nodes` filtering.
- **Linux client:** catalog model and `select --country/--city` behavior.
- **macOS client:** catalog model and equivalent `tempvpnctl select` options.
- **Agent skill:** generic location parsing, ambiguity handling, indexer-first
  discovery, pre-payment failure rules, and natural-language examples.
- **Configuration and infrastructure:** optional country/city fields for rollout;
  node deployments must set a country code to participate in country-specific
  discovery.
- **Documentation and tests:** protocol examples, operator guidance, cross-client
  parity, and indexer/client selection tests.
- **Unaffected semantics:** MPP price/challenge handling, payment credentials,
  client private-key ownership, session expiry, pause/delete rules, WireGuard
  routing, and admin/registry credential separation.
- **Compatibility:** catalog additions are additive. Existing nodes remain usable
  for unfiltered or explicit-URL selection but are not guessed into a country.
  Existing clients ignore new JSON fields.
- **Rollback:** clients and the skill can return to region-only selection while
  the indexer continues emitting additive metadata. Removing configured
  location fields restores legacy behavior without session or data migration.
