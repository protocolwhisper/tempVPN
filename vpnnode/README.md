# VPN Node Client MVP

This directory contains a Rust MVP for one managed egress node: `germany`.

It deliberately does not implement payment or multi-region routing. The target path is:

```text
Client/Codex -> 127.0.0.1:1080 SOCKS5 proxy -> WireGuard tunnel -> Germany VPS -> internet
```

## Binaries

- `vpn-node-daemon`: runs on the Germany VPS and grants temporary WireGuard peers through an admin-token protected HTTP API.
- `vpn-client`: runs locally, requests a temporary session, starts a WireGuard tunnel, starts a loopback-only SOCKS5 proxy, and launches the child command with proxy env vars.

## Build

```sh
cd vpnnode
cargo build
```

## Germany VPS setup

Install WireGuard and configure `wg0` with `configs/wg-server.example.conf` as a starting point. Enable forwarding and NAT on the VPS.

Create a daemon config:

```sh
cp configs/vpn-node.example.toml vpn-node.toml
```

Set these values:

- `admin_token`: a secret token shared only with trusted clients.
- `server_public_key`: public key for the VPS WireGuard interface.
- `endpoint`: `GERMANY_VPS_IP:51820`.
- `wg_interface`: usually `wg0`.

Run:

```sh
VPN_NODE_ADMIN_TOKEN="change-me" cargo run -p vpn-node-daemon -- --config vpn-node.toml
```

The session API is:

```text
POST /sessions
GET /sessions/:session_id
DELETE /sessions/:session_id
GET /health
```

Use `Authorization: Bearer <admin_token>` or `X-Admin-Token: <admin_token>` for session endpoints.

## Local VPN client

Create a local config:

```sh
cp configs/vpn-client.example.toml vpn-client.toml
```

Set:

- `node_url`: the daemon URL.
- `admin_token`: the daemon admin token.
- `expected_exit_ip`: the Germany VPS public IP.

Run Codex through the Germany node:

```sh
sudo -E cargo run -p vpn-client-cli -- --config vpn-client.toml run --region germany --duration 30m -- codex
```

Run a test command:

```sh
sudo -E cargo run -p vpn-client-cli -- --config vpn-client.toml run --region germany --duration 5m -- curl ifconfig.me
```

Generate a WireGuard client config for a person or device:

```sh
cargo run -p vpn-client-cli -- --config vpn-client.toml config --region germany --duration 30m --output client.conf
```

The generated config can be imported into the WireGuard app or used with:

```sh
sudo wg-quick up ./client.conf
```

The daemon keeps that peer active until the requested duration expires. To revoke
it before expiry, call `DELETE /sessions/:session_id` with the printed session ID.

Check the active local status from another terminal:

```sh
cargo run -p vpn-client-cli -- --config vpn-client.toml status
```

## Safety notes

- The client private key is generated locally and is only written to a temporary WireGuard config.
- Only the client public key is sent to `vpn-node-daemon`.
- The SOCKS5 proxy refuses non-loopback bind addresses.
- If the proxy or WireGuard interface dies, `vpn-client` kills the child process.
- On normal exit or Ctrl+C, `vpn-client` revokes the daemon session, brings the tunnel down, stops the proxy, removes status, and deletes the temporary config directory.
- `vpn-node-daemon` removes peers when sessions expire and removes tracked peers on graceful shutdown.

## Missing production pieces

- Persistent session store for crash recovery.
- TLS termination for the daemon if it is exposed directly.
- Systemd unit files and hardened Linux firewall rules.
- Payment/MPP integration.
- Multi-region routing.
