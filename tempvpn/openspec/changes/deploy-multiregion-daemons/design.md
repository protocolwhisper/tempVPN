## Context

The current Terraform configuration models one Compute Engine daemon. The MVP target is six Linux daemons across the Americas, Europe, and Asia, using two of those daemons as regional registry nodes. The remaining USD 273.87 promotional credit expires in August 2026, so the deployment must use on-demand resources, have an explicit cost guardrail, and avoid annual commitments.

The selected topology is:

| Node | Region | Role | Machine | Registry group |
|---|---|---|---|---|
| US East | `us-east1` | registry + exit | `e2-medium` | Americas |
| US West | `us-west2` | exit | `e2-small` | Americas |
| Sao Paulo | `southamerica-east1` | exit | `e2-small` | Americas |
| Belgium | `europe-west1` | registry + exit | `e2-medium` | Europe/Asia |
| Madrid | `europe-southwest1` | exit | `e2-small` | Europe/Asia |
| Singapore | `asia-southeast1` | exit | `e2-small` | Europe/Asia |

## Goals / Non-Goals

**Goals:**

- Define the complete six-node Linux daemon fleet with Terraform.
- Keep topology, roles, sizes, and location metadata reviewable as data.
- Route each exit daemon to exactly one of two registry nodes.
- Default to Standard Tier networking and 10 GB `pd-standard` disks.
- Keep credentials out of checked-in variable examples.
- Make one Terraform apply generate credentials, upload the verified daemon artifact privately, and boot usable services on all six nodes.
- Support plan-first deployment and explicit rollback.

**Non-Goals:**

- Applying Terraform or creating resources during implementation.
- Changing Swift, macOS, or Linux client behavior.
- Aggregating the two registry catalogs in a client.
- Adopting or deleting pre-existing GCP resources automatically.
- Adding DNS, managed TLS, load balancers, autoscaling, or committed-use discounts.

## Decisions

### Map-driven nodes

A `nodes` map is the source of truth for zone, subnet CIDR, machine type, location metadata, role, and registry group. Compute addresses, subnetworks, and instances use `for_each`, which keeps every resource keyed by a stable logical node name and avoids six copied resource blocks.

### Two independent registry groups

US East is the registry for US East, US West, and Sao Paulo. Belgium is the registry for Belgium, Madrid, and Singapore. Terraform derives each daemon's registry URL from the static address of the registry node in its group. Client selection between registries remains an operator/client configuration concern outside this deployment change.

### MVP cost profile

Registry nodes use `e2-medium`; exits use `e2-small`. Static Standard Tier IPv4 addresses and 10 GB `pd-standard` disks are used for all nodes. The estimated fixed infrastructure cost is approximately USD 103 through the August credit expiry window, or approximately USD 136 for a 30-day month, before traffic and taxes. The USD 273.87 credit is a fleet-wide ceiling, not a per-node budget.

### Secret handling

Terraform `random_password` resources generate one admin token per node and one registry-write token per registry group. Sensitive outputs provide an explicit operator recovery path without printing values during normal plan/apply output. Compute instance metadata/startup templates and random resources cause those values to exist in Terraform state, so the state backend must be treated as sensitive. Distinct generated tokens limit credential reuse. Optional caller-supplied credentials are not needed for the MVP deployment.

Terraform also generates a distinct MPP challenge-signing key for each daemon and injects it only through the systemd service environment. The payment recipient is a public EVM address and requires no wallet private key on any node. The live rollout uses `0x84626d5163CA4e07e7082FAe942f7b0754fC8b0A` as the explicit recipient.

### Capacity fallback

The initial `us-west1` placement was unavailable for `e2-small` in all three zones during rollout. The US West exit therefore runs in Los Angeles (`us-west2-a`) with the same machine size, role, registry group, and cost-conscious profile. Sao Paulo moved from `southamerica-east1-b` to `southamerica-east1-a` for the same capacity reason; its region and topology role did not change.

### Private artifact delivery

The deployment consumes the verified `x86_64-unknown-linux-musl` daemon binary from the local workspace. Terraform uploads it under a SHA-256-addressed object name in a private Cloud Storage bucket. A dedicated fleet service account receives bucket-level object-viewer access and no object-write permission. At boot, the startup script retrieves a short-lived OAuth token from the Compute Engine metadata service and uses it to download the object through the authenticated Cloud Storage API before starting systemd. This avoids public binaries, signed-URL expiry, and six manual SCP operations.

### Existing resources remain separate

The stopped legacy instances, their disks and snapshots, and the existing reserved address are not imported implicitly. The runbook requires an explicit operator choice to import a matching resource or create the new named fleet. No destructive cleanup is part of this change.

### Independent node power control

A `stopped_nodes` set contains logical node keys whose Compute Engine `desired_status` is `TERMINATED`; every other instance declares `RUNNING`. Changing the set updates only the selected instance status and preserves its boot disk, static IP, and Terraform address. The default set is empty. Operators should drain new sessions before stopping an exit, and should understand that stopping `us-east` or `belgium` temporarily removes the registry for its entire three-node group.

### Public API security boundary

The daemon API and WireGuard UDP port require public ingress for the MVP. Administrative SSH ingress remains configurable and should be restricted to operator CIDRs. Plain HTTP may be used only for a controlled soft rollout; production payment/session traffic requires a DNS and TLS termination plan before public user traffic.

## Risks / Trade-offs

- Cross-region registry heartbeats and catalog access add small egress and latency; two groups limit that path length.
- Two registries are separate catalogs; users pointed at one registry see only its three-node group unless client behavior changes later.
- Static public IPv4 addresses add hourly cost but give stable endpoints for registry membership.
- Terraform state contains startup metadata with tokens; insecure local or remote state storage can expose them.
- The deployment depends on the verified local daemon artifact existing before plan/apply; Terraform fails early when it is missing.
- The private artifact bucket and IAM binding add a few zero/near-zero-cost resources and must remain until nodes no longer need to reboot from scratch.
- Independently stopping a registry node leaves its group's exit VMs running but unable to refresh or serve a current catalog through that registry.
- Six tiny VMs favor geographic coverage over redundancy within each location.
- Traffic can exceed the fixed-cost estimate, especially through Sao Paulo and Singapore; billing alerts and budget checks remain required.

## Migration Plan

1. Build and verify the static `x86_64-unknown-linux-musl` daemon artifact locally.
2. Decide explicitly whether any existing GCP resource should be imported; otherwise leave legacy resources untouched.
3. Review the full Terraform plan, including generated secrets, private artifact storage, IAM, and six daemon VMs.
4. Apply registry nodes first and verify automatic artifact installation, health, public reachability, WireGuard, and registry mode.
5. Apply exit nodes by registry group and verify their registry heartbeats and catalog presence.
6. Run end-to-end daemon/session checks before directing MVP users to the endpoints.
7. Roll back by targeting newly created fleet resources only; do not remove legacy resources.

## Open Questions

- Which operator CIDRs should be allowed to SSH to the fleet?
- Which DNS names and TLS termination approach will be used before public user traffic?
