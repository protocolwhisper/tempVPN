## Why

Fixed paid sessions currently live only in each node daemon's memory, so a restart or blue/green replacement loses paused balances and forces active tunnels to be destroyed. A durable logical-node authority is required before node generations can be drained without interrupting active customers or losing paid entitlements.

## What Changes

- Add a dedicated coordinator service whose SQLite database is the durable authority for fixed paid sessions, payment redemption, logical-node generations, tunnel-address reservations, and peer reconciliation.
- Expose compatible coordinator `/nodes` and `/health` reads for discovery integration while keeping payment and session-lifecycle routes on the selected node API.
- Bind active sessions to one node generation, leave paused sessions unbound, and allow a paused session to resume on the current accepting generation with its unused balance and assigned tunnel IP.
- Add promotion and active-only draining: the old generation stops receiving purchases and paused resumes but keeps active peers until every active lease has ended and peer removal is confirmed.
- Make one-time payment redemption idempotent by persisting payment intents and unique transaction references; retries after a lost response return the same entitlement.
- Protect node-to-coordinator traffic with generation-scoped mTLS enrollment and keep raw session tokens out of database lookup indexes.
- Define the coordinator's portable deployment contract while deferring all GCP and Terraform implementation to the `deploymaster` branch; automated backups and multi-coordinator high availability remain intentionally deferred during product development.
- Preserve Linux, macOS, and agent workflows. Clients continue sending only their WireGuard public key and may receive a different server key and endpoint when a paused session resumes on a newer generation.
- Do not move or enable Streaming Session v2. MIG, load-balancer, DNS-promotion, and deployment-controller automation will consume these contracts in a dependent change.

## Capabilities

### New Capabilities

- `persistent-session-coordination`: Durable fixed-session, payment-idempotency, generation-registration, lease, security, and failure contracts owned by the registry/coordinator.
- `active-only-node-draining`: Promotion, admission control, active-generation pinning, peer reconciliation, and safe old-generation retirement behavior.

### Modified Capabilities

- `session-lifecycle`: Replace process-local session ownership with durable logical-node ownership and active-generation leases while preserving public fixed-session routes.
- `node-allocation`: Preserve one unique tunnel address for a paid entitlement across pauses and generation reassignment.
- `client-connection`: Keep the logical node API URL stable while accepting refreshed WireGuard server keys and endpoints after a paused resume.

## Impact

- Registry: gains a separate stateful coordinator service with compatible `/nodes` and `/health` reads; the existing global discovery aggregator remains an independent change.
- Node daemon: replaces the in-memory fixed-session authority with coordinator calls and reconciles generation-scoped WireGuard peer revisions.
- Linux and macOS clients: protocol-compatible behavior is retained and verified; no client private key leaves the device.
- Agent skill and documentation: explain durable paused resumes and active-only draining without changing natural-language discovery inputs.
- Configuration: add portable coordinator database-path, mTLS identity, secret-file, generation-identity, and service-health settings. GCP resources, Persistent Disk attachment, startup scripts, Terraform lifecycle rules, rollout rehearsal, and Terraform validation are owned exclusively by the dependent `deploymaster` work.
- Payment behavior changes only to add durable intent binding and replay-safe retry handling. Session expiry keeps the configured grace deadline and 90-second stale timeout. Network routing for active tunnels never migrates between generations.
- Rollback to process-local sessions is safe only before coordinator-backed purchases begin or after all durable entitlements reach a terminal state; otherwise rollback must retain coordinator API and schema compatibility.
