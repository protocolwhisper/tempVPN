#!/bin/bash
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
project="$root/clients/macos/TempVPN.xcodeproj"
derived="$root/target/xcode-derived"
packages="$root/target/xcode-packages"
package_cache="$root/target/xcode-package-cache"
module_cache="$root/target/xcode-module-cache"
xcode_home="$root/target/xcode-home"
output=${1:-"$root/target/TempVPN.app"}

arguments=(
  -project "$project"
  -scheme TempVPN
  -configuration Release
  -destination "platform=macOS"
  -derivedDataPath "$derived"
  -clonedSourcePackagesDirPath "$packages"
  -packageCachePath "$package_cache"
)

if [[ -n "${APPLE_DEVELOPMENT_TEAM:-}" ]]; then
  arguments+=(-allowProvisioningUpdates DEVELOPMENT_TEAM="$APPLE_DEVELOPMENT_TEAM")
  if [[ -n "${CODE_SIGN_IDENTITY:-}" ]]; then
    arguments+=(CODE_SIGN_IDENTITY="$CODE_SIGN_IDENTITY")
  fi
else
  arguments+=(CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO)
fi

mkdir -p "$package_cache" "$module_cache" "$xcode_home/Library/Caches"
if [[ -n "${APPLE_DEVELOPMENT_TEAM:-}" ]]; then
  CLANG_MODULE_CACHE_PATH="$module_cache" \
  SWIFTPM_MODULECACHE_OVERRIDE="$module_cache" \
    xcodebuild "${arguments[@]}" build
else
  CLANG_MODULE_CACHE_PATH="$module_cache" \
  SWIFTPM_MODULECACHE_OVERRIDE="$module_cache" \
  SWIFTPM_DISABLE_SANDBOX=1 \
  HOME="$xcode_home" \
  CFFIXED_USER_HOME="$xcode_home" \
    xcodebuild "${arguments[@]}" IDEPackageSupportDisableManifestSandbox=YES build
fi

product="$derived/Build/Products/Release/TempVPN.app"
test -d "$product"
mkdir -p "$(dirname "$output")"
rm -rf "$output"
ditto "$product" "$output"

echo "Built headless host at $output"
if [[ -z "${APPLE_DEVELOPMENT_TEAM:-}" ]]; then
  echo "This build is for compilation verification only; sign it before installation."
fi
