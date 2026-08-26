## Context

See `proposal.md` for motivation. The six VMs download one content-addressed daemon binary and render a TOML file from instance metadata. That template currently omits most MPP settings, so the daemon falls back to Moderato, `localhost:8080`, disabled streaming, and in-memory state. Fixed-session stale cleanup also updates billing state without removing the corresponding WireGuard peer.

## Goals / Non-Goals

**Goals:**

- Make network authorization end no later than metering for both explicit and automatic pause.
- Make production payment identity explicit and identical across fixed and Session v2 challenges.
- Keep all private signing material server-side and out of Git, command output, and public instance metadata.
- Roll out a verifiable immutable daemon artifact to all six existing VMs without replacing addresses or disks.

**Non-Goals:**

- Landing-page and directory metadata polish from review item 7.
- Proxying paid node traffic through the global registry.
- Changing Linux or macOS WireGuard key ownership or request formats.

## Decisions

### Automatic pause performs the same peer cleanup as explicit pause

The in-memory cleanup sweep will collect the public keys of sessions it stale-pauses, clear those bindings, and remove each peer after releasing the session-store lock. A focused mock-WireGuard test will prove the peer is absent after cleanup. Keeping the command outside the lock avoids blocking unrelated session operations on the external `wg` process.

If peer removal fails, cleanup logs the error and retries on subsequent sweeps by retaining enough cleanup state; metering must not silently remain stopped forever while the peer stays authorized. An alternative of inspecting WireGuard traffic was rejected because traffic does not prove that the paying client is alive and would change the documented heartbeat contract.

### Production configuration is rendered explicitly

Terraform will declare and pass RPC URL, realm, currency, chain ID, Session v2 reserve/operator/pricing, production mode, and durable SQLite path into the generated TOML. Production validation in the daemon remains the final fail-closed boundary. This avoids changing safe development defaults while preventing production from inheriting them accidentally.

### The close key is fetched at boot from Secret Manager

Terraform enables Secret Manager and grants the fleet service account accessor permission only on the named close-key secret. The startup script uses its metadata-service access token to fetch the latest secret version into a root-owned environment file; neither the key nor its value is rendered into instance metadata or Terraform state. `MPP_SECRET_KEY` remains Terraform-generated as it is today; migrating that existing secret is separate from the reported issue.

For this rollout, the operator explicitly chose the existing payment-recipient account as the Session v2 close signer, so the secret derives to `0x59E5aa2A081FB9F56FE9ae57b7688A5884d74dDC` and `mpp_session_operator` equals `mpp_payment_recipient`. This avoids funding a second fee-paying account but means compromise of any authorized node could expose the receiving account. A later rotation should move settlement authority to a separately funded operator key without changing the payment recipient.

The Session v2 SQLite database lives under `/var/lib/vpn-node-daemon`, owned by the daemon user and explicitly writable by systemd. macOS continues its 30-second client heartbeat loop. The Linux CLI retains its explicit heartbeat command; no client protocol changes are required for this server-side correction.

### Runtime realm is the deliberate shared protection space

All node challenges use `tempvpn.xyz`, matching the directory entry and allowing the operator to describe one service realm while node URLs remain distinct HTTPS origins. The runtime challenge remains authoritative. Per-node hostname realms were considered but would not fit the directory's single realm field and are unnecessary because TLS still authenticates the selected node origin.

## Risks / Trade-offs

- **[A failed WireGuard removal could outlive billing state]** → retain retryable cleanup state, log prominently, and cover the failure path.
- **[A startup reset interrupts active tunnels]** → verify no active sessions, deploy registry nodes first, and roll through exits individually.
- **[Secret Manager is currently disabled]** → enable it explicitly, create or verify the named secret before applying VM metadata, and never print its value.
- **[The temporary close signer is also the payment recipient]** → restrict Secret Manager access to the fleet identity, retain no local copy, monitor the account, and rotate to a dedicated operator after this compatibility rollout.
- **[Global registry has no current Cloud Run deployment]** → implement and deploy the existing `add-global-registry-aggregator` change independently, verify its default URL, then repair DNS.
- **[Directory repository is separately versioned]** → commit its POST correction on its existing branch and do not push without explicit publication authority.

## Migration Plan

1. Add tests and implement stale-peer cleanup and explicit production configuration.
2. Enable Secret Manager, create or verify the close-key secret, and grant least-privilege access.
3. Build a static Linux artifact and produce a reviewed Terraform plan containing no instance replacement or destructive action.
4. Apply infrastructure changes and reset one node at a time, verifying health, challenge metadata, method routing, TLS, and closed backend ingress.
5. Deploy the existing global aggregator change and verify `/nodes`, `/openapi.json`, `/llms.txt`, and `/docs`.
6. Correct and validate the nested directory entry.

Rollback restores the previous artifact hash and startup configuration, then resets only the affected VM. Static IPs, disks, paid-session data, and WireGuard addressing are not destroyed. Rollback of the payment configuration is emergency-only because it restores the reviewed incompatibilities.
