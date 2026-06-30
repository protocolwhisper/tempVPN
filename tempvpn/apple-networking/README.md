# Apple Networking VPN

This directory is a minimal macOS NetworkExtension scaffold for making tempVPN
appear as a native VPN in macOS System Settings.

The current Rust client uses `wg-quick`, which creates a working `utun`
WireGuard tunnel but does not register a macOS VPN service. This scaffold moves
the macOS-facing connection lifecycle into Apple's VPN APIs:

- `HostApp/TempoVPNController.swift` creates or updates a `NETunnelProvider`
  VPN profile named `Tempo VPN` and starts it with a temporary WireGuard config.
- `PacketTunnel/PacketTunnelProvider.swift` is the NetworkExtension entry point.
  It expects the WireGuard config at tunnel start and hands it to WireGuardKit.
- `Shared/TempoVPNProfile.swift` contains the small shared model and option keys
  used by both targets.

To make this buildable, create a macOS app target and a Packet Tunnel Provider
extension target in Xcode, add these Swift files, link WireGuardKit into the
extension, and enable the Network Extension entitlement with
`packet-tunnel-provider`.

The existing payment/session flow can stay the same: after `POST /sessions`
returns the assigned IP, server public key, and endpoint, render the normal
WireGuard config and call `TempoVPNController.connect(profile:)`. macOS then
owns the VPN lifecycle, so the connection appears in System Settings instead of
only existing as a `wg-quick` interface.

This scaffold does not replace the current CLI path yet. It is the native macOS
integration point for the next app-target build.
