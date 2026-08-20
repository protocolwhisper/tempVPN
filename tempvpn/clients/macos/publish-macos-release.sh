#!/bin/bash
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
version=${TEMPVPN_VERSION:-}
team_id=${APPLE_DEVELOPMENT_TEAM:-}
application_identity=${DEVELOPER_ID_APPLICATION_IDENTITY:-}
installer_identity=${DEVELOPER_ID_INSTALLER_IDENTITY:-}

[[ -n "$version" ]] || { echo "Set TEMPVPN_VERSION without a leading v." >&2; exit 1; }
[[ -n "$team_id" ]] || { echo "Set APPLE_DEVELOPMENT_TEAM." >&2; exit 1; }
[[ -n "$application_identity" ]] || { echo "Set DEVELOPER_ID_APPLICATION_IDENTITY." >&2; exit 1; }
[[ -n "$installer_identity" ]] || { echo "Set DEVELOPER_ID_INSTALLER_IDENTITY." >&2; exit 1; }

branch=$(git -C "$root" branch --show-current)
[[ "$branch" != "deploymaster" ]] || {
  echo "Refusing to publish from local-only deploymaster. Switch to a publishable branch." >&2
  exit 1
}
git -C "$root" diff --quiet && git -C "$root" diff --cached --quiet || {
  echo "Refusing to publish from a dirty tracked worktree." >&2
  exit 1
}

for command in gh go pkgbuild xcodebuild; do
  command -v "$command" >/dev/null 2>&1 || { echo "Required command is missing: $command" >&2; exit 1; }
done
gh auth status >/dev/null

export CODE_SIGN_IDENTITY="$application_identity"
export XCODE_CODE_SIGN_IDENTITY="Developer ID Application"
export TEMPVPN_BUILD_NUMBER=${TEMPVPN_BUILD_NUMBER:-1}
export DEVELOPER_ID_INSTALLER_IDENTITY="$installer_identity"

"$root/clients/macos/build-macos-products.sh"
"$root/clients/macos/package-macos-release.sh"

architecture=$(lipo -archs "$root/target/TempVPN.app/Contents/MacOS/TempVPN")
package="$root/target/release/TempVPN-${version}-macos-${architecture}.pkg"
manifest="$root/target/release/tempvpn-macos-manifest.json"
tag="v$version"

if gh release view "$tag" >/dev/null 2>&1; then
  gh release upload "$tag" "$package" "$manifest" --clobber
else
  gh release create "$tag" "$package" "$manifest" \
    --target "$(git -C "$root" rev-parse HEAD)" \
    --title "TempVPN $tag" \
    --generate-notes \
    --latest
fi

echo "Published $tag. Stable agent manifest:"
echo "https://github.com/protocolwhisper/tempVPN/releases/latest/download/tempvpn-macos-manifest.json"
