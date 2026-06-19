# Tempo Workspace

This repo is split into separate project directories so the Solidity and Rust tooling do not share one root.

```text
foundry/                  Tempo AccountKeychain precompile example and tests
rust/mpp-payment-service/  Rust MPP payment server and client
vpnnode/                  Separate VPN node work
tunnel/                   Separate tunnel infrastructure work
```

## Foundry AccountKeychain

The Foundry project shows how to build calldata for the Tempo `AccountKeychain` precompile at:

```text
0xAAAAAAAA00000000000000000000000000000000
```

Run local deterministic tests:

```sh
cd foundry
forge test --offline
```

The tests install a mock at the precompile address with `vm.etch` and cover:

- root key authorizes `sessionKey` for `tokenX` with a spending limit and `transfer(address,uint256)` scope to one recipient
- root key revokes that session key

To call the real Tempo precompile, set:

```sh
export TEMPO_RPC_URL="https://rpc.moderato.tempo.xyz"
export ROOT_PRIVATE_KEY="0x..."
export SESSION_KEY="0x..."
export TOKEN_X="0x..."
export RECIPIENT="0x..."
export SPEND_LIMIT="100000000000000000000"
```

Then broadcast from the Foundry project:

```sh
cd foundry
forge script script/AuthorizeSessionKey.s.sol:AuthorizeSessionKey \
  --rpc-url moderato \
  --chain-id 42431 \
  --broadcast \
  --skip-simulation
```

`--skip-simulation` matters because normal Foundry does not implement Tempo's custom Rust precompile locally. The transaction must execute on a Tempo node.

## Rust MPP Payment Service

The Rust project is a minimal MPP service using the official `mpp` Rust SDK with Tempo charge payments.

Configure it:

```sh
cd rust/mpp-payment-service
cp .env.example .env
```

Set at least:

```sh
export MPP_SECRET_KEY="replace-with-a-random-server-secret"
export MPP_PAYMENT_RECIPIENT="0xYourTempoAddress"
export TEMPO_RPC_URL="https://rpc.moderato.tempo.xyz"
```

Run the server:

```sh
cd rust/mpp-payment-service
cargo run --bin mpp_service
```

Endpoints:

```text
GET /health
GET /free
GET /paid/time
GET /openapi.json
GET /.well-known/mpp/openapi.json
```

`/paid/time` returns an MPP `402 Payment Required` challenge until a client retries with a valid `Authorization: Payment ...` credential. After payment verifies, the JSON response includes a unique `accessKey`.

Example paid client call:

```sh
cd rust/mpp-payment-service
export TEMPO_PRIVATE_KEY="0x..."
cargo run --bin mpp_client -- http://localhost:3000/paid/time
```

Successful body shape:

```json
{
  "paid": true,
  "accessKey": "550e8400-e29b-41d4-a716-446655440000",
  "reference": "payment-or-tx-reference",
  "now": "1781800000",
  "chainId": 42431
}
```

The Rust client uses `TEMPO_RPC_URL` and the MPP SDK `TempoProvider`. It signs and submits a Tempo testnet payment, so use a funded Moderato testnet wallet.

## Moderato Testnet

```text
HTTP RPC: https://rpc.moderato.tempo.xyz
WS RPC:   wss://rpc.moderato.tempo.xyz
Chain ID: 42431
```

## MCP Setup

Register the Tempo and MPP MCP servers with the current Codex CLI syntax:

```sh
codex mcp add tempo --url "${TEMPO_MCP_URL:-https://mcp.tempo.xyz}"
codex mcp add mpp --url "${MPP_MCP_URL:-https://mpp.dev/api/mcp}"
codex mcp list
```

Do not send wallet private keys or payment secrets to MCP servers.
