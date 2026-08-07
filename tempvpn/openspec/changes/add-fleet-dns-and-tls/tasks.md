## 1. Terraform DNS and ingress configuration

- [x] 1.1 Add the six canonical node hostnames plus validated per-node TLS and legacy-compatibility rollout sets to Terraform variables and example configuration.
- [x] 1.2 Add mixed-mode instance tags and scoped firewall rules so nodes can expose port 8080, ports 80/443, or both during the compatibility window.
- [x] 1.3 Render daemon bind, advertised node URL, and regional registry URL from each node's rollout state without changing WireGuard endpoints or payment configuration.
- [x] 1.4 Output the exact Namecheap A-record map and effective per-node/registry API origins for operator review.

## 2. Node TLS provisioning

- [x] 2.1 Extend startup provisioning to install and configure Caddy only for TLS-enabled nodes and reverse-proxy to the loopback daemon backend.
- [x] 2.2 Add service ordering and local health checks so daemon and proxy failures are visible without exposing credentials.
- [x] 2.3 Update the Terraform runbook with DNS-first staging, certificate verification, existing-session compatibility, backend-port retirement, and per-node rollback.

## 3. Static validation

- [x] 3.1 Format and validate Terraform and render startup metadata for both a legacy node and a TLS-enabled node.
- [x] 3.2 Produce a refreshed Terraform plan and verify it does not replace or destroy any VM, disk, or reserved public address.
- [x] 3.3 Run strict OpenSpec validation and relevant workspace tests.

## 4. Live DNS and staged rollout

- [x] 4.1 Add and independently resolve all six authoritative DNS A records against the Terraform-reserved IPv4 addresses.
- [x] 4.2 Enable and verify HTTPS on US East and Belgium one at a time, including `/health`, regional catalogs, and node registration.
- [ ] 4.3 Enable and verify HTTPS on the other four nodes one at a time, confirming their renewed catalog origins before proceeding.
- [ ] 4.4 Update global aggregator upstreams to the regional HTTPS origins and verify the combined six-node catalog.
- [ ] 4.5 After the compatibility window, verify direct public port 8080 is closed for every migrated node and document the final rollback state.
