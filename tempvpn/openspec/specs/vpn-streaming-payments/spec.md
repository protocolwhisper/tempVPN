# VPN Streaming Payments Specification

## Purpose

Define how a VPN client purchases continuously metered access through current Tempo MPP Session v2 payments while the node safely controls the corresponding WireGuard peer.

## Requirements

### Requirement: Negotiate a current Tempo payment session
The node SHALL challenge an unpaid streaming-session request with MPP method `tempo`, intent `session`, and method detail `sessionProtocol: "v2"`. The challenge SHALL identify the configured currency and recipient, price access in time units, and include a suggested reserve sufficient for more than one billing unit.

#### Scenario: Client starts without a credential
- **WHEN** a client requests streaming VPN access without an MPP credential
- **THEN** the node responds with HTTP 402 and a current Tempo Session v2 challenge

#### Scenario: Legacy session client attempts payment
- **WHEN** a credential uses the legacy contract-backed session protocol
- **THEN** the node rejects it without creating a VPN peer

### Requirement: Activate access only after verified funding
The node SHALL create or activate a WireGuard peer only after it has verified a Session v2 channel operation that funds at least one billing unit for the requested client key.

#### Scenario: Valid funded channel
- **WHEN** the node verifies a current Session v2 credential with sufficient available value
- **THEN** it creates the VPN session and emits its connection details on the authenticated stream

#### Scenario: Invalid session credential
- **WHEN** channel identity, signature, chain, currency, recipient, or available value validation fails
- **THEN** the node rejects the request and does not create or retain a WireGuard peer

### Requirement: Meter active VPN time atomically
The node SHALL charge the configured amount once per completed billing interval using an atomic compare-and-update of channel state. It SHALL NOT deliver a paid interval event or keep access active for that interval unless the corresponding value has been reserved by an accepted cumulative voucher.

#### Scenario: Balance covers the next interval
- **WHEN** a billing interval completes and accepted channel value covers its charge
- **THEN** the node atomically records one charged unit and keeps the VPN peer active

#### Scenario: Concurrent voucher processing
- **WHEN** multiple requests attempt to advance the same channel concurrently
- **THEN** the node accepts only monotonic valid state transitions and never spends the same reserved value twice

### Requirement: Pause service while awaiting a voucher
When accepted channel value cannot cover the next billing interval, the node SHALL stop paid access and emit a `payment-need-voucher` event containing the cumulative amount required to continue. It SHALL resume only after verifying sufficient newer channel state.

#### Scenario: Balance is exhausted
- **WHEN** the next interval cannot be charged from accepted channel value
- **THEN** the node removes or disables the WireGuard peer and emits `payment-need-voucher` without charging the interval

#### Scenario: Client replenishes the channel
- **WHEN** the node verifies a newer voucher or top-up that covers the required cumulative amount
- **THEN** it restores the same logical VPN session and continues metering without charging the paused period

### Requirement: End access safely
The node SHALL remove the WireGuard peer when the payment stream closes, the payment channel is finalized, the session reaches its configured safety limit, or the client fails to replenish within the configured grace period. The stream SHALL emit a final payment receipt when channel state permits.

#### Scenario: Client closes normally
- **WHEN** the client closes its payment session and the final state is verified
- **THEN** the node removes the peer and returns a receipt for the accepted cumulative value and charged units

#### Scenario: Stream disappears unexpectedly
- **WHEN** the authenticated stream disconnects and is not resumed within the grace period
- **THEN** the node removes the peer and retains the last valid channel state for settlement and replay protection

### Requirement: Preserve one-time session compatibility
The node SHALL keep the existing one-time charged `POST /sessions` behavior available while the streaming endpoint is introduced.

#### Scenario: Existing one-time client creates a session
- **WHEN** a client completes the existing one-time MPP charge flow on `POST /sessions`
- **THEN** the node creates a fixed-duration VPN session with the same externally visible response behavior as before the upgrade

### Requirement: Require durable production state
The node SHALL support in-memory channel state only for local development and single-process testing. Production configuration SHALL fail closed unless channel accounting, voucher monotonicity, replay protection, and stream ownership use a durable atomic store shared by all serving instances.

#### Scenario: Development mode uses memory
- **WHEN** the node runs explicitly in local development mode
- **THEN** it may use an in-memory channel store and reports that the state is non-durable

#### Scenario: Production starts without durable storage
- **WHEN** production mode is selected without a configured durable atomic store
- **THEN** node startup fails before accepting payment traffic
