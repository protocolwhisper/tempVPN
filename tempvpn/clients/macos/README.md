# TempVPN on macOS

The macOS product is native and agent-only. It has no window, Dock icon, menu
bar item, or graphical controls:

```text
Agent -> tempvpnctl -> NETunnelProviderManager
                         |
                         v
                headless TempVPN.app
                         |
                         v
              Packet Tunnel Provider
                         |
                         v
                    WireGuardKit
```

`TempVPN.app` is still technically required because macOS only installs Packet
Tunnel extensions from a containing application bundle. Its `LSUIElement` flag
is enabled and its executable exits immediately after LaunchServices registers
the extension. The agent interacts exclusively with `tempvpnctl`, while the
resulting `TempVPN` profile is visible and controllable in System Settings.

No Homebrew WireGuard commands are used by the macOS client. Linux remains a
separate Rust client that uses `wg`/`wg-quick`.

## Agent workflow

```bash
export VPN_CLIENT_REGISTRY_URL="https://registry.example.com"
tempvpnctl select --country DE --selection-policy lowest-latency --json
tempvpnctl check --node-url "$SELECTED_NODE_URL" --json

mppx "$SELECTED_NODE_URL/sessions" \
  --account main \
  --json-body '{"duration_seconds":1800}' \
  --silent > /tmp/tempvpn-session.json

tempvpnctl connect \
  --session-response /tmp/tempvpn-session.json \
  --node-url "$SELECTED_NODE_URL" \
  --node-name "$SELECTED_NODE_NAME" \
  --country-code "$SELECTED_COUNTRY_CODE" \
  --city "$SELECTED_CITY" \
  --region "$SELECTED_REGION" \
  --json

tempvpnctl status --json
tempvpnctl disconnect --json
```

`tempvpnctl` generates an X25519/WireGuard key, stores the private key in the
shared Keychain access group, and sends only the public key to the selected
node. The Packet Tunnel extension retrieves the private key internally, starts
WireGuardKit, heartbeats the paid session every 30 seconds, and pauses unused
time when the tunnel stops.

Payment stays outside the client. The agent must pay the exact selected node
with an already configured `mppx` account and pass that node's JSON response to
`tempvpnctl`. The CLI rejects a response bound to a different node. Omit absent
optional location arguments. The live `check` must run immediately before
`mppx`; advertised capacity is a snapshot and does not reserve a slot.

Natural-language parsing does not run in the indexer. The agent converts a
request such as “Connect 30 mins to Germany” into duration `1800`, country `DE`,
and policy `lowest_latency`; `tempvpnctl` sends only structured query fields.
The client measures latency from this Mac, because indexer-to-node latency does
not represent the user's network path.

## Build before Apple signing

Prerequisites are Xcode and Go. Build both products without installing them:

```bash
./clients/macos/build-macos-products.sh
```

Outputs:

```text
target/TempVPN.app
target/tempvpnctl
```

An unsigned build verifies compilation but cannot install or activate the
Packet Tunnel extension.

## Signing for local use

A paid Apple Developer Program membership is only required for Developer ID
distribution and notarization. For a build that runs on this Mac, a free
Apple ID is sufficient: add it under Xcode > Settings > Accounts to get a
Personal Team, then build with the same team and signing identity for both
products:

```bash
export APPLE_DEVELOPMENT_TEAM="TEAMID"
export CODE_SIGN_IDENTITY="Apple Development: Your Name (TEAMID)"
./clients/macos/build-macos-products.sh
```

The application, extension, and CLI share the Keychain group
`TEAMID.com.protocolwhisper.tempvpn.shared`. If Xcode reports the bundle
identifier `com.tempo.tempvpn` as unavailable for your team, choose a unique
bundle ID prefix for the app and Packet Tunnel targets before building.
Production downloads additionally need Developer ID distribution signing and
notarization.

Install only signed products. The installer deliberately refuses unsigned
artifacts and does not invoke `sudo` itself:

```bash
sudo ./clients/macos/install-tempvpnctl.sh
```

The installer places the invisible host in `/Applications/TempVPN.app`, the
agent command in `/usr/local/bin/tempvpnctl`, and launches the host once to
register the extension. macOS then requests one-time VPN approval on the first
connection.
