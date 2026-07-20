---
name: tempvpn
description: Discover, purchase, connect, disconnect, and verify temporary WireGuard VPN sessions through the TempVPN node registry and Tempo MPP on Linux or macOS.
---

# Paid WireGuard VPN Client

This skill teaches an agent how to buy and use a temporary WireGuard VPN session from the VPN node service using Tempo MPP payment.

## Supported Platforms

Use the Rust `vpn-client` with `wg`/`wg-quick` on Linux. On macOS use the signed
`tempvpnctl` plus the invisible `TempVPN.app` that contains the Packet Tunnel
Provider and WireGuardKit. The macOS host has no graphical interface; it exists
only because macOS requires an app container for Network Extensions. Windows is
not supported. Never use the Rust client as the macOS product path.

## Intent Mapping

When the user says something like:

- "use tempo to buy 30 min vpn"
- "buy a 30 minute VPN"
- "start a paid VPN with Tempo"
- "get me a temporary WireGuard VPN"
- "use the VPN node service"

Interpret that as: create a paid VPN session from `POST /sessions` using Tempo MPP, with the requested duration, then immediately connect the local WireGuard tunnel and verify the public IP. For "30 min", send `duration_seconds: 1800`. Only stop after purchasing if the user explicitly asks to purchase a session without connecting.

If the user asks to "use", "start", "connect", or "route traffic", create the
paid session and use the designated platform client. Linux requires `wg`,
`wg-quick`, and network-interface permission. macOS must use the installed
native Packet Tunnel path and must not fall back to external WireGuard commands.

If the user asks to "disconnect", "stop", "turn off", or "end the VPN", bring down the local WireGuard tunnel/interface/config and pause the paid server-side usage balance. The client flow has no revoke/delete/admin access. Do not call, ask for, or depend on any daemon revoke/delete endpoint; the server-side session expires automatically when connected time is exhausted or its grace deadline passes.

If the user asks to "install", "download", or lacks the platform command, use
the platform installation section below. Installation and payment are separate
actions. Never hide an installation, administrator prompt, or dependency setup
inside the paid request.

## Registry and service selection

- Registry URL: use `VPN_CLIENT_REGISTRY_URL` or `registry_url` from the client config
- Paid endpoint: `POST /sessions`
- Payment method: MPP `tempo` charge
- Payment recipient, price, and currency are advertised by the selected node's MPP challenge.
- Session expiry: automatic by connected-time balance; the client must not call revoke/delete endpoints

Before payment, select one healthy node. On Linux run `vpn-client select` and
optionally pass `--region <region>`; `--json` returns a machine-readable
`node_url`. An explicit `--node-url` bypasses catalog ranking but is still
health-checked. Pay only `SELECTED_NODE_URL/sessions`, and pass that same URL to
the platform client. Never send payment to the registry URL unless it is also
the selected exit node.

## Important Implementation Note

The Rust `vpn-client` CLI in this repo is the local connection tool. Payment and connection are two technical steps, but they form one continuous default workflow:

1. Discover the node, then use preinstalled `mppx` to pay that node's `POST /sessions`.
2. Save the paid session JSON.
3. On Linux use `vpn-client connect --node-url SELECTED_NODE_URL` with the paid
   response. On macOS pass the response path and selected node URL to
   `tempvpnctl connect`. The selected-node override makes accidental cross-node
   import fail closed; the exact command appears below.

Do not pause for confirmation between successful payment and connection. Never
use a daemon admin or registry-write token for client lifecycle calls. Never
create or replace an MPPX account automatically; account provisioning is a
separate explicit user action.

## Get Client Binary From GitHub

The repo publishes `vpn-client` binaries through GitHub Releases at:

```text
https://github.com/protocolwhisper/tempVPN/releases/latest
```

The Rust release artifact is Linux-only:

- Linux x86_64: `vpn-client-x86_64-unknown-linux-musl.tar.gz`
- Checksums: `SHA256SUMS`

Example for Linux x86_64:

```bash
curl -L -o vpn-client.tar.gz https://github.com/protocolwhisper/tempVPN/releases/latest/download/vpn-client-x86_64-unknown-linux-musl.tar.gz
tar -xzf vpn-client.tar.gz
chmod +x vpn-client
./vpn-client --help
```

If there is no published release asset yet, build locally from the `tempvpn`
directory with `cargo build --release -p vpn-client-cli`.

## Install The Native Headless macOS Client

The macOS release contains two signed products:

```text
/Applications/TempVPN.app   invisible Network Extension container
/usr/local/bin/tempvpnctl   agent-facing controller
```

Do not install unsigned builds and do not substitute `wg`/`wg-quick`. From a
trusted signed source checkout or release package, installation is:

```bash
sudo ./clients/macos/install-tempvpnctl.sh
tempvpnctl --version
```

The installer itself does not invoke `sudo`; the agent must request permission
before running it with administrator privileges. macOS requests one-time VPN
profile approval on first connection. Installation never creates an MPP
account.

## Payment Flow

Call `POST /sessions` to create a session. If the request is unpaid, the server returns `402 Payment Required` with a `WWW-Authenticate: Payment ...` challenge. Do not use admin tokens, revoke/delete endpoints, or bypass endpoints for client access.

If using the Rust CLI, first configure `mppx` with the MPP agent quickstart. If the agent does not already have `mppx`, install it from the MPP agent quickstart:

```bash
npm install -g mppx
mppx account create --account main
```

The account creation command above is initial setup only. On macOS, run account checks and setup with access to the user's real Keychain. Never create a replacement automatically after a purchase failure, never infer absence from a sandboxed account listing, and never expose command output that could contain a generated private key.

Always use the MPPX account named `main` for VPN payments by passing `--account main`; do not rely on whichever account happens to be the default. The preferred skill flow uses `mppx` directly for the paid HTTP request. If unsure about exact POST/JSON flags for the installed version, run:

```bash
mppx --help
```

Reference: `https://mpp.dev/quickstart/agent#mppx`

## Create A Paid Session

Payment creates a node-bound usage balance. It does not send a WireGuard key.
The platform client generates the key locally afterward and sends only its
public key to that same node's connect endpoint.

Use the requested duration in seconds:

- `5 min` -> `300`
- `30 min` -> `1800`
- `1 hour` -> `3600`

Request body:

```json
{
  "duration_seconds": 1800
}
```

Agent procedure:

1. On Linux, check `wg --version` and `wg-quick --version`. On macOS, check
   `tempvpnctl --version` and verify `/Applications/TempVPN.app` is installed;
   do not require external WireGuard tools.
2. On Linux, generate a local WireGuard private key. On macOS, `tempvpnctl`
   generates and stores it in the user's Keychain during session import:

```bash
wg genkey > /tmp/vpn-client.key
chmod 600 /tmp/vpn-client.key
```

3. Send the paid `POST /sessions` request through `mppx` and save the JSON response. This creates a usage balance; connected time starts only when the client activates the session:

```bash
mppx "$SELECTED_NODE_URL/sessions" \
  --account main \
  --json-body "{\"duration_seconds\":1800}" \
  --silent > /tmp/vpn-session.json
```

4. Immediately use the platform client to activate the paid session and
   connect. The client derives the public key locally, calls
   `POST /sessions/{id}/connect`, and starts burning connected-time balance only
   after the peer is active. On Linux, in the repository demo environment:

```bash
sudo ./target/debug/vpn-client connect \
  --node-url "$SELECTED_NODE_URL" \
  --session-response /tmp/vpn-session.json \
  --private-key-path /tmp/vpn-client.key
```

On macOS:

```bash
tempvpnctl connect \
  --session-response /tmp/vpn-session.json \
  --node-url "$SELECTED_NODE_URL" \
  --json
```

For Linux config generation without bringing up a tunnel:

```bash
./vpn-client config \
  --session-response /tmp/vpn-session.json \
  --private-key-path /tmp/vpn-client.key \
  --output client.conf
```

5. Save the response fields needed for the WireGuard config and status: `assigned_ip`, `server_public_key`, `endpoint`, `remaining_seconds`, and `not_after`.

The successful response contains:

```json
{
  "session_id": "sess_...",
  "assigned_ip": "10.8.0.x/32",
  "server_public_key": "GM/WPqqgqiRlrrd++b/dvrK/bgcOjXLNrNKzmdlvHWg=",
  "endpoint": "SELECTED_NODE_IP:51820",
  "expected_exit_ip": "SELECTED_NODE_IP",
  "created_at": "...",
  "not_after": "...",
  "remaining_seconds": 1800,
  "state": "active"
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

For a manual Linux tunnel on a machine with WireGuard privileges:

```bash
sudo wg-quick up ./client.conf
```

## Verify The VPN

After the tunnel is up, verify the visible public IP:

```bash
curl -s https://ipinfo.io/json
```

The response must match the selected node's `expected_exit_ip`. Treat this verification as part of the normal completion workflow. Report the `ip`, `city`, `region`, `country`, and `org` fields back to the user when available.

If the returned `ip` is not the VPN node IP, do not claim the VPN is active. On
Linux, inspect `wg-quick` and the generated configuration. On macOS, run
`tempvpnctl status --json` and inspect the native `TempVPN` profile in System
Settings.

## Disconnect

Disconnect means local tunnel teardown plus pausing the paid usage balance. The paid client does not have revoke or delete access, and it must not attempt server-side session deletion. The daemon expires the paid session when `remaining_seconds` reaches zero or the `not_after` grace deadline passes.

On macOS, always disconnect through the controller so it tears down WireGuard
and pauses the server-side balance:

```bash
tempvpnctl disconnect --json
```

For a Linux WireGuard config brought up manually with `wg-quick`, disconnect
locally with:

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

The returned `ip` should no longer equal the selected node's `expected_exit_ip`. Report the new visible `ip` to the user.

The server removes the peer automatically when `remaining_seconds` reaches zero or `not_after` passes, so no daemon admin token, revoke call, or delete call is needed or allowed for normal paid usage.

## Important Rules

- Never send the client private key to the server.
- Stop before payment on Windows. Use the native client designated above on Linux or macOS.
- Keep platform networking separate: Linux uses `wg`/`wg-quick`; macOS uses the
  signed headless app, Packet Tunnel extension, and WireGuardKit.
- Discover first and ensure MPP payment and all session lifecycle calls target the exact same node.
- Always make VPN payments with the MPPX account named `main` by passing `--account main`.
- A request to buy, start, or use the VPN includes automatic local connection and public-IP verification unless the user explicitly requests purchase only.
- On macOS, use `tempvpnctl select`, pay that node with `mppx`, then import the
  response with `tempvpnctl connect --node-url SELECTED_NODE_URL`.
- Never conclude that `main` is missing from a sandboxed `mppx account list`, and never create or replace it automatically as failure recovery.
- Never expose output from an account-creation failure because it may contain newly generated private-key material.
- Never ask for or use the daemon admin token for normal paid client access.
- Never call revoke or delete endpoints in the normal paid client flow. The skill is for paid client access; disconnect pauses usage balance and expiry cleanup is automatic.
- If a payment challenge is returned, use an MPP client such as `mppx` to pay and retry the same request.
- If a command needs network installation, payment credentials, or `sudo` and no matching approval rule already exists, request approval before continuing.
