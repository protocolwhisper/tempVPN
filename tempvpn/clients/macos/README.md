# TempVPN Apple Networking

This directory contains the native macOS TempVPN app, command-line controller,
and Packet Tunnel Provider shown in macOS System Settings.

The current Rust client uses `wg-quick`, which creates a working `utun`
WireGuard tunnel but does not register a macOS VPN service. This scaffold moves
the macOS-facing connection lifecycle into Apple's VPN APIs:

- `HostApp/TempoVPNController.swift` creates or updates a `NETunnelProvider`
  VPN profile named `TempVPN` and starts it with a temporary WireGuard config.
- `CLI/` builds `tempvpnctl`, which selects nodes, imports paid-session JSON,
  stores private keys in the shared Keychain group, and controls the VPN profile.
- `PacketTunnel/PacketTunnelProvider.swift` hands the resolved configuration to
  WireGuardKit, sends heartbeats, and pauses unused time when stopped.
- `Shared/TempoVPNProfile.swift` contains the small shared model and option keys
  used by both targets.
- `HostApp/TempoVPNStatusStore.swift` reads the shared Rust status file so a
  menu bar companion can show remaining connected time and current node state.

`TempVPN.xcodeproj` contains a minimal macOS menu bar host app target and a
Packet Tunnel Provider extension target. The host app embeds the extension and
uses the Network Extension entitlement with `packet-tunnel-provider`.

WireGuardKit is vendored at `Vendor/wireguard-apple` and linked into
`TempVPNPacketTunnel` as a local Swift Package. The vendored copy carries two
small compatibility patches for this toolchain:

- `Package.swift` uses `swift-tools-version:5.5` so SwiftPM accepts the package
  platform declarations.
- `WireGuardKitC.h` imports `<sys/types.h>` before using BSD integer typedefs
  required by the latest modular macOS SDK.

WireGuardKit also requires its Go backend archive, `libwg-go.a`. The packet
tunnel target has a build phase that runs WireGuard's own Makefile to produce
that archive. Install Go before building the Xcode project.

The payment/session flow now separates purchase from connected time. `POST
/sessions` creates a paid usage balance, `POST /sessions/{id}/connect` starts
burning that balance and returns the assigned IP, server public key, and
endpoint, and `POST /sessions/{id}/pause` stops burning time. After connect,
render the normal WireGuard config and call `TempoVPNController.connect(profile:)`.
macOS then owns the VPN lifecycle, so the connection appears in System Settings
instead of only existing as a `wg-quick` interface.

The Rust `vpn-client` is Linux-only and is not part of the macOS product path.

Build and sign the CLI with the same team and shared Keychain group as the app:

```bash
CODE_SIGN_IDENTITY="Developer ID Application: Example (TEAMID)" \
  ./clients/macos/build-tempvpnctl.sh
```

## Distribution

For production macOS users, distribute `TempVPN.app` in a signed and notarized
DMG. A raw binary or CLI-only release is useful for agents and development, but
it cannot deliver the normal VPN-app experience: one-time profile approval,
System Settings integration, menu bar status, and later connects without admin
prompts all require the signed app plus embedded Packet Tunnel Provider.
