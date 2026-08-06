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
| `select` | Fetches the registry catalog, filters by region, probes nodes in parallel, and prints the fastest healthy node. |
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
| `GET /health` | Reports service health and the number of active sessions. |
| `GET /nodes` | Public node catalog used by clients to discover and latency-rank nodes. |
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

Select before paying. The selected node URL must be used for both MPP payment
and session import:

```bash
./target/debug/vpn-client select --region eu-west --json
./target/tempvpnctl select --region eu-west --json
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
- Persistent daemon sessions across crashes.
- Restoring an interrupted client's local tunnel automatically after a daemon crash.
- Direct public production exposure without a TLS reverse proxy.
