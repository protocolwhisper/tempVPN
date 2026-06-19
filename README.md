# Tempo Workspace

```text
foundry/  Solidity AccountKeychain precompile example and tests.
rust/     Rust MPP payment service and client.
tunnel_terraform/  Terraform for the GCP VPN node.
vpnnode/  Rust WireGuard VPN node API and client CLI.
```

## Releasing the VPN Client

Push a version tag to build and publish downloadable `vpn-client` binaries:

```sh
git tag v0.1.0
git push origin v0.1.0
```

The `Release VPN client` workflow uploads Linux musl, macOS, and Windows archives
plus `SHA256SUMS` to the GitHub Release for that tag.
