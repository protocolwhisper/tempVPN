## 1. Session cleanup safety

- [x] 1.1 Add regression tests proving stale auto-pause removes its WireGuard peer and retries a failed removal
- [x] 1.2 Implement retryable stale-peer cleanup without holding the session-store lock across WireGuard commands

## 2. Production payment configuration

- [x] 2.1 Declare and validate all production MPP, Session v2, durable-store, and Secret Manager Terraform inputs
- [x] 2.2 Render explicit mainnet and Session v2 configuration into the daemon TOML and fetch the close key securely at boot
- [x] 2.3 Update systemd filesystem permissions and infrastructure documentation for the durable Session v2 store and close-key ownership
- [x] 2.4 Add Terraform checks that prevent production deployment with development realm, chain, RPC, currency, disabled streaming, or a non-durable store

## 3. Discovery consistency

- [x] 3.1 Change the nested MPP directory entry from `GET /sessions/stream` to `POST /sessions/stream`
- [x] 3.2 Run the directory schema/tests and verify pricing and realm remain correct without changing review item 7 metadata

## 4. Verification and rollout

- [x] 4.1 Run Rust tests, OpenSpec validation, Terraform formatting/validation, and relevant client contract tests
- [x] 4.2 Enable or verify Secret Manager, create the named close-key secret without exposing its value, and review a non-destructive Terraform plan
- [x] 4.3 Build and apply the content-addressed daemon rollout one node at a time after confirming zero active sessions
- [x] 4.4 Implement/deploy the existing global registry aggregator change and restore `registry.tempvpn.xyz`
- [x] 4.5 Verify production TLS, closed legacy ingress, fixed and streaming challenge metadata, POST/GET/HEAD routing, OpenAPI lifecycle paths, pricing wording, realms, and node discovery
- [x] 4.6 Commit the main repository and nested directory repository locally without pushing `deploymaster`
