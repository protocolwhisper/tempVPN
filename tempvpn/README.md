# tempVPN implementation

This directory contains the Codex skill, native macOS app/CLI, Linux Rust client,
and Rust VPN-node/registry daemon used by tempVPN. Start with the repository
[`README.md`](../README.md) for installation and skill-loading instructions.

> [!IMPORTANT]
> Linux uses `vpn-client`; macOS uses the signed `TempVPN.app`, Packet Tunnel
> Provider, and `tempvpnctl`. Windows is not supported.

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

These files are deployment templates. The supported macOS demo uses compiled
defaults and the paid session response, so a client config file is not required.

## Client prerequisites

- Linux: `wg`, `wg-quick`, and the Rust `vpn-client`.
- macOS: signed `TempVPN.app`, its Packet Tunnel Provider, and signed `tempvpnctl`.
- Node.js/npm and `mppx`, for Tempo MPP payment.
- A funded MPPX account named `main`, available in macOS Keychain.
- Network access to the session API and returned WireGuard endpoint.

See the root [`README.md`](../README.md#prerequisites) for installation commands
and the reason each dependency is required.

## Build

```bash
cargo build -p vpn-client-cli
CLANG_MODULE_CACHE_PATH="$PWD/target/swift-cli-module-cache" \
  swiftc -parse-as-library clients/macos/CLI/*.swift -o target/tempvpnctl
xcodebuild -project clients/macos/TempVPN.xcodeproj -scheme TempVPN build
```

Select before paying. The selected node URL must be used for both MPP payment
and session import:

```bash
./target/debug/vpn-client select --region eu-west --json
./target/tempvpnctl select --region eu-west --json
```

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
HTTP API with TLS. The current in-memory session store is not crash-persistent.

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
- Direct public production exposure without a TLS reverse proxy.
