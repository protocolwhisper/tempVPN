# tempVPN implementation

This directory contains the Codex skill, native headless macOS client, Linux
Rust client, and Rust VPN-node/registry daemon used by tempVPN. Start with the repository
[`README.md`](../README.md) for installation and skill-loading instructions.

> [!IMPORTANT]
> Linux uses the Rust `vpn-client` with `wg`/`wg-quick`. macOS uses
> `tempvpnctl`, an invisible host app, a Packet Tunnel extension, and
> WireGuardKit. Windows is not supported.

## Architecture

```text
Node daemons --authenticated leases--> Registry-mode daemon
Linux/macOS clients <-- GET /nodes -----------|
       |
       +-- MPP payment and VPN session go directly to selected node

Fixed-session node generations --mTLS--> Persistent session coordinator
                                      `-- SQLite durable authority
```

## Components

### `SKILL.md`

The reusable agent workflow. Its front matter tells Codex when the skill should
trigger, while its body defines the safe purchase, connection, verification,
and disconnect-pause sequence. It deliberately prohibits sending private
keys, using daemon admin credentials, or deleting paid sessions.

### `clients/linux`

The local Rust executable, built as `vpn-client`.

| Command | Purpose |
| --- | --- |
| `select` | Sends structured country/city/region filters to the registry, probes eligible nodes with bounded concurrency, and prints the fastest healthy node. |
| `check` | Rechecks one selected node's live health and capacity immediately before payment. |
| `connect` | Writes a private WireGuard config, brings up the interface, checks it, verifies the visible IP when possible, and records local status. |
| `status` | Reads local status and checks whether the WireGuard interface is still active. |
| `disconnect` | Brings down the recorded local interface, pauses the server-side usage balance, deletes its generated config, and removes local status. |
| `heartbeat` | Refreshes the server-side connected-time accounting and prints remaining balance. |
| `config` | Generates a WireGuard configuration without bringing up the tunnel. This is a development/manual path. |
| `run` | Starts WireGuard plus a loopback-only SOCKS5 proxy, runs one child command with proxy variables, and cleans up afterward. This is not the default macOS skill flow. |

The agent passes `--session-response`, `--private-key-path`, and the exact
selected `--node-url`, keeping MPP payment and local tunnel control explicit.

### `node/linux`

The Linux server component. It exposes:

| Endpoint | Role |
| --- | --- |
| `GET /health` | Reports service health, active sessions, drain state, and available tunnel slots. |
| `GET /nodes` | Public catalog with optional `country`, `city`, `region`, and `available` filters. |
| `PUT /registry/nodes/:id` | Authenticated node lease registration or refresh. |
| `DELETE /registry/nodes/:id` | Authenticated graceful lease removal. |
| `POST /sessions` | MPP-protected endpoint that creates a paid connected-time balance. |
| `POST /sessions/:id/connect` | Activates a paid balance, adds or refreshes the WireGuard peer, and starts consuming time. |
| `POST /sessions/:id/pause` | Pauses a paid balance, removes the WireGuard peer, and stops consuming time. |
| `POST /sessions/:id/heartbeat` | Updates connected-time accounting for active sessions. |
| `GET /sessions/:id/status` | Public paid-session status lookup for remaining seconds and grace deadline. |
| `GET /sessions/stream` | Tempo TIP-1034 Session v2 authenticated SSE control stream for metered access. |
| `HEAD /sessions/stream` | Session v2 open, voucher, top-up, and close management operations. |
| `GET /sessions/:id` | Administrative session lookup; not used by the skill. |
| `DELETE /sessions/:id` | Administrative removal; prohibited in the normal paid client flow. |

Registry writes use a dedicated token separate from the admin token and MPP.
Nodes refresh 90-second leases every 30 seconds and retry registry outages with
capped exponential backoff. The daemon allocates tunnel IP addresses, invokes `wg` to manage peers, tracks
connected-time balance, and removes expired peers during periodic cleanup. Its
admin token belongs only on the server/operator side.

In `fixed_session_mode = "coordinator"`, the daemon remains the public client
API but delegates fixed purchases and lifecycle mutations to the standalone
coordinator. It reconciles only its coordinator-managed WireGuard peers. See
[`registry/coordinator/README.md`](registry/coordinator/README.md) for service
configuration, mTLS roles, promotion, drain safety, and rollback limits.

### `configs`

- `vpn-client.example.toml`: optional client command, interface, proxy, status,
  node URL, and expected-exit-IP overrides.
- `vpn-node.example.toml`: daemon bind address, WireGuard interface, MPP charge,
  duration, cleanup, and server identity settings.
- `wg-server.example.conf`: starting point for the server WireGuard interface.

These files are deployment templates. The macOS CLI uses the registry
environment variable and paid session response, so a client config file is not
required.

## Client prerequisites

- Linux: `wg`, `wg-quick`, and the Rust `vpn-client`.
- macOS: signed `tempvpnctl`, invisible `TempVPN.app`, Packet Tunnel Provider,
  and WireGuardKit. The host has no graphical interface.
- Node.js/npm and `mppx`, for Tempo MPP payment.
- A funded MPPX account named `main`, available in macOS Keychain.
- Network access to the session API and returned WireGuard endpoint.

See the root [`README.md`](../README.md#prerequisites) for installation commands
and the reason each dependency is required.

## Build

```bash
cargo build -p vpn-client-cli
./clients/macos/build-macos-products.sh
```

Unsigned macOS outputs validate compilation only. After Apple signing, install
the native pair:

```bash
sudo ./clients/macos/install-tempvpnctl.sh
```

End users do not build this checkout. When the TempVPN skill finds the native
macOS client missing, it downloads the latest notarized package from the
project's GitHub Releases, verifies the SHA-256 checksum, pinned Apple team,
Developer ID signatures, bundle identifiers, entitlements, and stapled
notarization ticket, then offers the verified package through macOS Installer.
See [`clients/macos/README.md`](clients/macos/README.md) for the maintainer
release command.

Select before paying. The selected node URL must be used for both MPP payment
and session import:

```bash
./target/debug/vpn-client select --country DE --selection-policy lowest-latency --json
./target/debug/vpn-client check --node-url "$SELECTED_NODE_URL" --json

./target/tempvpnctl select --country DE --selection-policy lowest-latency --json
./target/tempvpnctl check --node-url "$SELECTED_NODE_URL" --json
```

The headless host exists only because macOS requires an app container for the
Network Extension. Agents and users control it through `tempvpnctl` or macOS
System Settings; there is no graphical TempVPN interface.

Disconnect pauses the paid server session so unused connected time remains
available until the configured grace deadline.

## Server development

The daemon is a separate operator concern and normally runs on the Linux VPN
node:

```bash
cp configs/vpn-node.example.toml vpn-node.toml
VPN_NODE_ADMIN_TOKEN="replace-with-a-server-only-secret" \
  cargo run -p vpn-node-daemon -- --config vpn-node.toml
```

Before deployment, configure WireGuard forwarding/NAT, replace every example
placeholder, keep the admin token out of client environments, and terminate the
HTTP API with TLS. The fixed-price `POST /sessions` flow remains available while
streaming is introduced.

### Durable fixed sessions and blue/green drain

Set `fixed_session_mode = "coordinator"` and the coordinator URL, logical node,
generation ID, root CA, certificate, and private-key paths shown in
`configs/vpn-node.example.toml`. The URL must address the private mTLS listener,
not the public registry endpoint. The coordinator is a separate process and is
the only SQLite owner; node daemons do not mount or open its database.

A paid entitlement belongs to the stable logical-node URL. An active tunnel
stays pinned to its current generation. After it pauses, it can resume through
the accepting generation with the same remaining balance and tunnel IP but a
fresh server WireGuard key and endpoint. Clients build the tunnel from each
connect response. Retryable `409` transitions mean peer reconciliation is still
in progress; retryable `503` responses mean the coordinator could not confirm a
durable mutation. Never fall back to process memory after either response.

During promotion, share the active MPP challenge key between old and new
generations for at least the five-minute challenge lifetime plus clock-skew
allowance. Blue stops purchases and paused claims but continues serving its
active sessions. Delete blue only after operator drain status says it is safe.
GCP, Terraform, load-balancer, DNS, and VM-deletion automation belong to the
dependent `deploymaster` work, not this branch.

### Tempo Session v2 streaming

The streaming server is all Rust. It attaches a local TIP-1034 v2 adapter to
Rust `mpp`'s `SessionMethod`; `clap` only parses daemon command-line arguments.
No TypeScript or `mppx` runtime is embedded in the node. A client may use an
MPP-compatible client implementation, including `mppx`, but the verifier,
accounting store, SSE meter, and WireGuard lifecycle all run in
`vpn-node-daemon`.

For a local Moderato test, add the streaming fields from
`configs/vpn-node.example.toml`, keep `mpp_streaming_mode = "development"` and
`mpp_session_store = "memory"`, provide `MPP_SECRET_KEY`, and provide a testnet
close key in the process environment:

```bash
MPP_SECRET_KEY="replace-with-an-hmac-secret" \
VPN_NODE_MPP_SESSION_CLOSE_PRIVATE_KEY="0x..." \
  cargo run -p vpn-node-daemon -- --config vpn-node.toml
```

An unpaid `GET` or `HEAD` request receives a 402 challenge containing method
`tempo`, intent `session`, and `sessionProtocol: "v2"`. The request's WireGuard
public key and safety duration are HMAC-bound into the challenge. After a valid
funded credential, `GET` returns connection details over SSE. Each completed
billing interval consumes one atomic unit; exhaustion disables the peer and
emits `payment-need-voucher`, and a newer verified voucher resumes the same
logical session without billing the paused period.

Production mode fails at startup unless `mpp_session_store = "sqlite"` and
`mpp_session_sqlite_path` is configured. Put the database on durable storage
whose filesystem provides working SQLite locks to every process serving the
same MPP realm. SQLite immediate transactions protect cumulative voucher
monotonicity, spend, replay state, and the single active stream lease. Startup
expires stale leases and removes their recorded WireGuard peers before payment
traffic is accepted. Keep the close key in a secret manager or environment
injection, never in source control or logs.

To roll back streaming, set `mpp_streaming_enabled = false` and restart. The
streaming route will not be registered and `POST /sessions` continues to work.
Retain the SQLite database even after rollback so accepted voucher and replay
state remain available for settlement or a later restart.

## Structured node discovery protocol

Updated nodes advertise additive catalog fields:

```json
{
  "id": "de-frankfurt-1",
  "name": "Frankfurt 1",
  "region": "eu-central",
  "country_code": "DE",
  "subdivision_code": "DE-HE",
  "city": "Frankfurt",
  "accepting_sessions": true,
  "available_slots": 42,
  "api_url": "https://de-frankfurt-1.example",
  "wireguard_endpoint": "192.0.2.10:51820",
  "expected_exit_ip": "192.0.2.10",
  "lease_expires_at": "2026-07-31T18:00:00Z"
}
```

`country_code` is an ISO 3166-1 alpha-2 value. Operators may omit country,
subdivision, and city during migration, but nodes without structured location
never match country/city requests. `region` remains an independent deployment
label and is not interpreted as a country.

Clients use conjunctive, URL-encoded filters such as:

```text
GET /nodes?country=DE&city=Frankfurt&available=true
```

The indexer decides catalog eligibility; the user's client measures three
health samples and ranks median latency. `accepting_sessions` and
`available_slots` are advisory lease snapshots, not reservations, so every paid
workflow runs the selected node's live `check` immediately before `mppx`.

Roll out indexer support first, then node metadata/availability, then clients,
and finally natural-language routing in the skill. To roll back, return clients
to explicit URL or legacy region selection. Additive fields may remain, and
existing node-bound sessions continue without migration.

## Safety and lifecycle

- The client private key is generated and retained locally.
- The paid session request contains only duration; the connect request contains
  the client public key.
- The local SOCKS5 proxy used by `run` binds to loopback only.
- `run` stops its child process if the tunnel or proxy fails.
- `connect` persists local state so `status` and `disconnect` can find the
  correct interface and generated config.
- Server cleanup removes expired peers and pauses stale active sessions if
  heartbeats stop.

## Not yet supported

- Windows client use.
- Automated coordinator backups or multi-writer coordinator availability.
- Automatic restoration of a client's already-running local tunnel after its
  owning node generation disappears; active tunnels are deliberately not migrated.
- Direct public production exposure without a TLS reverse proxy.
