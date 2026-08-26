## Purpose

Define a reproducible, cost-controlled Google Cloud topology for six temporary VPN exit daemons split between independent Americas and Europe/Asia registry groups.

## ADDED Requirements

### Requirement: Six-node regional topology

The deployment configuration SHALL define two US nodes, two European nodes, one Asian node, and one Latin American node, and SHALL assign every node to exactly one registry group.

#### Scenario: Americas group is planned

- **WHEN** the default MVP topology is evaluated
- **THEN** the US East node runs the Americas registry and advertises itself as an exit node
- **AND** the US West and Latin American nodes register only with that Americas registry

#### Scenario: Europe and Asia group is planned

- **WHEN** the default MVP topology is evaluated
- **THEN** the Belgium node runs the Europe/Asia registry and advertises itself as an exit node
- **AND** the second European node and Asian node register only with that Europe/Asia registry

### Requirement: Reversible on-demand capacity

The MVP deployment SHALL use on-demand general-purpose instances without Spot capacity or one-year or three-year commitments.

#### Scenario: Default machine sizes are planned

- **WHEN** the six-node topology uses its default sizing
- **THEN** the two registry/exit nodes use `e2-medium`
- **AND** the four exit-only nodes use `e2-small`

#### Scenario: A node needs more capacity

- **WHEN** an operator changes an individual node's configured machine type
- **THEN** the deployment updates only that node's compute shape
- **AND** preserves its stable node identity and external address

### Requirement: Cost-conscious network and storage defaults

The deployment SHALL default to Standard Tier external networking and minimum practical standard persistent boot disks while keeping every WireGuard and node API endpoint directly reachable.

#### Scenario: Default infrastructure is planned

- **WHEN** the topology is rendered with default values
- **THEN** every node has a regional Standard Tier external IPv4 address
- **AND** every node has a 10 GB standard persistent boot disk
- **AND** no load balancer, Cloud NAT gateway, GPU, or premium operating-system image is created

### Requirement: Registry and administrative secret separation

Each registry group SHALL use a server-only registry-write token, and every registry-write token SHALL remain distinct from daemon-admin credentials.

#### Scenario: Registry group configuration is generated

- **WHEN** node startup configuration is rendered
- **THEN** each node receives only the registry-write token for its assigned group
- **AND** clients do not receive registry-write or daemon-admin tokens

#### Scenario: Secrets are generated during infrastructure deployment

- **WHEN** Terraform creates the daemon fleet
- **THEN** it generates a distinct administrative token for every node and a distinct registry-write token for every registry group
- **AND** generated values are available only through sensitive outputs and protected Terraform state
- **AND** the deployment documentation warns that infrastructure state contains sensitive values

### Requirement: Automated private daemon delivery

The deployment SHALL install the verified Linux daemon artifact on every VM without requiring a public artifact URL or manual file copy.

#### Scenario: Artifact infrastructure is planned

- **WHEN** Terraform evaluates the deployment with a valid local Linux daemon binary
- **THEN** it plans a private Cloud Storage bucket and a content-addressed daemon object
- **AND** it plans a dedicated VM service account with read-only access to that bucket

#### Scenario: A daemon VM starts

- **WHEN** the startup script runs on a fleet VM
- **THEN** it obtains a short-lived access token from the Compute Engine metadata service
- **AND** downloads the private daemon object using that identity
- **AND** installs and starts the `vpn-node-daemon` systemd service automatically

### Requirement: Non-destructive planning and rollback

Operators SHALL be able to validate and plan the topology without creating cloud resources, and rollback SHALL not migrate or delete paid sessions implicitly.

#### Scenario: Soft deployment is requested

- **WHEN** the operator runs initialization, formatting, validation, and planning only
- **THEN** no Compute Engine resource is created or changed

#### Scenario: Deployment is rolled back

- **WHEN** the operator removes the multi-region infrastructure after draining new sessions
- **THEN** existing paid sessions are allowed to pause or expire according to the existing lifecycle
- **AND** rollback does not invoke administrative session deletion as a migration mechanism

### Requirement: Independent node power control

Operators SHALL be able to stop and restart a selected node through Terraform without destroying its disk or static address and without changing the desired status of unselected nodes.

#### Scenario: One exit node is stopped

- **WHEN** an operator adds `madrid` to the configured stopped-node set and applies the reviewed plan
- **THEN** the Madrid instance desired status becomes `TERMINATED`
- **AND** the other five instances remain `RUNNING`
- **AND** the Madrid disk, static address, and Terraform identity are preserved

#### Scenario: A stopped node is restarted

- **WHEN** an operator removes a node key from the configured stopped-node set and applies the reviewed plan
- **THEN** only that instance returns to `RUNNING`
- **AND** its existing Terraform-managed identity and endpoint are retained
