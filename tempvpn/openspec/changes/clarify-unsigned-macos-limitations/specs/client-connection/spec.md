## ADDED Requirements

### Requirement: macOS product readiness

The macOS client SHALL provide a non-destructive readiness check that determines
whether the installed native products can create a TempVPN Network Extension
connection. Readiness SHALL require the controller, headless host application,
and embedded Packet Tunnel extension to have valid non-ad-hoc Apple-team
signatures, the same team identity, and the entitlements needed for Network
Extension and shared-Keychain operation.

#### Scenario: Signed product set is ready

- **GIVEN** the controller, installed host application, and embedded Packet Tunnel extension have valid signatures from the same Apple team
- **AND** their effective entitlements contain the required Network Extension capability and matching TempVPN shared-Keychain access group
- **WHEN** the readiness check runs
- **THEN** it reports that the macOS client is ready to connect
- **AND** does not create, activate, heartbeat, pause, or pay for a session

#### Scenario: Unsigned or ad-hoc-signed artifact is inspected

- **GIVEN** any required macOS product lacks an Apple-team signature
- **WHEN** the readiness check runs
- **THEN** it reports the product as not ready
- **AND** identifies signing as the failed prerequisite

#### Scenario: Product signatures belong to different teams

- **GIVEN** the required products are signed but do not share one Apple team identity
- **WHEN** the readiness check runs
- **THEN** it reports the product set as not ready
- **AND** identifies team alignment as the failed prerequisite

#### Scenario: Required entitlement is absent

- **GIVEN** a required product lacks its Network Extension entitlement or matching shared-Keychain access group
- **WHEN** the readiness check runs
- **THEN** it reports the product set as not ready
- **AND** identifies the missing or mismatched capability without exposing credential or key material

#### Scenario: Host app or extension is absent

- **GIVEN** the headless application is not installed or does not contain the expected Packet Tunnel extension
- **WHEN** the readiness check runs
- **THEN** it reports the product set as not ready
- **AND** identifies the missing installed component

### Requirement: Machine-readable readiness diagnostics

The macOS readiness command SHALL provide stable machine-readable output suitable
for agent decisions as well as an actionable human-readable form.

#### Scenario: Agent requests JSON output

- **WHEN** an agent requests readiness in JSON
- **THEN** the command returns an overall readiness boolean
- **AND** returns a result and stable reason code for each required component or capability
- **AND** exits successfully only when the complete product set is ready

#### Scenario: Readiness failure is displayed to a user

- **WHEN** the readiness command runs without JSON and a prerequisite fails
- **THEN** it explains that unsigned builds are compilation and test artifacts
- **AND** identifies the signing, entitlement, installation, or team-alignment action required before connection

### Requirement: Pre-payment macOS readiness

The supported macOS agent workflow MUST establish product readiness before
paying a VPN node. A failed readiness check SHALL stop the default connection
workflow before payment.

#### Scenario: Agent prepares a paid macOS connection

- **GIVEN** the user asks to buy and connect a TempVPN session on macOS
- **WHEN** the agent checks client prerequisites
- **THEN** it runs the non-destructive readiness command before invoking the MPP client
- **AND** proceeds to selection and payment only when readiness succeeds

#### Scenario: Readiness fails before payment

- **GIVEN** the macOS product set is not ready
- **WHEN** the agent prepares the paid connection
- **THEN** it stops before sending an MPP payment
- **AND** reports the failed prerequisite
- **AND** does not substitute external WireGuard tools or an unsigned fallback

### Requirement: Connect-time readiness enforcement

The macOS controller SHALL repeat readiness validation before touching the
session's private-key state or calling its activation endpoint. This check SHALL
protect direct callers that did not follow the agent workflow.

#### Scenario: Direct connect uses an unready product set

- **GIVEN** a caller already has a paid but paused session response
- **AND** the installed macOS product set is not ready
- **WHEN** the caller invokes `tempvpnctl connect`
- **THEN** the command fails before loading or creating the session private key
- **AND** does not call the session activation endpoint
- **AND** leaves the paid session paused with its unused balance

#### Scenario: Connect uses a ready product set

- **GIVEN** the installed macOS product set passes readiness validation
- **WHEN** the caller invokes `tempvpnctl connect` with a valid node-bound paid response
- **THEN** the existing key generation, session activation, and native tunnel workflow continues unchanged

### Requirement: Unsigned build boundary

Unsigned macOS output SHALL be treated only as a development artifact for
compilation and tests. It SHALL NOT be represented as installable or capable of
establishing the production TempVPN Network Extension tunnel.

#### Scenario: Developer builds without signing configuration

- **WHEN** the macOS build completes without an Apple development team and signing identity
- **THEN** the build output clearly identifies both products as unsigned development artifacts
- **AND** explains that installation, shared-Keychain access, extension registration, and VPN activation are unavailable

#### Scenario: Installer receives unsigned products

- **WHEN** installation is attempted with an unsigned, ad-hoc-signed, partially signed, or mismatched product set
- **THEN** the installer rejects the products before copying them into system locations
- **AND** reports the failed signing or entitlement prerequisite
