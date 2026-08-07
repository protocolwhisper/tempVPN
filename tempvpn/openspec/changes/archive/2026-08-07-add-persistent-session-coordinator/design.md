## Context

Fixed sessions are currently held in `node/linux` memory and WireGuard changes are applied directly by the same process. The repository also contains a stateless global discovery aggregator and a durable SQLite store used only by disabled Streaming Session v2. This change needs a new single-writer authority without coupling VPN packet forwarding to database latency or merging the streaming accounting model.

The public API address identifies a logical node. WireGuard server keys and endpoints identify a concrete generation and may change after a paused resume. Active packet traffic never passes through the coordinator.

## Goals / Non-Goals

**Goals:**

- Preserve fixed paid entitlements, balances, payment idempotency, and tunnel addresses across coordinator and node restarts.
- Permit active-only blue/green draining without forcibly migrating active peers.
- Make database/WireGuard side effects recoverable through explicit transitional states and peer reconciliation.
- Keep existing Linux and macOS fixed-session routes and response shapes compatible.
- Keep the storage and service interfaces portable outside GCP.

**Non-Goals:**

- Streaming Session v2 persistence or activation.
- Multi-writer coordinator high availability, Redis caching, or automated database backups.
- MIG creation, public load-balancer routing, DNS promotion, or automatic VM deletion.
- Migrating existing process-local sessions into SQLite.

## Decisions

### Separate coordinator service with one SQLite writer

Add a Rust/Axum service under `registry/coordinator`. It owns SQLite and exposes public read-only `/health` and `/nodes` routes plus versioned private coordination routes. Node APIs remain the client-facing payment and lifecycle boundary and call the coordinator for durable mutations.

The coordinator uses one process-level connection owner and short blocking transactions. SQLite is configured with WAL, `synchronous=FULL`, foreign keys, a five-second busy timeout, and `BEGIN IMMEDIATE`. Schema migrations use `PRAGMA user_version`; startup refuses unknown newer versions. A database health failure makes mutations unavailable rather than falling back to node memory.

Redis was rejected as the authority because persistence and uniqueness would still require durable transactional storage. PostgreSQL is the planned migration target if measured concurrent-write demand exceeds the single coordinator.

### Durable schema and invariants

The initial schema contains:

- `sessions`: internal UUID, SHA-256 token lookup hash, encrypted token bytes, encryption-key version and nonce, logical node, public state, transitional phase, total/remaining seconds, grace and accounting timestamps, last client heartbeat, assigned IP, last client public key, active generation, prior-peer release deadline, and optimistic revision.
- `payment_intents`: random intent ID, logical node, duration, request fingerprint, challenge-key version, expiry, state, and resulting session.
- `payment_redemptions`: payment method and transaction reference as a unique key, request fingerprint, intent, and session.
- `node_generations`: logical node and generation key, stable API URL, WireGuard endpoint/public key, tunnel network, admission state, health deadline, desired/applied peer revisions, and reported peer count.
- `desired_peers`: generation, session, client public key, assigned IP, lease deadline, and desired revision.
- `enrollment_tokens`: token hash, logical-node/generation scope, expiry, and consumed timestamp.

A partial unique index permits only one accepting generation per logical node. Assigned IP is unique per logical node while a reservation exists. Expired session and payment rows are retained; terminal cleanup clears the address and desired peer.

### Token lookup and secret ownership

The public `sess_` token remains the client credential. Nodes hash it before coordinator lookup. The coordinator stores a ChaCha20-Poly1305-encrypted recoverable copy only for lost-response payment retry. Versioned encryption and MPP challenge keys are supplied as owner-readable files and never stored in SQLite or Terraform state.

The coordinator owns fixed-purchase intent creation and redemption. A challenge binds intent ID, logical node, duration, method, and request digest. The current and previous challenge-key versions overlap for the five-minute challenge lifetime plus clock-skew allowance. A duplicate transaction for the same fingerprint returns the committed session; a different fingerprint is rejected.

### Explicit activation and release phases

SQLite and `wg` cannot share one transaction. Connect therefore transitions `paused -> activating`, reserves or reuses an IP, sets the accepting generation, and increments that generation's desired revision. The generation applies the desired peer and acknowledges the revision; only then does the coordinator expose `active`. A failure or activation timeout returns the session to paused and releases an address created by that failed attempt.

Pause, stale timeout, exhaustion, and expiry first account elapsed usage and transition the peer to `releasing`. The desired peer is removed and the old generation revision advances. The session becomes unowned/reassignable only after removal acknowledgement or the last 90-second lease deadline. This prevents duplicate use of one address across generations.

Reconciliation returns a complete desired peer snapshot plus revision. A generation makes its managed peers exactly match that snapshot and acknowledges the revision and actual count. Unknown unmanaged WireGuard peers are outside this service's ownership and are not removed.

### Generation admission and stable client routing contract

Generation states are `standby`, `accepting`, `draining`, and `retired`. Promotion is an immediate transaction that makes green accepting and blue draining. New purchases and paused claims require accepting state; lifecycle reads and active heartbeats remain valid for draining-owned sessions.

The session `node_url` is the stable logical-node API URL. A future load balancer routes that API address to serving generation gateways. The `endpoint` and `server_public_key` returned by connect always come from the active generation. Existing clients already build the tunnel from each activation response; Linux and macOS regression tests lock that behavior.

Drain status is safe only when active and transitional counts are zero, desired and applied revisions match, and actual managed peer count is zero. The later deployment controller is responsible for acting on that status.

### Leases and outage behavior

Client heartbeats renew active state every 30 seconds; a lease is stale after 90 seconds. Nodes retain existing peers during a shorter coordinator outage but reject new mutations. If renewal cannot be confirmed before the local lease deadline, the owning node removes the peer. On recovery the coordinator accounts through the stale cutoff and completes release before permitting resume.

### mTLS enrollment and authorization

An offline root signs a rotatable online intermediate. The enrollment listener uses server-authenticated TLS and accepts a ten-minute, single-use enrollment token plus a generation-generated public key. Normal private routes require a generation certificate with logical-node and generation identity in SAN extensions. Certificates default to 24 hours and renew before eight hours remain. Operator promotion and drain calls require a separate operator certificate role.

The dependent `deploymaster` Terraform work provisions service identities and references secret versions but does not generate or retain private values. Node and coordinator private keys are created or materialized locally with owner-only permissions. CA and MPP rotations accept overlapping versions until all in-flight credentials expire.

### Deploymaster-only GCP deployment contract

This `AppleNetworking` change implements no GCP or Terraform resources. A dependent change on `deploymaster` adds one coordinator VM, static internal/public addressing as required by the existing TLS pattern, and a dedicated data disk mounted at `/var/lib/tempvpn-coordinator`. The disk uses `auto_delete = false`; replacement attaches the existing disk and initialization refuses to format a filesystem containing data. Firewall rules expose public reads separately from the mTLS coordination listener.

The application consumes ordinary file paths and URLs rather than GCP-specific APIs. A future provider can supply an attached block volume and the same secret files. Backups, snapshots, and cross-provider replication remain explicitly disabled for development. The `deploymaster` handoff owns GCP resource definitions, safe disk mounting, replacement-safety validation, cutover/drain rehearsal, and final Terraform-inclusive validation.

## Risks / Trade-offs

- [The coordinator VM is a single availability point] → Existing peers keep their bounded lease; mutations fail closed, and the persistent disk survives ordinary VM replacement.
- [SQLite serializes writes] → Keep transactions short, use WAL, measure heartbeat load, and migrate to PostgreSQL before measured saturation.
- [Peer acknowledgement can delay pause or resume] → Stop usage accounting immediately, make lifecycle retries idempotent, and expose a retryable transitional response until release is safe.
- [No development backup protects against disk corruption or account loss] → Document the accepted risk and keep the schema portable for a later backup change.
- [Stable API routing is not automated here] → Test with explicit generation gateways and make production promotion depend on the later load-balancer/controller change.
- [Legacy in-memory entitlements cannot be exported reliably] → Disable purchases and confirm an empty development fleet before first cutover.

## Migration Plan

1. Build and test the coordinator, SQLite migrations, payment idempotency, and fake generation reconciliation locally.
2. Add node coordinator mode behind explicit configuration; retain process-local mode only for tests until cutover.
3. Hand the portable service contract to the `deploymaster` branch; no GCP or Terraform implementation is performed on `AppleNetworking`.
4. On `deploymaster`, provision the coordinator VM and persistent disk without changing fleet admission or discovery defaults.
5. On `deploymaster`, stop current purchases and explicitly confirm the development fleet has no active or paused fixed entitlements.
6. On `deploymaster`, enroll one generation, reconcile zero peers, switch its fixed-session routes to coordinator mode, and run purchase/connect/pause/restart tests.
7. On `deploymaster`, exercise a two-generation promotion and drain without deleting either VM.
8. Leave production VM deletion and traffic promotion disabled until the dependent deployment-controller change consumes safe-drain status.

Rollback before coordinator-backed purchases disables coordinator mode. After the first durable purchase, rollback must preserve the coordinator schema and compatible node API until all nonterminal entitlements expire or are drained.
