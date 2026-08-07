#!/bin/bash
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
app_source=${TEMPVPN_APP_SOURCE:-"$root/target/TempVPN.app"}
cli_source=${TEMPVPN_CLI_SOURCE:-"$root/target/tempvpnctl"}
app_destination=${TEMPVPN_APP_DESTINATION:-"/Applications/TempVPN.app"}
cli_destination=${TEMPVPN_CLI_DESTINATION:-"/usr/local/bin/tempvpnctl"}

test -d "$app_source/Contents/PlugIns/TempVPNPacketTunnel.appex" || {
  echo "Missing signed headless app at $app_source" >&2
  exit 1
}
test -x "$cli_source" || {
  echo "Missing signed tempvpnctl at $cli_source" >&2
  exit 1
}
codesign --verify --deep --strict "$app_source" || {
  echo "TempVPN.app is not correctly signed." >&2
  exit 1
}
codesign --verify --strict "$cli_source" || {
  echo "tempvpnctl is not correctly signed." >&2
  exit 1
}

test -w "$(dirname "$app_destination")" && test -w "$(dirname "$cli_destination")" || {
  echo "Installation needs administrator permission. Ask the user before rerunning this installer with sudo." >&2
  exit 1
}

rm -rf "$app_destination"
ditto "$app_source" "$app_destination"
install -m 755 "$cli_source" "$cli_destination"

# Launch once without showing UI so LaunchServices registers the extension.
open -gj "$app_destination"
echo "Installed headless TempVPN.app and tempvpnctl. macOS will request one-time VPN approval on first connect."
