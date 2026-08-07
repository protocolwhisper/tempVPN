## MODIFIED Requirements

### Requirement: Paid response validation

The client SHALL fail closed when a paid or activated response belongs to a different logical node or lacks fields needed to construct the tunnel. A generation-specific WireGuard key or endpoint change SHALL NOT be treated as a logical-node mismatch.

#### Scenario: Paid response matches selection
- **GIVEN** a paid response from the selected logical node
- **WHEN** the client imports it
- **THEN** it activates the session through that logical node's stable API URL
- **AND** accepts the activated response only when its logical node URL still matches
- **AND** requires an assigned tunnel address, server public key, and endpoint

#### Scenario: Activated response claims another logical node
- **WHEN** the activation response identifies a logical node other than the paid node
- **THEN** the client attempts to pause the paid session
- **AND** rejects the response

#### Scenario: Activated response lacks an address
- **WHEN** the activation response lacks a non-empty assigned tunnel address
- **THEN** the client attempts to pause the paid session
- **AND** does not start the local tunnel

## ADDED Requirements

### Requirement: Generation-aware paused resume

Linux and macOS clients SHALL build every resumed tunnel from the current activation response rather than assuming the previous generation's WireGuard metadata remains valid.

#### Scenario: Linux resumes after promotion
- **WHEN** a paused Linux session receives a new server public key or endpoint
- **THEN** Linux renders and starts the tunnel with the new values
- **AND** retains the stable logical node URL for lifecycle calls

#### Scenario: macOS resumes after promotion
- **WHEN** a paused macOS session receives a new server public key or endpoint
- **THEN** the Network Extension configuration uses the new values
- **AND** the client keeps its private key in the local Keychain
