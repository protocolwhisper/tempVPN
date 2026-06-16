# Tempo AccountKeychain Foundry Example

This project shows how to build calldata for the Tempo `AccountKeychain` precompile at:

```text
0xAAAAAAAA00000000000000000000000000000000
```

The local tests install a mock at that same address with `vm.etch`, so `forge test` is deterministic. To call the real precompile, use the script against a Tempo RPC node.

## Local tests

```sh
forge test --offline
```

The two tests cover:

- root key authorizes `sessionKey` for `tokenX` with a spending limit and `transfer(address,uint256)` scope to one recipient
- root key revokes that session key

## Real Tempo call

Set these env vars:

```sh
export TEMPO_RPC_URL="https://rpc.moderato.tempo.xyz"
export ROOT_PRIVATE_KEY="0x..."
export SESSION_KEY="0x..."
export TOKEN_X="0x..."
export RECIPIENT="0x..."
export SPEND_LIMIT="100000000000000000000"
```

Then broadcast the direct precompile call:

```sh
forge script script/AuthorizeSessionKey.s.sol:AuthorizeSessionKey \
  --rpc-url moderato \
  --chain-id 42431 \
  --broadcast \
  --skip-simulation
```

`--skip-simulation` matters because normal Foundry does not implement Tempo's custom Rust precompile locally. The transaction must execute on a Tempo node.

Moderato testnet:

```text
HTTP RPC: https://rpc.moderato.tempo.xyz
WS RPC:   wss://rpc.moderato.tempo.xyz
Chain ID: 42431
```
