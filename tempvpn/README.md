# VPN Node Client MVP

This directory contains a Rust MVP for one managed US egress node.

The target path is:

```text
Client/Codex -> 127.0.0.1:1080 SOCKS5 proxy -> WireGuard tunnel -> USA VPS -> internet
```

## Binaries

- `vpn-node-daemon`: runs on the USA VPS and grants temporary WireGuard peers through an MPP-protected HTTP API.
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
- `mpp_payment_recipient`: payment recipient for paid session creation. The example defaults to `0xB01E80a8CD7C72589f30D2004aeb60937a2150d3`.

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

`POST /sessions` is MPP-protected and returns `402 Payment Required` until the request includes a valid Tempo MPP receipt. Use `Authorization: Bearer <admin_token>` or `X-Admin-Token: <admin_token>` for the `GET` and `DELETE` session management endpoints.

## Local VPN client

The client machine needs:

- Rust/Cargo to build `vpn-client`.
- WireGuard tools on `PATH`: `wg` and `wg-quick`.
- Permission to create a WireGuard interface, so tunnel commands usually run with `sudo`.
- Network access to the node API. For now the Rust client defaults to `http://34.30.107.52:8080`.
- `mppx` installed and configured from the MPP agent quickstart: `https://mpp.dev/quickstart/agent`.
- A funded/default `mppx` account, for example from `mppx account create` and the quickstart funding step.

The explicit agent flow is: create a local WireGuard keypair, pay for a session with `mppx`, then use `vpn-client` to connect with the paid session response.

```sh
cargo build -p vpn-client-cli
wg genkey | tee /tmp/vpn-client.key | wg pubkey > /tmp/vpn-client.pub
chmod 600 /tmp/vpn-client.key
mppx http://34.30.107.52:8080/sessions \
  --json-body "{\"client_public_key\":\"$(cat /tmp/vpn-client.pub)\",\"duration_seconds\":1800}" \
  --silent > /tmp/vpn-session.json
sudo ./target/debug/vpn-client connect \
  --session-response /tmp/vpn-session.json \
  --private-key-path /tmp/vpn-client.key
```

Disconnect the local tunnel. The paid server-side session expires automatically:

```sh
sudo ./target/debug/vpn-client disconnect
```

You can also create a local config for overrides:

```sh
cp configs/vpn-client.example.toml vpn-client.toml
```

Optional values:

- `node_url`: the daemon URL, for example `http://34.30.107.52:8080`.
- `mppx_command`: path to the `mppx` binary. Defaults to `mppx`.
- `mppx_account`: optional named `mppx` account. Defaults to the `mppx` default account or `MPPX_ACCOUNT`.
- `mppx_config`: optional `mppx` config path.
- `mppx_network`: optional Tempo network override, for example `testnet`.
- `mppx_rpc_url`: optional Tempo RPC override. Defaults to `mppx` behavior or `MPPX_RPC_URL`.
- `expected_exit_ip`: the USA VPS public IP.

Do not paste private keys into chat or commit them. Manage the payer account with `mppx`:

```sh
mppx account create
mppx account list
```

Run Codex through the USA node:

```sh
sudo ./target/debug/vpn-client run --duration 30m -- codex
```

Run a test command:

```sh
sudo ./target/debug/vpn-client run --duration 5m -- curl ifconfig.me
```

Generate a WireGuard client config for a person or device:

```sh
cargo run -p vpn-client-cli -- config \
  --session-response /tmp/vpn-session.json \
  --private-key-path /tmp/vpn-client.key \
  --output client.conf
```

The generated config can be imported into the WireGuard app or used with:

```sh
sudo wg-quick up ./client.conf
```

The daemon keeps that peer active until the requested duration expires. The paid
client does not revoke or delete server sessions.

Check the active local status from another terminal:

```sh
cargo run -p vpn-client-cli -- --config vpn-client.toml status
```

## Safety notes

- The client private key is generated locally and is only written to a temporary WireGuard config.
- Only the client public key is sent to `vpn-node-daemon`.
- The SOCKS5 proxy refuses non-loopback bind addresses.
- If the proxy or WireGuard interface dies, `vpn-client` kills the child process.
- On normal exit or Ctrl+C, `vpn-client` brings the tunnel down, stops the proxy, removes status, and deletes the temporary config directory. The daemon session expires automatically.
- `vpn-node-daemon` removes peers when sessions expire and removes tracked peers on graceful shutdown.

## Missing production pieces

- Persistent session store for crash recovery.
- TLS termination for the daemon if it is exposed directly.
- Systemd unit files and hardened Linux firewall rules.
- Multi-region routing.
