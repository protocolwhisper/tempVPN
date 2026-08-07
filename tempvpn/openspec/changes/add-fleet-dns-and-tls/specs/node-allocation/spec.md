## MODIFIED Requirements

### Requirement: Leased node registration

A registry-mode daemon SHALL accept node advertisements only through the authenticated registry-write endpoint. The registry credential SHALL be separate from the daemon-admin credential. A DNS-enabled production node SHALL advertise its stable HTTPS API origin; an explicitly configured development or rollback node MAY advertise an HTTP origin.

#### Scenario: Node registers a valid lease

- **GIVEN** registry mode is enabled
- **AND** the caller supplies the configured registry token
- **WHEN** a node advertisement has an identifier matching the URL path
- **AND** the identifier contains only letters, digits, `.`, `_`, or `-`
- **THEN** the registry stores or replaces that node record
- **AND** assigns a lease expiration based on the configured lease duration
- **AND** normalizes a trailing slash from the advertised API URL

#### Scenario: DNS-enabled production node registers

- **GIVEN** a production node has completed DNS and TLS verification
- **WHEN** it refreshes its registry lease
- **THEN** its advertised API URL uses its unique `https://<node>.tempvpn.xyz` origin
- **AND** health, payment, activation, heartbeat, status, and pause calls resolve to that same node

#### Scenario: Invalid advertisement is rejected

- **WHEN** the advertised identifier differs from the URL path or contains unsupported characters
- **THEN** the registry rejects the advertisement without changing the catalog

#### Scenario: Registry write is unauthorized

- **WHEN** a caller registers or removes a node without the configured registry token
- **THEN** the registry rejects the request as unauthorized
