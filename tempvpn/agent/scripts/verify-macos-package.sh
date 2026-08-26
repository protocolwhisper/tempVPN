#!/bin/bash
set -euo pipefail

expected_team_id="T4295L8LL4"
expected_package_id="com.tempo.tempvpn.pkg"
expected_app_id="com.tempo.tempvpn"
expected_extension_id="com.tempo.tempvpn.PacketTunnel"
expected_keychain_group="$expected_team_id.com.protocolwhisper.tempvpn.shared"

usage() {
  echo "usage: $0 PACKAGE.pkg" >&2
  exit 2
}

[[ $# -eq 1 ]] || usage
package=$1
[[ -f "$package" ]] || {
  echo "Package does not exist: $package" >&2
  exit 1
}

for command in codesign pkgutil plutil spctl xcrun; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "Required macOS command is missing: $command" >&2
    exit 1
  }
done

work_dir=$(mktemp -d "${TMPDIR:-/tmp}/tempvpn-package-verify.XXXXXX")
trap 'rm -rf "$work_dir"' EXIT
expanded="$work_dir/expanded"

if ! signature_report=$(pkgutil --check-signature "$package" 2>&1); then
  echo "Package has no valid Installer signature." >&2
  exit 1
fi
grep -F "Developer ID Installer:" <<<"$signature_report" >/dev/null || {
  echo "Package is not signed with a Developer ID Installer certificate." >&2
  exit 1
}
grep -F "($expected_team_id)" <<<"$signature_report" >/dev/null || {
  echo "Package signer does not belong to expected Apple team $expected_team_id." >&2
  exit 1
}

spctl --assess --type install --verbose=4 "$package" >/dev/null
xcrun stapler validate "$package" >/dev/null
pkgutil --expand-full "$package" "$expanded"

package_info_count=$(find "$expanded" -name PackageInfo -type f -print | awk 'END { print NR }')
[[ "$package_info_count" -eq 1 ]] || {
  echo "Expected one component package, found $package_info_count." >&2
  exit 1
}
package_info=$(find "$expanded" -name PackageInfo -type f -print | head -n 1)
grep -F "identifier=\"$expected_package_id\"" "$package_info" >/dev/null || {
  echo "Unexpected package identifier." >&2
  exit 1
}

app_count=$(find "$expanded" -path '*/Payload/Applications/TempVPN.app' -type d -print | awk 'END { print NR }')
cli_count=$(find "$expanded" -path '*/Payload/usr/local/bin/tempvpnctl' -type f -print | awk 'END { print NR }')
[[ "$app_count" -eq 1 && "$cli_count" -eq 1 ]] || {
  echo "Package must contain exactly one TempVPN.app and one tempvpnctl." >&2
  exit 1
}

app=$(find "$expanded" -path '*/Payload/Applications/TempVPN.app' -type d -print | head -n 1)
cli=$(find "$expanded" -path '*/Payload/usr/local/bin/tempvpnctl' -type f -print | head -n 1)
extension="$app/Contents/PlugIns/TempVPNPacketTunnel.appex"
[[ -d "$extension" ]] || {
  echo "Package is missing the Packet Tunnel extension." >&2
  exit 1
}

identity_field() {
  local product=$1 field=$2
  codesign -dv --verbose=4 "$product" 2>&1 | sed -n "s/^${field}=//p" | head -n 1
}

entitlements_file() {
  local product=$1 output=$2
  codesign -d --entitlements - "$product" >"$output" 2>/dev/null
  grep -F "[Dict]" "$output" >/dev/null || {
    echo "Could not read DER entitlements for $product." >&2
    exit 1
  }
}

entitlement_has_value() {
  local report=$1 key=$2 expected=$3
  awk -v key="$key" -v expected="$expected" '
    $0 == "\t[Key] " key { within_key = 1; next }
    within_key && $0 ~ /^\t\[Key\] / { exit }
    within_key && index($0, expected) { found = 1 }
    END { exit !found }
  ' "$report"
}

require_entitlement_value() {
  local report=$1 key=$2 expected=$3
  entitlement_has_value "$report" "$key" "$expected" || {
    echo "Missing or incorrect entitlement $key (expected $expected)." >&2
    exit 1
  }
}

codesign --verify --deep --strict --verbose=2 "$app" >/dev/null
codesign --verify --strict --verbose=2 "$extension" >/dev/null
codesign --verify --strict --verbose=2 "$cli" >/dev/null

for product in "$app" "$extension" "$cli"; do
  [[ "$(identity_field "$product" TeamIdentifier)" == "$expected_team_id" ]] || {
    echo "Code signature team mismatch for $product." >&2
    exit 1
  }
  codesign -dv --verbose=4 "$product" 2>&1 | grep -F "Authority=Developer ID Application:" >/dev/null || {
    echo "$product is not signed for Developer ID distribution." >&2
    exit 1
  }
  codesign -dv --verbose=4 "$product" 2>&1 | grep -F "Runtime Version=" >/dev/null || {
    echo "$product is not protected by the hardened runtime." >&2
    exit 1
  }
done

[[ "$(identity_field "$app" Identifier)" == "$expected_app_id" ]] || {
  echo "Unexpected application bundle identifier." >&2
  exit 1
}
[[ "$(identity_field "$extension" Identifier)" == "$expected_extension_id" ]] || {
  echo "Unexpected Packet Tunnel bundle identifier." >&2
  exit 1
}

app_entitlements="$work_dir/app-entitlements.plist"
extension_entitlements="$work_dir/extension-entitlements.plist"
cli_entitlements="$work_dir/cli-entitlements.plist"
entitlements_file "$app" "$app_entitlements"
entitlements_file "$extension" "$extension_entitlements"
entitlements_file "$cli" "$cli_entitlements"

require_entitlement_value "$app_entitlements" "com.apple.developer.networking.networkextension" "packet-tunnel-provider-systemextension"
require_entitlement_value "$extension_entitlements" "com.apple.developer.networking.networkextension" "packet-tunnel-provider-systemextension"
require_entitlement_value "$app_entitlements" "keychain-access-groups" "$expected_keychain_group"
require_entitlement_value "$extension_entitlements" "keychain-access-groups" "$expected_keychain_group"
require_entitlement_value "$cli_entitlements" "keychain-access-groups" "$expected_keychain_group"

for plist in "$app_entitlements" "$extension_entitlements" "$cli_entitlements"; do
  if entitlement_has_value "$plist" "com.apple.security.get-task-allow" "true"; then
    echo "Development-only get-task-allow entitlement is present." >&2
    exit 1
  fi
done

version=$(plutil -extract CFBundleShortVersionString raw -o - "$app/Contents/Info.plist")
architecture=$(lipo -archs "$app/Contents/MacOS/TempVPN")
[[ "$architecture" != *" "* ]] || {
  echo "Universal packages are not yet represented by this release manifest." >&2
  exit 1
}

printf '{"ready":true,"version":"%s","architecture":"%s","team_id":"%s"}\n' \
  "$version" "$architecture" "$expected_team_id"
