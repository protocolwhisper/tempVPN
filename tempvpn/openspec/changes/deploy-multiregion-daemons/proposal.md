## Why

The MVP needs a geographically useful Linux daemon fleet before the Google Cloud Free Trial credit expires. The complete six-node deployment must remain within the available USD 273.87 credit, avoid commitments, and be reproducible without changing client applications.

## What Changes

- Replace the single-instance Terraform shape with a map-driven six-node deployment.
- Deploy two `e2-medium` registry-and-exit nodes in US East and Belgium.
- Deploy four `e2-small` exit nodes in US West, Sao Paulo, Madrid, and Singapore.
- Create two independent registry groups: Americas and Europe/Asia.
- Use Standard Tier external networking, 10 GB `pd-standard` boot disks, and per-node subnets.
- Build the Linux daemon before deployment and upload the verified artifact to a private Terraform-managed Cloud Storage bucket.
- Create a dedicated VM service account that can read only the deployment artifact.
- Generate distinct daemon-admin and registry-write credentials in Terraform and expose them only through sensitive outputs.
- Configure, install, and start each Linux daemon automatically with its location, registry relationship, and generated secrets.
- Allow operators to stop or restart any individual node through Terraform without destroying it or changing the status of other nodes.
- Document a plan-first rollout, existing-resource import decisions, security prerequisites, cost guardrails, and rollback.

No GCP resources are created by this change unless an operator explicitly runs `terraform apply`.

## Capabilities

### New Capabilities

- `multi-region-deployment`: Reproducible Terraform topology for six Linux VPN daemons split across two registry groups.

### Modified Capabilities

None.

## Impact

- `infra/terraform`: topology inputs, Compute Engine resources, private artifact storage, service-account IAM, generated secrets, startup template, sensitive outputs, examples, and operator documentation.
- GCP project `platypus-497309` only when an operator later applies the reviewed plan.
- Existing stopped VMs, disks, snapshots, and reserved addresses are not automatically adopted or deleted.
- No macOS, Swift, Linux client, registry protocol, or daemon application-code changes are in scope.
