## Why

The six live node APIs are advertised as bare public IPv4 addresses over HTTP, so paid session creation and lifecycle traffic lack stable names and transport security. Each node needs an independently routable DNS name and HTTPS endpoint before the fleet can safely serve public clients.

## What Changes

- Assign one `tempvpn.xyz` hostname to each of the six reserved node IPv4 addresses: `us-east`, `us-west`, `sao-paulo`, `belgium`, `madrid`, and `singapore`.
- Terminate automatically renewed TLS on every node and reverse-proxy public HTTPS traffic to the local daemon.
- Advertise HTTPS node, registry, and payment URLs while preserving the exact selected-node binding throughout payment and session lifecycle calls.
- Restrict direct public access to the daemon's backend port after HTTPS verification; retain WireGuard UDP ingress unchanged.
- Document staged Namecheap DNS entry, certificate readiness checks, per-node rollout, and rollback to the reserved IP endpoints.
- Coordinate the regional registry hostnames with the separate global registry aggregator change.
- Do not alter payment recipients, MPP challenge semantics, credential boundaries, connected-time expiry, client private-key handling, or WireGuard routing.

## Capabilities

### New Capabilities

- `fleet-dns-tls`: Stable per-node DNS names, managed HTTPS ingress, safe advertisement, and staged rollback for every public node API.

### Modified Capabilities

- `node-allocation`: Catalog records and node-bound lifecycle responses advertise the selected node's stable HTTPS origin instead of a bare HTTP address.

## Impact

- `infra/terraform`: hostname configuration, HTTP/HTTPS firewall rules, VM startup provisioning for TLS termination, advertised URLs, outputs, and rollout documentation.
- Namecheap authoritative DNS: six node A records; the separate aggregator change owns `registry.tempvpn.xyz`.
- Node daemon/configuration: remains a loopback or firewalled HTTP backend behind the TLS proxy; no payment or session protocol change.
- Regional registries: self-registration uses stable HTTPS registry origins after each registry node is ready.
- Linux client, macOS client, and agent skill: protocol-compatible HTTPS URLs replace bare HTTP URLs; explicit development overrides remain supported.
- Rollback restores the prior advertised IP URLs and public backend-port rule while leaving reserved addresses, sessions, and WireGuard endpoints intact.
