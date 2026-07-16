#!/bin/bash
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
output=${1:-"$root/target/tempvpnctl"}
identity=${CODE_SIGN_IDENTITY:?Set CODE_SIGN_IDENTITY to the macOS signing identity}
mkdir -p "$(dirname "$output")"

CLANG_MODULE_CACHE_PATH="$root/target/swift-cli-module-cache" \
  swiftc -parse-as-library "$root"/clients/macos/CLI/*.swift -o "$output"
codesign --force --options runtime --timestamp --sign "$identity" \
  --entitlements "$root/clients/macos/Resources/CLI/tempvpnctl.entitlements" \
  "$output"
