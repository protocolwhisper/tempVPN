# VPN Node Client MVP

This directory contains a Rust MVP for one managed egress node: `us`.

It deliberately does not implement payment or multi-region routing. The target path is:

```text
Client/Codex -> 127.0.0.1:1080 SOCKS5 proxy -> WireGuard tunnel -> USA VPS -> internet
```

## Binaries

- `vpn-node-daemon`: runs on the USA VPS and grants temporary WireGuard peers through an admin-token protected HTTP API.
- `vpn-client`: runs locally, requests a temporary session, starts a WireGuard tunnel, starts a loopback-only SOCKS5 proxy, and launches the child command with proxy env vars.

## Build

```sh
cd vpnnode
cargo build
```

## USA VPS setup

Install WireGuard and configure `wg0` with `configs/wg-server.example.conf` as a starting point. Enable forwarding and NAT on the VPS.

Create a daemon config:

```sh
cp configs/vpn-node.example.toml vpn-node.toml
```

Set these values:

- `admin_token`: a secret token shared only with trusted clients.
- `server_public_key`: public key for the VPS WireGuard interface.
- `endpoint`: `USA_VPS_IP:51820`.
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

The client machine needs:

- Rust/Cargo to build `vpn-client`.
- WireGuard tools on `PATH`: `wg` and `wg-quick`.
- Permission to create a WireGuard interface, so tunnel commands usually run with `sudo`.
- Network access to the node API. For now the Rust client defaults to `http://34.30.107.52:8080`.
- The daemon admin token, provided with `--admin-token`, `VPN_CLIENT_ADMIN_TOKEN`, or a local config file.

The shortest path is no config file. Build once, then connect in one command:

```sh
cargo build -p vpn-client-cli
sudo ./target/debug/vpn-client --admin-token "$VPN_NODE_ADMIN_TOKEN" connect --region us --duration 30m
```

Disconnect and revoke the server session:

```sh
sudo ./target/debug/vpn-client --admin-token "$VPN_NODE_ADMIN_TOKEN" disconnect
```

You can also create a local config for overrides:

```sh
cp configs/vpn-client.example.toml vpn-client.toml
```

Optional values:

- `node_url`: the daemon URL, for example `http://34.30.107.52:8080`.
- `admin_token`: the daemon admin token. Prefer `--admin-token` or `VPN_CLIENT_ADMIN_TOKEN`.
- `expected_exit_ip`: the USA VPS public IP.

Do not paste the token into chat or commit it. On the VPN server, read it locally:

```sh
sudo sed -n 's/^admin_token = "\(.*\)"/\1/p' /etc/vpn-node-daemon/vpn-node.toml
```

Then put it only in your local `vpn-client.toml`, or export it for one shell:

```sh
export VPN_CLIENT_ADMIN_TOKEN="..."
```

Passing secrets as command-line args can expose them to local process listings
while the command is running. For your own machine this may be acceptable during
testing; `VPN_CLIENT_ADMIN_TOKEN` is safer.

Run Codex through the USA node:

```sh
sudo ./target/debug/vpn-client --admin-token "$VPN_NODE_ADMIN_TOKEN" run --region us --duration 30m -- codex
```

Run a test command:

```sh
sudo ./target/debug/vpn-client --admin-token "$VPN_NODE_ADMIN_TOKEN" run --region us --duration 5m -- curl ifconfig.me
```

Generate a WireGuard client config for a person or device:

```sh
cargo run -p vpn-client-cli -- --admin-token "$VPN_NODE_ADMIN_TOKEN" config --region us --duration 30m --output client.conf
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
