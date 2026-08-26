## Why

The production fleet is running an older daemon with development MPP defaults, while automatic stale-session pause can stop billing without removing the WireGuard peer. The directory and deployed discovery surfaces also disagree with the corrected POST-only streaming contract, so the reviewed fixes are not yet safe or usable end to end.

## What Changes

- Make stale-session auto-pause remove the authorized WireGuard peer and add regression coverage for the billing/network-routing invariant.
- Render every production Tempo MPP setting into the node startup configuration, including mainnet RPC, realm, currency, chain ID, streaming Session v2 settings, durable storage, and the server-side close key.
- Keep `MPP_SECRET_KEY` and the Session v2 close private key server-side and sourced from Secret Manager/Terraform-sensitive inputs.
- Correct the MPP directory streaming route to `POST /sessions/stream` and keep the intended shared `tempvpn.xyz` realm consistent with runtime challenges.
- Rebuild and roll out the daemon fleet, restore the global registry through its existing change, and verify the public API contract without transmitting real payment credentials.
- Do not modify landing-page or metadata-polish work tracked as review item 7.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `vpn-streaming-payments`: Consolidate the production Session v2 contract around standard-wallet signatures, POST-only creation, a configured production realm and chain, and durable server-side state.

## Impact

- Node daemon session cleanup and tests.
- Terraform variables, rendered startup configuration, Secret Manager integration, daemon artifact, and six GCP VMs.
- The nested MPP directory service entry.
- The existing global-registry aggregator deployment change and public smoke checks.
- Payment and credential configuration changes from development defaults to Tempo mainnet; session expiry and network routing change so billing never stops while a peer remains authorized.
- Linux and macOS request formats remain compatible. Rollback uses the previous content-addressed daemon artifact and Terraform configuration, but would reintroduce the reported security and payment incompatibilities.
