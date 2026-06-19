---
name: tempvpn
description: Use this skill when the user says "load tempvpn" or asks to buy, start, connect, route traffic through, disconnect from, install the client for, verify, or generate config for a temporary WireGuard VPN using Tempo/MPP, including phrases like "use tempo to buy 30 min vpn", "buy a 30 minute VPN", "use tempvpn", or "load tempvpnskill". It covers GitHub client binary discovery, the paid POST /sessions flow for the vpn-node-daemon at 34.30.107.52:8080, duration conversion, mppx/payment handling, local WireGuard setup, ipinfo verification, and local-only disconnect cleanup with no revoke/delete access.
---

# Paid WireGuard VPN Client

This skill teaches an agent how to buy and use a temporary WireGuard VPN session from the VPN node service using Tempo MPP payment.

## Intent Mapping

When the user says something like:

- "use tempo to buy 30 min vpn"
- "buy a 30 minute VPN"
- "start a paid VPN with Tempo"
- "get me a temporary WireGuard VPN"
- "use the VPN node service"

Interpret that as: create a paid VPN session from `POST /sessions` using Tempo MPP, with the requested duration. For "30 min", send `duration_seconds: 1800`.

If the user asks to "use", "start", "connect", or "route traffic", create the paid session and then bring up WireGuard locally if the environment has `wg`, `wg-quick`, and permission to create network interfaces. If the environment lacks those permissions, generate a WireGuard config file and explain the command needed to import or bring it up.

If the user asks to "disconnect", "stop", "turn off", or "end the VPN", only bring down the local WireGuard tunnel/interface/config. The client flow has no revoke/delete/admin access. Do not call, ask for, or depend on any daemon revoke/delete endpoint; the server-side session expires automatically.

If the user asks to "install", "download", "get the binary", or lacks a local `vpn-client`, fetch the latest release binary from GitHub before continuing. For paid Tempo purchase requests, still use the paid HTTP flow unless the binary has been updated to support MPP.

## Service

- Base URL: `http://34.30.107.52:8080`
- Paid endpoint: `POST /sessions`
- Payment method: MPP `tempo` charge
- Payment recipient: `0xB01E80a8CD7C72589f30D2004aeb60937a2150d3`
- Configured price: `0.01` of the configured Tempo currency
- Currency: `0x20c0000000000000000000000000000000000000`
- Session expiry: automatic; the client must not call revoke/delete endpoints

## Important Implementation Note

The Rust `vpn-client` CLI in this repo is the local connection tool. The agent should treat payment and connection as two separate steps:

1. Use `mppx` from `https://mpp.dev/quickstart/agent` to pay for `POST /sessions`.
2. Save the paid session JSON.
3. Use the Rust `vpn-client` binary with `--session-response` and `--private-key-path` to generate config, connect, or run through the tunnel.

The Rust CLI can still create the paid session internally as a convenience, but the skill flow should prefer the explicit `mppx` payment step followed by the Rust binary connection step. Never use the daemon admin token for client session creation or disconnect cleanup.

## Get Client Binary From GitHub

The repo publishes `vpn-client` binaries through GitHub Releases at:

```text
https://github.com/protocolwhisper/Einfach-/releases/latest
```

Release asset names from the workflow:

- Linux x86_64: `vpn-client-x86_64-unknown-linux-musl.tar.gz`
- macOS Intel: `vpn-client-x86_64-apple-darwin.tar.gz`
- macOS Apple Silicon: `vpn-client-aarch64-apple-darwin.tar.gz`
- Windows x86_64: `vpn-client-x86_64-pc-windows-msvc.zip`
- Checksums: `SHA256SUMS`

Select the asset matching the current OS and CPU. Example for macOS Apple Silicon:

```bash
curl -L -o vpn-client.tar.gz https://github.com/protocolwhisper/Einfach-/releases/latest/download/vpn-client-aarch64-apple-darwin.tar.gz
tar -xzf vpn-client.tar.gz
chmod +x vpn-client
./vpn-client --help
```

Example for Linux x86_64:

```bash
curl -L -o vpn-client.tar.gz https://github.com/protocolwhisper/Einfach-/releases/latest/download/vpn-client-x86_64-unknown-linux-musl.tar.gz
tar -xzf vpn-client.tar.gz
chmod +x vpn-client
./vpn-client --help
```

If there is no published release asset yet, build locally from `vpnnode` with `cargo build --release -p vpn-client-cli`.

## Payment Flow

Call `POST /sessions` to create a session. If the request is unpaid, the server returns `402 Payment Required` with a `WWW-Authenticate: Payment ...` challenge. Do not use admin tokens, revoke/delete endpoints, or bypass endpoints for client access.

If using the Rust CLI, first configure `mppx` with the MPP agent quickstart. If the agent does not already have `mppx`, install it from the MPP agent quickstart:

```bash
npm install -g mppx
mppx account create
```

The preferred skill flow uses `mppx` directly for the paid HTTP request. If unsure about exact POST/JSON flags for the installed version, run:

```bash
mppx --help
```

Reference: `https://mpp.dev/quickstart/agent#mppx`

## Create A Paid Session

Generate a WireGuard keypair locally. Send only the public key to the server.

Use the requested duration in seconds:

- `5 min` -> `300`
- `30 min` -> `1800`
- `1 hour` -> `3600`

Request body:

```json
{
  "client_public_key": "<wireguard-client-public-key>",
  "duration_seconds": 1800
}
```

Agent procedure:

1. Check for WireGuard tools with `wg --version` and, if connecting locally, `wg-quick --version`.
2. Generate a local WireGuard private key and public key:

```bash
wg genkey | tee /tmp/vpn-client.key | wg pubkey > /tmp/vpn-client.pub
chmod 600 /tmp/vpn-client.key
```

3. Send the paid `POST /sessions` request through `mppx` and save the JSON response:

```bash
mppx http://34.30.107.52:8080/sessions \
  --json-body "{\"client_public_key\":\"$(cat /tmp/vpn-client.pub)\",\"duration_seconds\":1800}" \
  --silent > /tmp/vpn-session.json
```

4. Use the Rust binary to consume the paid session and connect:

```bash
sudo ./vpn-client connect \
  --session-response /tmp/vpn-session.json \
  --private-key-path /tmp/vpn-client.key
```

For config generation without bringing up a tunnel:

```bash
./vpn-client config \
  --session-response /tmp/vpn-session.json \
  --private-key-path /tmp/vpn-client.key \
  --output client.conf
```

5. Save the response fields needed for the WireGuard config: `assigned_ip`, `server_public_key`, `endpoint`, and `expires_at`.

The successful response contains:

```json
{
  "session_id": "sess_...",
  "assigned_ip": "10.8.0.x/32",
  "server_public_key": "GM/WPqqgqiRlrrd++b/dvrK/bgcOjXLNrNKzmdlvHWg=",
  "endpoint": "34.30.107.52:51820",
  "created_at": "...",
  "expires_at": "..."
}
```

## WireGuard Config

Build a local WireGuard config from the response. Keep the private key local:

```ini
[Interface]
PrivateKey = <client-private-key>
Address = <assigned_ip>
DNS = 1.1.1.1

[Peer]
PublicKey = <server_public_key>
Endpoint = <endpoint>
AllowedIPs = 0.0.0.0/0, ::/0
PersistentKeepalive = 25
```

For a local tunnel on a machine with WireGuard privileges:

```bash
sudo wg-quick up ./client.conf
```

## Verify The VPN

After the tunnel is up, verify the visible public IP:

```bash
curl -s https://ipinfo.io/json
```

The response should show the VPN egress IP, normally `34.30.107.52` for the current node. Treat this verification as part of the normal completion workflow. Report the `ip`, `city`, `region`, `country`, and `org` fields back to the user when available.

If the returned `ip` is not the VPN node IP, do not claim the VPN is active. Check that `wg-quick up` succeeded, the WireGuard interface exists, and the config uses `AllowedIPs = 0.0.0.0/0, ::/0`.

## Disconnect

Disconnect means local tunnel teardown only. The paid client does not have revoke or delete access, and it must not attempt server-side session deletion. The daemon expires the paid session automatically at `expires_at`.

For a WireGuard config brought up with `wg-quick`, disconnect locally with:

```bash
sudo wg-quick down ./client.conf
```

If the config was written to a specific path, use that path:

```bash
sudo wg-quick down /path/to/client.conf
```

After disconnecting, verify traffic is no longer using the VPN:

```bash
curl -s https://ipinfo.io/json
```

The returned `ip` should no longer be `34.30.107.52`. Report the new visible `ip` to the user.

The server removes the peer automatically when `expires_at` is reached, so no daemon admin token, revoke call, or delete call is needed or allowed for normal paid usage.

## Important Rules

- Never send the client private key to the server.
- Never ask for or use the daemon admin token for normal paid client access.
- Never call revoke or delete endpoints in the normal paid client flow. The skill is for paid client access and local tunnel disconnect only; expiry cleanup is automatic.
- If a payment challenge is returned, use an MPP client such as `mppx` to pay and retry the same request.
- If a command needs network installation, payment credentials, or `sudo`, request approval before continuing.
