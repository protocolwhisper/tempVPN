# Persistent session coordinator

`tempvpn-session-coordinator` is a standalone control-plane service for fixed
paid sessions. It owns SQLite, payment idempotency, logical-node generations,
session balances, tunnel-address reservations, and desired WireGuard peers. It
does not proxy client HTTP calls or VPN packets.

The public listener serves only `GET /health` and `GET /nodes`. Nodes use the
separate `/coordination/v1` mTLS listener through the
`tempvpn-coordinator-client` library. Generation certificates are scoped to one
logical node and generation; promotion, drain, enrollment-token creation, and
administrative termination require an operator certificate.

## Runtime configuration

| Environment variable | Purpose |
| --- | --- |
| `COORDINATOR_BIND_ADDR` | Public read-only listener; defaults to `0.0.0.0:8080`. |
| `COORDINATOR_COORDINATION_BIND_ADDR` | Private mTLS listener; defaults to `0.0.0.0:8443`. |
| `COORDINATOR_DATABASE_PATH` | SQLite file; defaults to `/var/lib/tempvpn-coordinator/coordinator.sqlite`. |
| `COORDINATOR_TOKEN_KEY_FILE` | Required 32-byte key used to encrypt recoverable session tokens. |
| `COORDINATOR_TOKEN_KEY_VERSION` | Positive encryption-key version; defaults to `1`. |
| `COORDINATOR_SERVER_CERT_FILE` / `COORDINATOR_SERVER_KEY_FILE` | TLS server identity. |
| `COORDINATOR_CLIENT_ROOT_CA_FILE` | Offline-root certificate used to verify node and operator identities. |
| `COORDINATOR_INTERMEDIATE_CERT_FILE` / `COORDINATOR_INTERMEDIATE_KEY_FILE` | Online issuer used for enrollment and renewal. |

Keep every private key in an owner-readable file outside SQLite and source
control. SQLite uses WAL, `synchronous=FULL`, foreign keys, a five-second busy
timeout, and immediate write transactions. Its directory must be durable in a
real deployment; this branch intentionally does not add cloud disks or backups.

## Promotion and active-only drain

1. Register green as a healthy standby generation and confirm it reconciles an
   empty desired-peer snapshot.
2. Promote green with an operator certificate. Green becomes the sole target
   for new purchases and paused resumes; blue becomes draining.
3. Leave active blue sessions pinned to blue. Never copy or forcibly migrate
   their peers.
4. Wait until blue drain status reports zero active/transitional sessions,
   matching desired/applied revisions, zero actual peers, and
   `safe_to_delete = true`.
5. Only the deployment controller may then remove blue. This service reports
   safety; it does not delete VMs, change DNS, or operate a load balancer.

Connect and pause can temporarily return a retryable transition while the node
applies a WireGuard revision. Coordinator loss makes durable mutations fail
closed with a retryable error. Already managed peers survive only through their
last confirmed lease: nodes renew every 30 seconds and remove them locally
after 90 seconds without authority.

## Rollback limits

Before the first coordinator-backed purchase, a development node may return to
`fixed_session_mode = "memory"`. After durable entitlements exist, rolling back
must preserve the coordinator database and compatible API until every
nonterminal entitlement expires or is drained. Deleting or replacing the
SQLite file loses paid balances; automated recovery is deliberately out of
scope for this development change.
