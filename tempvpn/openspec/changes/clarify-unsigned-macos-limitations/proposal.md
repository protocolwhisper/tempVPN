## Why

Unsigned macOS builds currently verify that the Swift and Network Extension
targets compile, but the resulting binaries can still look runnable even though
macOS will not grant the extension and shared-Keychain capabilities required to
connect. A user or agent can discover this only after selecting and paying a
node, so TempVPN needs a pre-payment readiness check and precise diagnostics that
separate build-only artifacts from installable products.

## What Changes

- Add a machine-readable macOS readiness command that checks the CLI, headless
  app, embedded Packet Tunnel extension, code signatures, team alignment,
  required entitlements, and system installation state without creating or
  activating a paid session.
- Make `tempvpnctl connect` run the same readiness checks and fail before session
  activation when the installed products cannot support the native tunnel.
- Classify unsigned outputs explicitly as compilation/test artifacts: they may
  run non-privileged development operations, but they cannot be installed,
  access the production shared-Keychain group, register the Packet Tunnel
  extension, or establish a TempVPN tunnel.
- Update build output, installer diagnostics, macOS documentation, and the
  TempVPN agent workflow so readiness is checked before payment.
- Add Swift and shell-level tests for unsigned, partially signed, mismatched-team,
  missing-entitlement, and correctly signed states.
- Preserve the existing signed production workflow and provide no unsigned,
  ad-hoc-signing, external-WireGuard, or entitlement-bypass fallback.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `client-connection`: Require macOS signing and entitlement readiness to be
  observable and validated before payment or session activation, while defining
  the supported scope of unsigned build artifacts.

## Impact

- **macOS client:** `tempvpnctl` command routing, signing/readiness inspection,
  structured errors, host-app discovery, and tests.
- **Build and installation:** macOS build scripts, installer validation messages,
  and unsigned-output labeling.
- **Agent workflow and documentation:** pre-payment checks in `agent/SKILL.md`,
  `clients/macos/README.md`, and the repository README.
- **Unaffected surfaces:** the Rust node daemon, registry, Linux client,
  Terraform infrastructure, MPP protocol, session-expiry rules, server
  credentials, WireGuard configuration, and network-routing behavior.
- **Compatibility:** correctly signed products continue to connect as before.
  Automation gains a readiness command and structured failure output; no server
  API or configuration migration is required.
- **Rollback:** the readiness command and connect preflight can be reverted
  independently of server or protocol code. Documentation must remain explicit
  that unsigned Network Extension products are unsupported even if the command
  is rolled back.
