# tempVPN implementation

This directory contains the Codex skill, supported macOS launcher, Rust client,
and Rust VPN-node daemon used by tempVPN. Start with the repository
[`README.md`](../README.md) for installation and skill-loading instructions.

> [!IMPORTANT]
> The end-to-end client workflow currently supports **macOS only**. The daemon
> runs on Linux, but Linux and Windows are not supported client platforms yet.

## Architecture

```text
Mac                                      VPN node
---                                      --------
Codex reads SKILL.md
  -> macOS launcher
     -> wg creates local keypair
     -> mppx pays POST /sessions ------> MPP payment validation
         public key + duration           address allocation
     <- session JSON ------------------- temporary WireGuard peer
     -> AppleScript admin dialog
     -> vpn-client connect ============> WireGuard tunnel
     -> vpn-client status                automatic peer expiry
```

## Components

### `SKILL.md`

The reusable agent workflow. Its front matter tells Codex when the skill should
trigger, while its body defines the safe purchase, connection, verification,
and local-only disconnect sequence. It deliberately prohibits sending private
keys, using daemon admin credentials, or deleting paid sessions.

### `scripts/buy-and-connect-macos.sh`

The supported user entry point. It:

1. validates and converts a duration such as `30m`;
2. requires macOS and checks `wg`, `mppx`, and `osascript`;
3. confirms that the MPPX `main` account is visible in macOS Keychain;
4. creates an ephemeral WireGuard keypair in a restricted temporary directory;
5. pays the MPP-protected session endpoint;
6. calls the administrator helper to connect; and
7. prints `vpn-client status`.

Temporary key and session files are removed when the launcher exits.

### `scripts/connect-with-admin.applescript`

Runs only the privileged `vpn-client connect` command through the native macOS
administrator dialog. Payment remains unprivileged; only network-interface and
routing changes are elevated.

### `crates/vpn-client-cli`

The local Rust executable, built as `vpn-client`.

| Command | Purpose |
| --- | --- |
| `connect` | Writes a private WireGuard config, brings up the interface, checks it, verifies the visible IP when possible, and records local status. |
| `status` | Reads local status and checks whether the WireGuard interface is still active. |
| `disconnect` | Brings down the recorded local interface, deletes its generated config, and removes local status. |
| `config` | Generates a WireGuard configuration without bringing up the tunnel. This is a development/manual path. |
| `run` | Starts WireGuard plus a loopback-only SOCKS5 proxy, runs one child command with proxy variables, and cleans up afterward. This is not the default macOS skill flow. |

The launcher passes `--session-response` and `--private-key-path`, keeping MPP
payment and local tunnel control as explicit steps.

### `crates/vpn-node-daemon`

The Linux server component. It exposes:

| Endpoint | Role |
| --- | --- |
| `GET /health` | Reports service health and the number of active sessions. |
| `POST /sessions` | MPP-protected client endpoint that creates a temporary WireGuard peer. |
| `GET /sessions/:id` | Administrative session lookup; not used by the skill. |
| `DELETE /sessions/:id` | Administrative removal; prohibited in the normal paid client flow. |

The daemon allocates tunnel IP addresses, invokes `wg` to manage peers, and
removes expired peers during periodic cleanup. Its admin token belongs only on
the server/operator side.

### `configs`

- `vpn-client.example.toml`: optional client command, interface, proxy, status,
  node URL, and expected-exit-IP overrides.
- `vpn-node.example.toml`: daemon bind address, WireGuard interface, MPP charge,
  duration, cleanup, and server identity settings.
- `wg-server.example.conf`: starting point for the server WireGuard interface.

These files are deployment templates. The supported macOS demo uses compiled
defaults and the paid session response, so a client config file is not required.

## macOS prerequisites

- macOS administrator access, for WireGuard interface and route changes.
- `wg` from `wireguard-tools`, for keys and interface checks.
- Rust/Cargo, to build `vpn-client`.
- Node.js/npm and `mppx`, for Tempo MPP payment.
- A funded MPPX account named `main`, available in macOS Keychain.
- Network access to the session API and returned WireGuard endpoint.

See the root [`README.md`](../README.md#prerequisites) for installation commands
and the reason each dependency is required.

## Build and use the supported flow

```bash
cargo build -p vpn-client-cli
./scripts/buy-and-connect-macos.sh 30m
```

The launcher expects `target/debug/vpn-client`. During connection, macOS opens
an administrator dialog. Do not launch it first in an agent sandbox: sandboxed
MPPX account discovery can be unable to see the real Keychain account.

Check status or disconnect from this directory:

```bash
./target/debug/vpn-client status
sudo ./target/debug/vpn-client disconnect
```

The paid server peer remains until its expiry time; disconnect is local cleanup
only.

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
- The session request contains only the client public key and duration.
- The local SOCKS5 proxy used by `run` binds to loopback only.
- `run` stops its child process if the tunnel or proxy fails.
- `connect` persists local state so `status` and `disconnect` can find the
  correct interface and generated config.
- Server cleanup removes the temporary peer at expiry even if the client does
  not disconnect cleanly.

## Not yet supported

- End-to-end Linux or Windows client use.
- Multiple VPN regions and node selection.
- Persistent daemon sessions across crashes.
- Direct public production exposure without a TLS reverse proxy.
