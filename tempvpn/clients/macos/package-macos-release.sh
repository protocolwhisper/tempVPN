#!/bin/bash
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
version=${TEMPVPN_VERSION:-}
team_id=${APPLE_DEVELOPMENT_TEAM:-}
installer_identity=${DEVELOPER_ID_INSTALLER_IDENTITY:-}
app_source=${TEMPVPN_APP_SOURCE:-"$root/target/TempVPN.app"}
cli_source=${TEMPVPN_CLI_SOURCE:-"$root/target/tempvpnctl"}
output_dir=${TEMPVPN_RELEASE_OUTPUT_DIR:-"$root/target/release"}
repository="protocolwhisper/tempVPN"
release_tag="v$version"
package_id="com.tempo.tempvpn.pkg"
manifest_name="tempvpn-macos-manifest.json"

[[ -n "$version" ]] || { echo "Set TEMPVPN_VERSION without a leading v." >&2; exit 1; }
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]] || {
  echo "TEMPVPN_VERSION must be a semantic version without a leading v." >&2
  exit 1
}
[[ "$team_id" =~ ^[A-Z0-9]{10}$ ]] || { echo "Set APPLE_DEVELOPMENT_TEAM to the 10-character team ID." >&2; exit 1; }
[[ -n "$installer_identity" ]] || { echo "Set DEVELOPER_ID_INSTALLER_IDENTITY." >&2; exit 1; }
[[ -d "$app_source" && -x "$cli_source" ]] || { echo "Build the signed app and CLI before packaging." >&2; exit 1; }

extension_source="$app_source/Contents/PlugIns/TempVPNPacketTunnel.appex"
[[ -d "$extension_source" ]] || { echo "Signed app is missing its Packet Tunnel extension." >&2; exit 1; }

identity_field() {
  local product=$1 field=$2
  codesign -dv --verbose=4 "$product" 2>&1 | sed -n "s/^${field}=//p" | head -n 1
}

for product in "$app_source" "$extension_source" "$cli_source"; do
  codesign --verify --strict --verbose=2 "$product" >/dev/null
  [[ "$(identity_field "$product" TeamIdentifier)" == "$team_id" ]] || {
    echo "Signing team mismatch for $product." >&2
    exit 1
  }
  codesign -dv --verbose=4 "$product" 2>&1 | grep -F "Authority=Developer ID Application:" >/dev/null || {
    echo "$product must be signed with Developer ID Application." >&2
    exit 1
  }
  codesign -dv --verbose=4 "$product" 2>&1 | grep -F "Runtime Version=" >/dev/null || {
    echo "$product must enable the hardened runtime." >&2
    exit 1
  }
done

architecture=$(lipo -archs "$app_source/Contents/MacOS/TempVPN")
cli_architecture=$(lipo -archs "$cli_source")
[[ "$architecture" == "$cli_architecture" && "$architecture" != *" "* ]] || {
  echo "App and CLI must use the same single architecture." >&2
  exit 1
}

work_dir=$(mktemp -d "${TMPDIR:-/tmp}/tempvpn-release.XXXXXX")
trap 'rm -rf "$work_dir"' EXIT
payload="$work_dir/payload"
mkdir -p "$payload/Applications" "$payload/usr/local/bin" "$output_dir"
ditto "$app_source" "$payload/Applications/TempVPN.app"
install -m 755 "$cli_source" "$payload/usr/local/bin/tempvpnctl"

package_name="TempVPN-${version}-macos-${architecture}.pkg"
package="$output_dir/$package_name"
manifest="$output_dir/$manifest_name"
[[ ! -e "$package" ]] || {
  echo "Refusing to overwrite existing release package: $package" >&2
  exit 1
}
package_work="$work_dir/$package_name"

pkgbuild \
  --root "$payload" \
  --identifier "$package_id" \
  --version "$version" \
  --install-location / \
  --sign "$installer_identity" \
  "$package_work"

if [[ -n "${NOTARY_KEYCHAIN_PROFILE:-}" ]]; then
  xcrun notarytool submit "$package_work" --keychain-profile "$NOTARY_KEYCHAIN_PROFILE" --wait
elif [[ -n "${APPLE_AUTH_KEY_PATH:-}" ]]; then
  : "${APPLE_AUTH_KEY_ID:?Set APPLE_AUTH_KEY_ID with APPLE_AUTH_KEY_PATH}"
  : "${APPLE_AUTH_KEY_ISSUER_ID:?Set APPLE_AUTH_KEY_ISSUER_ID with APPLE_AUTH_KEY_PATH}"
  xcrun notarytool submit "$package_work" \
    --key "$APPLE_AUTH_KEY_PATH" \
    --key-id "$APPLE_AUTH_KEY_ID" \
    --issuer "$APPLE_AUTH_KEY_ISSUER_ID" \
    --wait
else
  echo "Set NOTARY_KEYCHAIN_PROFILE or the APPLE_AUTH_KEY_* variables." >&2
  exit 1
fi

xcrun stapler staple "$package_work"
xcrun stapler validate "$package_work"
spctl --assess --type install --verbose=4 "$package_work"

"$root/agent/scripts/verify-macos-package.sh" "$package_work" >/dev/null
sha256=$(shasum -a 256 "$package_work" | awk '{print $1}')
package_url="https://github.com/${repository}/releases/download/${release_tag}/${package_name}"

manifest_plist="$work_dir/manifest.plist"
plutil -create xml1 "$manifest_plist"
plutil -insert schema_version -integer 1 "$manifest_plist"
plutil -insert version -string "$version" "$manifest_plist"
plutil -insert architectures -json "[\"$architecture\"]" "$manifest_plist"
plutil -insert minimum_macos -string "13.0" "$manifest_plist"
plutil -insert package_url -string "$package_url" "$manifest_plist"
plutil -insert sha256 -string "$sha256" "$manifest_plist"
plutil -insert team_id -string "$team_id" "$manifest_plist"
plutil -insert package_identifier -string "$package_id" "$manifest_plist"
plutil -insert app_bundle_identifier -string "com.tempo.tempvpn" "$manifest_plist"
plutil -insert extension_bundle_identifier -string "com.tempo.tempvpn.PacketTunnel" "$manifest_plist"
manifest_output="$work_dir/$manifest_name"
plutil -convert json -o "$manifest_output" "$manifest_plist"
mv "$package_work" "$package"
mv -f "$manifest_output" "$manifest"

echo "Created notarized release assets:"
echo "  $package"
echo "  $manifest"
