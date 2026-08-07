## Context

See `proposal.md` for motivation and
`specs/client-connection/spec.md` for the behavior contract.

The unsigned build path deliberately passes
`CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO` to Xcode and produces a
standalone SwiftPM controller without production entitlements. These artifacts
are useful for compilation and unit tests, but macOS will not grant them the
shared-Keychain and Packet Tunnel capabilities used by the real product.

The installer currently performs strict signature verification, while
`tempvpnctl connect` checks only that a host bundle containing the expected
extension path exists. The next failure may therefore occur when the controller
looks up its shared Keychain group or asks Network Extension to start. Payment is
outside `tempvpnctl`, so the agent workflow must run readiness before invoking
`mppx`; connect-time enforcement is a second line of defense for direct callers.

## Goals / Non-Goals

**Goals:**

- Determine native macOS product readiness without network access, payment,
  session mutation, Keychain key creation, or VPN profile changes.
- Give people actionable diagnostics and agents stable JSON reason codes.
- Reuse one readiness policy in the diagnostic command and connect path.
- Validate effective signatures and entitlements rather than trusting build
  environment variables or source entitlement files.
- Keep unsigned builds useful for compilation and unit testing while making
  their runtime boundary unmistakable.

**Non-Goals:**

- Bypassing Apple signing, provisioning, Network Extension approval, hardened
  runtime, or notarization requirements.
- Automatically obtaining certificates, changing Apple Developer configuration,
  or signing artifacts on the user's behalf.
- Falling back to external WireGuard tools on macOS.
- Proving that the user has already approved the VPN profile; first connection
  may still trigger the normal macOS approval flow.
- Changing Tempo MPP, server APIs, session accounting, WireGuard routing, Linux
  behavior, or the ownership of private keys.

## Decisions

### 1. Add `tempvpnctl doctor` as the readiness interface

The controller will expose:

```text
tempvpnctl doctor [--json]
```

The existing `TEMPVPN_HOST_APP_PATH` test/development override will remain the
way to inspect a non-default host bundle. The command will inspect the running
controller plus the discovered host app and its embedded extension. It will
return exit code zero only for a fully ready set.

JSON output will contain an overall `ready` boolean and a list of checks with
stable codes, component names, pass/fail state, and safe remediation text.
Initial codes will cover:

- `cli-signature`
- `host-app-present`
- `host-app-signature`
- `packet-tunnel-present`
- `packet-tunnel-signature`
- `team-alignment`
- `network-extension-entitlement`
- `shared-keychain-entitlement`

The human form will render the same model rather than maintaining separate
decision logic.

**Alternative considered:** infer readiness from the presence of the app and
build-time environment variables. Rejected because artifacts can be copied,
re-signed, partially signed, or paired with a controller from another team after
the build completes.

### 2. Inspect effective code-signing information through macOS Security APIs

The controller will inspect itself as running code and the app/extension as
static code. Each component must:

- pass validity checking;
- expose a non-empty Apple team identifier, which excludes unsigned and ad-hoc
  identities from production readiness;
- share the same team identifier;
- expose the expected effective Network Extension entitlement where required;
- expose a common effective Keychain access group ending in
  `com.protocolwhisper.tempvpn.shared` and belonging to that team.

The host and extension bundle identifiers will also be checked against the
TempVPN identifiers so an unrelated signed extension cannot satisfy readiness.
Checks use the entitlements embedded in the final signatures, not the plist
templates in source control.

Signature inspection will be behind a small injectable interface. Unit tests
can supply deterministic signing records without requiring an Apple certificate
on the test machine.

**Alternative considered:** execute `/usr/bin/codesign` from `tempvpnctl`.
Rejected because parsing command output is less stable, complicates structured
errors, and duplicates information available through Security.framework.

### 3. Run connect preflight before private-key or session operations

`tempvpnctl connect` will decode command-line options, discover the host
application, and run the same readiness evaluator before:

- loading or creating the session's Keychain private key;
- calling `/sessions/{id}/connect`;
- saving a Network Extension profile.

If preflight fails, connect returns the readiness failure and leaves an already
purchased session in its initial paused state. No compensating pause call is
needed because activation never occurred.

**Alternative considered:** keep the current late Keychain/Network Extension
errors. Rejected because they are incomplete diagnostics and can activate a
paid session before discovering that local tunnel setup is impossible.

### 4. Make pre-payment enforcement part of the agent contract

The TempVPN skill will check `tempvpnctl doctor --json` before node payment on
macOS. A non-ready result terminates the paid workflow with remediation guidance.
The check is macOS-only; Linux retains its existing `wg` and `wg-quick`
prerequisite checks.

The controller cannot itself guarantee pre-payment ordering because payment is a
separate `mppx` process. The agent contract is therefore the primary prevention,
and connect preflight limits damage for direct callers to an unused paused
balance.

### 5. Preserve strict installation and improve its diagnostics

The installer will continue to reject artifacts before copying them. Its checks
will distinguish absent components, invalid or ad-hoc signatures, team
mismatch, and missing effective entitlements. It will revalidate installed
destinations after copying before launching the host application.

The installer will not execute an unverified candidate controller as root.
Shell-side inspection may use system signing tools, but it will enforce the same
policy and stable terminology as `doctor`.

### 6. Keep secrets and lifecycle ownership unchanged

Readiness operates only on public bundle metadata, signatures, and entitlements.
It will not read Keychain values, payment accounts, session response contents,
private keys, admin tokens, registry tokens, or VPN configuration.

The client remains the sole owner of its WireGuard private key. The node remains
the owner of session state and tunnel peer allocation. Failed readiness creates
no new cleanup obligation because no session lifecycle operation occurs.

### 7. Test policy separately from real certificate availability

Swift unit tests will cover the readiness decision matrix with injected component
records: ready, unsigned, ad-hoc, partial, mismatched teams, missing
entitlements, incorrect identifiers, and missing app/extension.

Command tests will verify JSON shape, exit status, human diagnostics, and that
connect does not call key or network collaborators after preflight failure.
Shell tests will exercise installer rejection with disposable unsigned and
ad-hoc fixtures. A correctly Apple-signed end-to-end installation remains a
release/manual test because CI cannot assume access to developer identities or
provisioning.

## Risks / Trade-offs

- **Security.framework metadata differs across macOS or signature types** →
  Normalize optional fields into stable internal states and test the minimum
  supported macOS version plus current release runners.
- **A valid product is rejected because entitlement formatting differs** →
  Compare effective values semantically, including arrays and team-prefixed
  Keychain groups, rather than comparing serialized plists.
- **Readiness is mistaken for VPN approval or notarization assurance** →
  Name each check precisely and document that first-use approval and production
  notarization remain separate release/runtime concerns.
- **Artifacts change between `doctor` and connection** → `connect` repeats the
  full readiness evaluation immediately before touching key or session state.
- **Installer and Swift policy drift** → Maintain a shared checklist and
  cross-check both paths with the same fixture matrix and terminology.
- **Existing scripts relied on late, less-specific failures** → Keep command
  syntax compatible and make the new earlier failure actionable and
  machine-readable.

## Migration Plan

1. Add the readiness model, Security-framework inspector, and tests without
   changing the existing connect path.
2. Add `tempvpnctl doctor` and verify JSON/human output on unsigned development
   artifacts and a signed release candidate.
3. Gate `connect` with the readiness evaluator before Keychain and session work.
4. Strengthen installer diagnostics and destination revalidation.
5. Update build messages, macOS documentation, root documentation, and the
   TempVPN agent skill in the same release.
6. Exercise the signed install and first-connection flow manually before
   publishing.

Rollback may remove the new command and connect gate without server or data
migration. The installer must continue rejecting unsigned products, and the
documentation must continue to state that unsigned artifacts are compilation
and testing outputs only.
