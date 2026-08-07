#!/bin/bash
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
package="$root/clients/macos"
output=${1:-"$root/target/tempvpnctl"}

CLANG_MODULE_CACHE_PATH="$root/target/swift-module-cache" swift build \
  --package-path "$package" \
  --disable-sandbox \
  --configuration release \
  --cache-path "$root/target/swift-cache" \
  --config-path "$root/target/swift-config" \
  --security-path "$root/target/swift-security" \
  --scratch-path "$root/target/swift-tempvpnctl"

mkdir -p "$(dirname "$output")"
cp "$root/target/swift-tempvpnctl/release/tempvpnctl" "$output"
chmod 755 "$output"

if [[ -n "${CODE_SIGN_IDENTITY:-}" ]]; then
  : "${APPLE_DEVELOPMENT_TEAM:?Set APPLE_DEVELOPMENT_TEAM when signing tempvpnctl}"
  entitlements="$root/target/tempvpnctl.entitlements"
  cp "$root/clients/macos/Resources/CLI/tempvpnctl.entitlements" "$entitlements"
  /usr/libexec/PlistBuddy \
    -c "Set :keychain-access-groups:0 ${APPLE_DEVELOPMENT_TEAM}.com.protocolwhisper.tempvpn.shared" \
    "$entitlements"
  codesign --force --options runtime --timestamp --sign "$CODE_SIGN_IDENTITY" \
    --entitlements "$entitlements" "$output"
fi

echo "Built $output"
