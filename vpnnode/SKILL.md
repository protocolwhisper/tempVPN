---
name: paid-wireguard-vpn-client
description: Use this skill when an agent needs to buy and use a temporary WireGuard VPN session from the MPP-protected vpn-node-daemon at 34.30.107.52:8080. It covers the paid POST /sessions flow, mppx usage for HTTP 402 payment handling, and WireGuard client config generation.
---

# Paid WireGuard VPN Client

This server sells temporary WireGuard sessions through an MPP-protected HTTP API.

## Service

- Base URL: `http://34.30.107.52:8080`
- Paid endpoint: `POST /sessions`
- Payment method: MPP `tempo` charge
- Payment recipient: `0xB01E80a8CD7C72589f30D2004aeb60937a2150d3`
- Configured price: `0.01` of the configured Tempo currency
- Currency: `0x20c0000000000000000000000000000000000000`
- Session expiry: automatic; do not call delete for normal client usage

## When Payment Is Needed

Call `POST /sessions` to create a session. If the request is unpaid, the server returns `402 Payment Required` with a `WWW-Authenticate: Payment ...` challenge. Do not use admin tokens or bypass endpoints for client access.

If the agent does not already have an MPP-capable HTTP client, use the `mppx` CLI from the MPP agent quickstart:

```bash
npm install -g mppx
mppx account create
```

Then use `mppx` to make the paid HTTP request. If unsure about exact POST/JSON flags for the installed version, run:

```bash
mppx --help
```

Reference: `https://mpp.dev/quickstart/agent#mppx`

## Create A Session

Generate a WireGuard keypair locally. Send only the public key to the server.

Request body:

```json
{
  "client_public_key": "<wireguard-client-public-key>",
  "duration_seconds": 1800
}
```

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

Build a local WireGuard config from the response:

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

Bring the tunnel up with the local platform's normal WireGuard tooling. The server removes the peer automatically when `expires_at` is reached.

## Important Rules

- Never send the client private key to the server.
- Never ask for or use the daemon admin token for normal paid client access.
- Do not call `DELETE /sessions/:id` in the normal client flow; expiry cleanup is automatic.
- If a payment challenge is returned, use an MPP client such as `mppx` to pay and retry the same request.
