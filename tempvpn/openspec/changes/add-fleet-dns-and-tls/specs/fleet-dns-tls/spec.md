## Purpose

Provide every public tempVPN node API with a stable DNS identity and encrypted HTTPS transport while preserving node-bound payment and session behavior.

## ADDED Requirements

### Requirement: Stable per-node DNS identity
Each production node SHALL have one unique hostname under `tempvpn.xyz` whose address record resolves to that node's reserved public IPv4 address.

#### Scenario: Fleet DNS is inspected
- **WHEN** an operator resolves the configured hostnames for all six nodes
- **THEN** each hostname resolves to its assigned node's reserved public IPv4 address
- **AND** no hostname is shared by two nodes

#### Scenario: Node is replaced without changing identity
- **WHEN** infrastructure recreates a node while retaining its reserved public IPv4 address
- **THEN** its public hostname remains unchanged

### Requirement: HTTPS-only production API ingress
Each DNS-enabled production node SHALL serve its public health, payment, session, and lifecycle API through HTTPS with a certificate valid for that node's hostname. The daemon backend port SHALL not be directly reachable from the public internet after that node's HTTPS endpoint is verified.

#### Scenario: Client reaches a node API
- **WHEN** a client calls a DNS-enabled node's advertised API origin
- **THEN** TLS validation succeeds for the advertised hostname
- **AND** the request reaches the same node daemon selected by discovery

#### Scenario: Direct backend access is attempted
- **GIVEN** a node has completed HTTPS rollout
- **WHEN** a public caller connects directly to its daemon backend port
- **THEN** network ingress rejects the connection

#### Scenario: Certificate is not ready
- **WHEN** a node hostname does not yet resolve correctly or its certificate is not valid
- **THEN** that HTTPS origin is not advertised as ready for paid traffic
- **AND** rollout does not remove the previously working ingress path

### Requirement: Node-bound HTTPS lifecycle
Changing a node from an IP-based HTTP origin to its DNS-based HTTPS origin SHALL NOT change the selected-node binding, payment recipient, MPP challenge, session identifier, connected-time balance, or pause semantics.

#### Scenario: Session is purchased through a DNS origin
- **GIVEN** discovery selected `https://madrid.tempvpn.xyz`
- **WHEN** the client pays for and activates a session
- **THEN** every lifecycle request remains bound to `https://madrid.tempvpn.xyz`
- **AND** no registry or different node receives the payment or session import

#### Scenario: Existing session spans a rollout
- **GIVEN** a paid session was created through the previously advertised origin
- **WHEN** its node is migrated to HTTPS
- **THEN** the operator preserves a working lifecycle path until that session is terminal or the old origin is intentionally retired

### Requirement: Staged and reversible rollout
Operators SHALL be able to enable DNS and TLS per node, verify the resulting public endpoint, and roll back one node without changing WireGuard addressing or destroying the reserved public address.

#### Scenario: One node is enabled
- **WHEN** an operator enables TLS for one node during staged rollout
- **THEN** other nodes retain their current advertised origins and ingress behavior

#### Scenario: One node is rolled back
- **WHEN** HTTPS verification fails for a node
- **THEN** the operator can restore its prior API origin and backend ingress
- **AND** other nodes and the global registry remain unaffected
