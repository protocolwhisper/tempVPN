# Client Connection Specification

## Purpose

Define the supported Linux and macOS client contract for importing a paid
session, handling WireGuard keys, establishing and verifying a tunnel,
maintaining status, and disconnecting safely.

## Requirements

### Requirement: Platform-specific connection path

The client workflow SHALL use the designated networking implementation for each
supported platform and SHALL stop before payment on an unsupported platform.

#### Scenario: Linux client connects

- **GIVEN** a Linux host with `vpn-client`, `wg`, and `wg-quick`
- **WHEN** the user connects a paid session
- **THEN** the Rust client manages the local WireGuard interface through `wg` and `wg-quick`

#### Scenario: macOS client connects

- **GIVEN** the signed `tempvpnctl`, invisible host app, Packet Tunnel extension, and WireGuardKit are installed
- **WHEN** the user connects a paid session on macOS
- **THEN** `tempvpnctl` uses the native Network Extension path
- **AND** does not fall back to external `wg` or `wg-quick` commands

#### Scenario: Unsupported platform is requested

- **GIVEN** a Windows host or another unsupported client platform
- **WHEN** a paid connection is requested
- **THEN** the workflow stops before purchasing a session

### Requirement: Client-owned private keys

Each client SHALL generate or load the WireGuard private key locally and SHALL
send only the derived public key to the node.

#### Scenario: Linux generates connection credentials

- **WHEN** the Linux client creates a new session itself
- **THEN** it generates a local ephemeral WireGuard keypair
- **AND** sends only the public key during activation
- **AND** writes a generated connection configuration with owner-only permissions

#### Scenario: Linux imports a paid response

- **GIVEN** the Linux client imports a paid session response
- **WHEN** it prepares activation
- **THEN** it requires a local private-key path
- **AND** derives the public key locally
- **AND** does not include the private key in the activation request

#### Scenario: macOS creates session credentials

- **WHEN** macOS imports a paid session for the first time
- **THEN** it generates a Curve25519 private key in the shared app Keychain group
- **AND** marks the key as available only after first unlock and only on that device
- **AND** stores and retrieves it by session identifier
- **AND** sends only its public key to the node

### Requirement: Paid response validation

The client SHALL fail closed when a paid or activated response belongs to a
different logical node or lacks fields needed to construct the tunnel. A
generation-specific WireGuard key or endpoint change SHALL NOT be treated as a
logical-node mismatch.

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

### Requirement: WireGuard tunnel configuration

The client SHALL construct its tunnel from server-returned connection metadata
and SHALL route only according to the client mode's configured allowed ranges.

#### Scenario: Normal full-tunnel connection

- **GIVEN** a successful activation response
- **WHEN** the client builds its normal WireGuard configuration
- **THEN** the interface uses the assigned peer address
- **AND** the peer uses the returned server public key and endpoint
- **AND** persistent keepalive is 25 seconds
- **AND** the normal connection routes IPv4 default traffic through the peer

#### Scenario: macOS full-tunnel connection

- **WHEN** macOS builds the native tunnel configuration
- **THEN** it routes both IPv4 and IPv6 default traffic through the peer
- **AND** resolves the private-key placeholder from the shared Keychain only inside the Packet Tunnel extension

#### Scenario: Linux uses custom allowed ranges

- **GIVEN** the Linux `connect` or configuration command is passed explicit allowed IP ranges
- **WHEN** it renders the configuration
- **THEN** those ranges replace the default IPv4 full-tunnel range

### Requirement: Generation-aware paused resume

Linux and macOS clients SHALL build every resumed tunnel from the current
activation response rather than assuming the previous generation's WireGuard
metadata remains valid.

#### Scenario: Linux resumes after promotion

- **WHEN** a paused Linux session receives a new server public key or endpoint
- **THEN** Linux renders and starts the tunnel with the new values
- **AND** retains the stable logical node URL for lifecycle calls

#### Scenario: macOS resumes after promotion

- **WHEN** a paused macOS session receives a new server public key or endpoint
- **THEN** the Network Extension configuration uses the new values
- **AND** the client keeps its private key in the local Keychain

### Requirement: Compensating cleanup on connection failure

A client SHALL avoid leaving a paid session actively consuming time when local
tunnel establishment fails.

#### Scenario: Linux cannot start or confirm the interface

- **WHEN** `wg-quick` fails or the requested interface is not active after startup
- **THEN** the client attempts to bring down any partial tunnel
- **AND** removes the generated configuration
- **AND** attempts to pause the server session
- **AND** reports the connection failure

#### Scenario: macOS cannot start the Network Extension tunnel

- **WHEN** the native tunnel cannot be installed or started
- **THEN** `tempvpnctl` attempts to pause the server session
- **AND** reports the connection failure

### Requirement: Connection verification

Tunnel establishment and exit-IP verification SHALL be reported as distinct
outcomes. A workflow SHALL NOT claim that VPN routing is verified unless the
observed public IP matches the selected node's expected exit IP.

#### Scenario: Expected exit IP is observed

- **GIVEN** the local tunnel is active
- **WHEN** a public-IP lookup through the intended route returns the node's expected exit IP
- **THEN** the workflow may report the VPN connection as verified

#### Scenario: Exit lookup is unavailable

- **GIVEN** the local tunnel is active
- **WHEN** DNS or the external public-IP service is temporarily unavailable
- **THEN** the client may leave the tunnel established
- **AND** reports exit verification as unavailable rather than successful

#### Scenario: Unexpected exit IP is observed

- **GIVEN** the local tunnel is active
- **WHEN** the observed public IP differs from the expected exit IP
- **THEN** the workflow reports the mismatch
- **AND** does not claim that traffic is verified through the selected VPN node

### Requirement: Local connection status

The client SHALL retain enough non-secret local state to inspect and disconnect
the active tunnel without introducing daemon-admin credentials.

#### Scenario: Linux records a connection

- **WHEN** Linux establishes a persistent connection
- **THEN** it records the session ID, node URL, interface, configuration path, tunnel address, observed exit IP when available, remaining seconds, and grace deadline

#### Scenario: macOS records a connection

- **WHEN** macOS installs the native tunnel
- **THEN** it stores the session ID, node URL, assigned address, expected exit IP, remaining seconds, and grace deadline in the Network Extension configuration
- **AND** stores the private key separately in the shared Keychain

#### Scenario: Status service is unavailable

- **GIVEN** the local tunnel configuration is available
- **WHEN** the node's public session-status request fails
- **THEN** the client reports the local tunnel state
- **AND** marks server state unavailable or uses saved non-secret session metadata

### Requirement: Connection heartbeat behavior

An active client SHALL support the node heartbeat contract and SHALL treat an
explicit inactive response as loss of session authorization.

#### Scenario: macOS tunnel remains active

- **GIVEN** the Packet Tunnel is running
- **WHEN** its 30-second heartbeat succeeds with state `active`
- **THEN** it resets the consecutive-failure count and keeps the tunnel running

#### Scenario: macOS session becomes inactive

- **WHEN** a heartbeat returns a client error or a state other than `active`
- **THEN** the Packet Tunnel cancels the connection

#### Scenario: macOS node is repeatedly unavailable

- **WHEN** three consecutive heartbeat attempts are unavailable
- **THEN** the Packet Tunnel cancels the connection

#### Scenario: Linux persistent connection refreshes status

- **GIVEN** a Linux connection and its local status record
- **WHEN** the caller invokes `vpn-client heartbeat`
- **THEN** the client heartbeats the recorded node-bound session
- **AND** updates the saved remaining seconds and grace deadline

### Requirement: Safe disconnect

Disconnect SHALL tear down local VPN routing and pause the server-side usage
balance. It SHALL NOT use the daemon's administrative deletion endpoint.

#### Scenario: Linux disconnects

- **GIVEN** a recorded Linux connection
- **WHEN** the user disconnects
- **THEN** the client brings down the recorded WireGuard configuration
- **AND** removes its generated configuration when applicable
- **AND** pauses the session on its recorded node
- **AND** removes local status after successful teardown and pause

#### Scenario: macOS disconnects

- **GIVEN** the native TempVPN profile
- **WHEN** the user runs `tempvpnctl disconnect`
- **THEN** the controller stops the Packet Tunnel
- **AND** the extension attempts to pause the node-bound session
- **AND** the controller repeats the pause idempotently after the tunnel stops
- **AND** reports whether the balance is paused or expired

#### Scenario: Paid balance remains

- **GIVEN** a disconnect leaves unused connected time before the grace deadline
- **WHEN** the server confirms the pause
- **THEN** the unused balance remains available for reconnection

### Requirement: Scoped run mode

The Linux `run` mode SHALL scope VPN access to its child-process workflow and
clean up the associated local and server resources when that workflow ends.

#### Scenario: Run mode starts

- **WHEN** Linux starts a command through `vpn-client run`
- **THEN** it starts the WireGuard tunnel
- **AND** binds its SOCKS5 proxy only to the configured loopback address
- **AND** passes proxy settings to the child process

#### Scenario: Tunnel or proxy fails

- **WHEN** the run-mode tunnel or proxy becomes unavailable
- **THEN** the kill switch stops the child process

#### Scenario: Run mode ends

- **WHEN** the child exits, the kill switch fires, the workflow is interrupted, or setup fails
- **THEN** the client removes local status
- **AND** stops the proxy
- **AND** tears down the WireGuard tunnel
- **AND** attempts to pause the server session
