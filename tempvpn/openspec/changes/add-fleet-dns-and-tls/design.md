## Context

See `proposal.md` for motivation. The deployed fleet has six regional VMs with reserved Standard Tier IPv4 addresses. Every daemon currently binds public TCP port 8080 and advertises an `http://<ip>:8080` origin; two of those daemons are regional registries. Namecheap is authoritative for `tempvpn.xyz`, while Terraform owns the VMs, firewall, startup configuration, and reserved addresses.

DNS entry and certificate issuance cannot be one atomic operation: the A record must resolve before an ACME client can complete validation. Existing paid sessions also retain their creating-node URL locally. The rollout therefore needs a per-node compatibility window rather than a fleet-wide cutover.

## Goals / Non-Goals

**Goals:**

- Give each node a predictable hostname matching its logical Terraform key.
- Encrypt public daemon API traffic and close direct backend ingress after verification.
- Allow one-node-at-a-time migration and rollback without replacing VMs or static IPs.
- Keep regional self-registration and global aggregation on stable HTTPS origins.

**Non-Goals:**

- Proxying WireGuard UDP through the TLS layer or changing tunnel routing.
- Moving Namecheap zone ownership into Terraform or storing Namecheap credentials.
- Replacing node-bound MPP/session semantics with a central payment proxy.
- Providing HTTPS for arbitrary operator-supplied development URLs.

## Decisions

### One hostname per logical node key

The production mapping is:

| Node | Hostname |
| --- | --- |
| US East | `us-east.tempvpn.xyz` |
| US West | `us-west.tempvpn.xyz` |
| Sao Paulo | `sao-paulo.tempvpn.xyz` |
| Belgium | `belgium.tempvpn.xyz` |
| Madrid | `madrid.tempvpn.xyz` |
| Singapore | `singapore.tempvpn.xyz` |

Each Namecheap A record points directly to its existing Terraform-reserved IPv4 address. Direct records preserve node identity and failure isolation. A wildcard or shared proxy was rejected because it would introduce another global data-plane dependency and make payment routing less obvious.

### Per-node Caddy TLS termination

Caddy runs as a system service on each TLS-enabled VM, obtains and renews a public certificate for that node's hostname, and reverse-proxies to the local daemon. The daemon binds loopback on TLS-enabled nodes and remains the only owner of API/payment/session behavior. Caddy's certificate state lives on the node's persistent boot disk; it receives no daemon-admin, registry-write, MPP-signing, wallet, session, or client-private-key credential.

An external HTTPS load balancer was rejected for the MVP because six regional backends, health checks, certificates, and routing rules add fixed cost and a shared routing layer. Manually provisioned certificates were rejected because renewal would become an operator secret-handling burden.

### Explicit per-node rollout switch

Terraform exposes a set of TLS-enabled logical node keys and a separate set of nodes retaining legacy HTTP ingress. A disabled node keeps its current public bind and `http://<reserved-ip>:8080` advertisement. An enabled node moves the daemon to a distinct loopback backend port, gets HTTP/HTTPS ingress tags, Caddy configuration, and `https://<hostname>` advertisement; when legacy ingress is retained, Caddy also reverse-proxies public port 8080 to the same backend for existing sessions. Regional registry URLs are derived from the registry node's own TLS switch, so non-registry nodes move to HTTPS registration only after their regional registry does.

Separate instance tags scope firewall rules correctly during mixed mode, including the temporary state where both TLS and legacy ingress are allowed. A single fleet-wide boolean was rejected because it cannot retain a working path for existing sessions while nodes are verified sequentially.

### DNS remains a reviewed manual boundary

Terraform outputs the exact Namecheap A-record table but does not manage the zone or accept registrar credentials. Operators enter records with a short rollout TTL, verify public resolution from independent resolvers, then enable TLS for the corresponding node. The `registry.tempvpn.xyz` record remains owned by the global registry aggregator change.

### Existing sessions determine retirement timing

Enabling HTTPS changes the URL embedded in new paid responses and registry leases; it does not rewrite existing local session state. Port 8080 retirement for a node occurs only after HTTPS verification and an operator-reviewed compatibility window. If strict immediate port closure is required, any active session created against the old origin must first end or be explicitly accepted as non-reconnectable.

Linux and macOS clients use the same URL compatibility behavior and require no platform-specific tunnel change. macOS still uses its native Network Extension; Linux still uses `wg-quick`.

## Risks / Trade-offs

- [DNS propagation delays ACME issuance] → Create and externally resolve each A record before enabling that node; keep its old origin during the compatibility window.
- [Automated certificate issuance is rate-limited] → Roll out sequentially, use stable hostnames, and avoid repeated instance recreation.
- [Changing a creating-node URL can affect reconnect/pause for old sessions] → Preserve old ingress until the compatibility window ends and never rewrite saved session origins.
- [A TLS proxy adds another local process] → Use systemd restart behavior and verify both proxy and daemon health before advertisement.
- [Port 80 is public for ACME and redirect] → Expose only ports 80 and 443 to TLS-enabled tags; keep daemon port loopback/firewalled.
- [Manual DNS can drift from Terraform outputs] → Publish the exact expected record map and validate it before each cutover.

## Migration Plan

1. Add hostname and per-node TLS rollout configuration, scoped firewall tags/rules, Caddy provisioning, and expected DNS outputs without enabling any node.
2. Confirm a Terraform plan updates metadata/firewall configuration without replacing VMs, disks, or reserved IPs.
3. Add the six Namecheap A records at a short TTL and verify each hostname resolves to the expected reserved IP.
4. Enable the two regional registry nodes one at a time; rerun startup provisioning or restart them, verify certificates and `/health`, then verify their catalogs.
5. Enable each remaining node one at a time, verifying public HTTPS health and its renewed registry record before continuing.
6. Point the global aggregator upstreams at the two regional HTTPS registry origins, then verify the six-node combined catalog.
7. After the active-session compatibility window, remove direct public backend ingress for every completed node and raise DNS TTL to the steady-state value.

Rollback one node by disabling its TLS switch, restoring its previous advertised IP origin and backend firewall tag, restarting provisioning, and retaining its reserved IP/A record for diagnosis. Roll back the global aggregator separately; neither action changes payment secrets, session data, or WireGuard configuration.
