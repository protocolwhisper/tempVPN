## 1. Terraform topology

- [x] 1.1 Add map-driven node topology, registry-group validation, and sensitive per-node/per-group token inputs.
- [x] 1.2 Convert subnetworks, static addresses, and Linux daemon instances to stable `for_each` resources.
- [x] 1.3 Derive each daemon's registry URL from its group's registry node and expose fleet/registry outputs.
- [x] 1.4 Default the six nodes to the selected regions, roles, machine sizes, Standard Tier networking, and 10 GB `pd-standard` disks.

## 2. Linux daemon bootstrap

- [x] 2.1 Pass per-node location, role, registry, network, and secret values into the existing Linux startup template.
- [x] 2.2 Keep daemon installation and systemd startup idempotent for plan-first staged rollout.

## 3. Operator documentation

- [x] 3.1 Document local-only planning and the explicit requirement for operator approval before `terraform apply`.
- [x] 3.2 Document the USD 273.87 fleet budget, approximate USD 103 expiry-window fixed cost, approximate USD 136 30-day fixed cost, and variable traffic risk.
- [x] 3.3 Document secret/state handling, SSH CIDR restriction, DNS/TLS prerequisites, existing-resource import decisions, staged rollout, and rollback.

## 4. Verification

- [x] 4.1 Run `terraform fmt -check` and `terraform validate` without applying resources.
- [x] 4.2 Generate and review a Terraform plan when credentials and secret values are available; do not apply it.
- [x] 4.3 Run strict OpenSpec validation and review the final local `deploymaster` diff for client-code changes.

## 5. Complete Terraform bootstrap

- [x] 5.1 Generate unique per-node admin credentials and per-group registry credentials with sensitive recovery outputs.
- [x] 5.2 Upload the verified static Linux daemon to a private content-addressed Cloud Storage object.
- [x] 5.3 Create a dedicated fleet service account with read-only artifact access and use metadata-service authentication during startup.
- [x] 5.4 Make every VM install and start the downloaded daemon automatically, failing visibly if artifact retrieval fails.
- [x] 5.5 Update the example and runbook for the single Terraform workflow and sensitive credential recovery.
- [x] 5.6 Add validated per-node stop/start control that preserves resources and leaves unselected instances running.
- [x] 5.7 Re-run format, validation, strict OpenSpec validation, artifact execution, and a zero-destroy Terraform plan.
