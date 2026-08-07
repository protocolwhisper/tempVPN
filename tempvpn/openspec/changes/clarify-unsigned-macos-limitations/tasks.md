## 1. Readiness Policy

- [ ] 1.1 Add Codable readiness result, component-check, and stable reason-code models for human and JSON output.
- [ ] 1.2 Implement a pure readiness evaluator for component presence, valid Apple-team signatures, expected bundle identifiers, team alignment, Network Extension entitlement, and shared-Keychain entitlement.
- [ ] 1.3 Add Swift unit tests covering ready, unsigned, ad-hoc, partially signed, mismatched-team, wrong-identifier, missing-entitlement, missing-app, and missing-extension inputs.

## 2. macOS Signature Inspection

- [ ] 2.1 Add an injectable Security.framework inspector for the running controller and static host-app and Packet Tunnel code.
- [ ] 2.2 Normalize effective team identity, bundle identifier, signature validity, Network Extension entitlement, and Keychain access groups into the readiness evaluator's input model.
- [ ] 2.3 Ensure signing inspection reads no Keychain values, session files, VPN configurations, payment data, or private credentials, and add focused tests for absent or malformed signing metadata.

## 3. Controller Commands and Enforcement

- [ ] 3.1 Add `tempvpnctl doctor [--json]` command parsing, human rendering, JSON rendering, and success/failure exit behavior.
- [ ] 3.2 Reuse the readiness evaluator at the start of `tempvpnctl connect`, before private-key loading or creation, session activation, and Network Extension profile changes.
- [ ] 3.3 Add command tests that assert stable JSON codes and actionable unsigned, team-mismatch, entitlement, and missing-component messages.
- [ ] 3.4 Add tests proving failed connect preflight does not invoke Keychain mutation, session activation, pause, or tunnel installation collaborators.
- [ ] 3.5 Preserve and test the existing ready-product connection path, node-binding checks, failure compensation, and private-key ownership.

## 4. Build and Installation Boundaries

- [ ] 4.1 Update unsigned macOS build output to identify both products as compilation/test artifacts and list the unavailable installation, Keychain, extension-registration, and VPN capabilities.
- [ ] 4.2 Strengthen the installer to reject unsigned, ad-hoc, partially signed, mismatched-team, wrong-identifier, and missing-entitlement product sets before copying.
- [ ] 4.3 Revalidate the installed app, embedded extension, and controller after copying and before launching the host application.
- [ ] 4.4 Add disposable shell fixtures/tests for missing, unsigned, and ad-hoc artifacts and verify that installation performs no destination mutation on failure.

## 5. Agent Workflow and Documentation

- [ ] 5.1 Update `agent/SKILL.md` to run `tempvpnctl doctor --json` before macOS node payment and stop without an external-WireGuard or unsigned fallback when readiness fails.
- [ ] 5.2 Update `clients/macos/README.md` with the exact unsigned-build boundary, readiness output, Apple-team/entitlement requirements, first-use approval distinction, and signed workflow.
- [ ] 5.3 Update the repository README prerequisites and macOS examples to include readiness before payment.

## 6. Verification

- [ ] 6.1 Run the complete Swift test suite and unsigned macOS build, then verify `doctor` reports the development artifacts as not ready with stable reason codes.
- [ ] 6.2 Run installer rejection tests and confirm failed cases leave configured installation destinations unchanged.
- [ ] 6.3 Run the Rust workspace tests to confirm the unchanged node, registry, and Linux client contracts remain green.
- [ ] 6.4 Manually verify a correctly Apple-signed release candidate passes `doctor`, installs, receives normal first-use VPN approval, connects, heartbeats, reports status, and disconnects while preserving unused balance.
- [ ] 6.5 Run strict OpenSpec validation and reconcile the implemented behavior, delta spec, documentation, and task completion state before archive.
