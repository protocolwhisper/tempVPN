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

if [[ -n "${TEMPVPN_VERSION:-}" ]]; then
  [[ "$TEMPVPN_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]] || {
    echo "TEMPVPN_VERSION must be a semantic version without a leading v." >&2
    exit 1
  }
  arguments+=(MARKETING_VERSION="$TEMPVPN_VERSION")
fi

if [[ -n "${TEMPVPN_BUILD_NUMBER:-}" ]]; then
  [[ "$TEMPVPN_BUILD_NUMBER" =~ ^[1-9][0-9]*$ ]] || {
    echo "TEMPVPN_BUILD_NUMBER must be a positive integer." >&2
    exit 1
  }
  arguments+=(CURRENT_PROJECT_VERSION="$TEMPVPN_BUILD_NUMBER")
fi

if [[ -n "${APPLE_DEVELOPMENT_TEAM:-}" ]]; then
  arguments+=(-allowProvisioningUpdates DEVELOPMENT_TEAM="$APPLE_DEVELOPMENT_TEAM")
  requested_identity=${XCODE_CODE_SIGN_IDENTITY:-${CODE_SIGN_IDENTITY:-}}
  if [[ "$requested_identity" == "Developer ID Application"* ]]; then
    arguments+=(CODE_SIGN_IDENTITY="Apple Development")
  elif [[ -n "${XCODE_CODE_SIGN_IDENTITY:-}" ]]; then
    arguments+=(CODE_SIGN_IDENTITY="$XCODE_CODE_SIGN_IDENTITY")
  elif [[ "${CODE_SIGN_IDENTITY:-}" == "Apple Development:"* ]]; then
    arguments+=(CODE_SIGN_IDENTITY="Apple Development")
  elif [[ "${CODE_SIGN_IDENTITY:-}" == "Developer ID Application:"* ]]; then
    arguments+=(CODE_SIGN_IDENTITY="Developer ID Application")
  elif [[ -n "${CODE_SIGN_IDENTITY:-}" ]]; then
    arguments+=(CODE_SIGN_IDENTITY="$CODE_SIGN_IDENTITY")
  fi
  if [[ -n "${APPLE_AUTH_KEY_PATH:-}" ]]; then
    : "${APPLE_AUTH_KEY_ID:?Set APPLE_AUTH_KEY_ID with APPLE_AUTH_KEY_PATH}"
    : "${APPLE_AUTH_KEY_ISSUER_ID:?Set APPLE_AUTH_KEY_ISSUER_ID with APPLE_AUTH_KEY_PATH}"
    arguments+=(
      -authenticationKeyPath "$APPLE_AUTH_KEY_PATH"
      -authenticationKeyID "$APPLE_AUTH_KEY_ID"
      -authenticationKeyIssuerID "$APPLE_AUTH_KEY_ISSUER_ID"
    )
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

if [[ "${requested_identity:-}" == "Developer ID Application"* ]]; then
  : "${CODE_SIGN_IDENTITY:?Set CODE_SIGN_IDENTITY to the exact Developer ID Application identity}"
  profile_directory="$HOME/Library/Developer/Xcode/UserData/Provisioning Profiles"
  [[ -d "$profile_directory" ]] || {
    echo "Xcode-managed provisioning profiles are missing." >&2
    exit 1
  }

  direct_profile() {
    local bundle_identifier=$1 profile name
    for profile in "$profile_directory"/*.provisionprofile; do
      [[ -f "$profile" ]] || continue
      name=$(security cms -D -i "$profile" 2>/dev/null | plutil -extract Name raw - 2>/dev/null || true)
      if [[ "$name" == "Mac Team Direct Provisioning Profile: $bundle_identifier" ]]; then
        printf '%s\n' "$profile"
        return 0
      fi
    done
    echo "Missing Xcode-managed Direct provisioning profile for $bundle_identifier." >&2
    return 1
  }

  extension="$output/Contents/PlugIns/TempVPNPacketTunnel.appex"
  app_profile=$(direct_profile com.tempo.tempvpn)
  extension_profile=$(direct_profile com.tempo.tempvpn.PacketTunnel)
  cp "$app_profile" "$output/Contents/embedded.provisionprofile"
  cp "$extension_profile" "$extension/Contents/embedded.provisionprofile"

  app_entitlements="$root/target/TempVPN-direct.entitlements"
  extension_entitlements="$root/target/TempVPNPacketTunnel-direct.entitlements"
  cp "$root/clients/macos/Resources/HostApp/TempVPN.entitlements" "$app_entitlements"
  cp "$root/clients/macos/Resources/PacketTunnel/PacketTunnel.entitlements" "$extension_entitlements"
  for entitlements in "$app_entitlements" "$extension_entitlements"; do
    /usr/libexec/PlistBuddy -c "Set :com.apple.developer.networking.networkextension:0 packet-tunnel-provider-systemextension" "$entitlements"
    /usr/libexec/PlistBuddy -c "Set :keychain-access-groups:0 ${APPLE_DEVELOPMENT_TEAM}.com.protocolwhisper.tempvpn.shared" "$entitlements"
  done

  codesign --force --options runtime --timestamp --sign "$CODE_SIGN_IDENTITY" \
    --entitlements "$extension_entitlements" "$extension"
  codesign --force --options runtime --timestamp --sign "$CODE_SIGN_IDENTITY" \
    --entitlements "$app_entitlements" "$output"
fi

echo "Built headless host at $output"
if [[ -z "${APPLE_DEVELOPMENT_TEAM:-}" ]]; then
  echo "This build is for compilation verification only; sign it before installation."
fi
