#!/bin/bash
set -euo pipefail

default_manifest_url="https://github.com/protocolwhisper/tempVPN/releases/latest/download/tempvpn-macos-manifest.json"
expected_team_id="T4295L8LL4"
expected_package_id="com.tempo.tempvpn.pkg"
expected_app_id="com.tempo.tempvpn"
expected_extension_id="com.tempo.tempvpn.PacketTunnel"

script_dir=$(cd "$(dirname "$0")" && pwd)
verifier="$script_dir/verify-macos-package.sh"
manifest_url=${TEMPVPN_RELEASE_MANIFEST_URL:-$default_manifest_url}
destination=""
completed=false

usage() {
  echo "usage: $0 [--destination DIR] [--manifest-url HTTPS_URL]" >&2
  exit 2
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --destination)
      [[ $# -ge 2 ]] || usage
      destination=$2
      shift 2
      ;;
    --manifest-url)
      [[ $# -ge 2 ]] || usage
      manifest_url=$2
      shift 2
      ;;
    *) usage ;;
  esac
done

[[ "$(uname -s)" == "Darwin" ]] || {
  echo "The macOS bootstrapper only runs on macOS." >&2
  exit 1
}
[[ "$manifest_url" == https://* ]] || {
  echo "Release manifest URL must use HTTPS." >&2
  exit 1
}
for command in curl cut plutil shasum sw_vers uname; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "Required macOS command is missing: $command" >&2
    exit 1
  }
done
[[ -x "$verifier" ]] || {
  echo "Package verifier is missing or not executable: $verifier" >&2
  exit 1
}

base_destination=${destination:-${TMPDIR:-/tmp}}
mkdir -p "$base_destination"
destination=$(mktemp -d "$base_destination/tempvpn-bootstrap.XXXXXX")
chmod 700 "$destination"
cleanup() {
  local status=$?
  if [[ "$completed" != true && -d "$destination" ]]; then
    rm -rf -- "$destination"
  fi
  trap - EXIT
  exit "$status"
}
trap cleanup EXIT
manifest="$destination/tempvpn-macos-manifest.json"

curl --fail --silent --show-error --location --max-redirs 5 \
  --proto '=https' --tlsv1.2 --max-filesize 1048576 "$manifest_url" -o "$manifest"
plutil -lint "$manifest" >/dev/null

read_manifest() {
  plutil -extract "$1" raw -o - "$manifest" 2>/dev/null
}

schema_version=$(read_manifest schema_version)
version=$(read_manifest version)
architecture=$(read_manifest architectures.0)
package_url=$(read_manifest package_url)
sha256=$(read_manifest sha256)
team_id=$(read_manifest team_id)
package_id=$(read_manifest package_identifier)
app_id=$(read_manifest app_bundle_identifier)
extension_id=$(read_manifest extension_bundle_identifier)
minimum_macos=$(read_manifest minimum_macos)

[[ "$schema_version" == "1" ]] || { echo "Unsupported release manifest schema." >&2; exit 1; }
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]] || { echo "Invalid release version." >&2; exit 1; }
[[ "$minimum_macos" =~ ^[0-9]+\.[0-9]+(\.[0-9]+)?$ ]] || { echo "Invalid minimum macOS version." >&2; exit 1; }
[[ "$sha256" =~ ^[0-9a-f]{64}$ ]] || { echo "Invalid package checksum." >&2; exit 1; }
[[ "$team_id" == "$expected_team_id" ]] || { echo "Unexpected Apple team in release manifest." >&2; exit 1; }
[[ "$package_id" == "$expected_package_id" ]] || { echo "Unexpected package identifier in release manifest." >&2; exit 1; }
[[ "$app_id" == "$expected_app_id" ]] || { echo "Unexpected app identifier in release manifest." >&2; exit 1; }
[[ "$extension_id" == "$expected_extension_id" ]] || { echo "Unexpected extension identifier in release manifest." >&2; exit 1; }
[[ "$architecture" == "$(uname -m)" ]] || { echo "No TempVPN release is available for $(uname -m)." >&2; exit 1; }
[[ "$package_url" == "https://github.com/protocolwhisper/tempVPN/releases/download/"* ]] || {
  echo "Package URL is outside the trusted TempVPN GitHub release path." >&2
  exit 1
}

version_at_least() {
  local actual=$1 required=$2 index actual_part required_part
  for index in 1 2 3; do
    actual_part=$(cut -d. -f"$index" <<<"$actual")
    required_part=$(cut -d. -f"$index" <<<"$required")
    actual_part=${actual_part%%[^0-9]*}
    required_part=${required_part%%[^0-9]*}
    ((10#${actual_part:-0} > 10#${required_part:-0})) && return 0
    ((10#${actual_part:-0} < 10#${required_part:-0})) && return 1
  done
  return 0
}

current_macos=$(sw_vers -productVersion)
version_at_least "$current_macos" "$minimum_macos" || {
  echo "TempVPN $version requires macOS $minimum_macos or newer." >&2
  exit 1
}

package_name="TempVPN-${version}-macos-${architecture}.pkg"
expected_package_url="https://github.com/protocolwhisper/tempVPN/releases/download/v${version}/${package_name}"
[[ "$package_url" == "$expected_package_url" ]] || {
  echo "Package URL does not match manifest version and architecture." >&2
  exit 1
}
package="$destination/$package_name"

curl --fail --silent --show-error --location --max-redirs 5 \
  --proto '=https' --tlsv1.2 --max-filesize 536870912 "$package_url" -o "$package"
actual_sha256=$(shasum -a 256 "$package" | awk '{print $1}')
[[ "$actual_sha256" == "$sha256" ]] || {
  echo "Downloaded package checksum does not match the release manifest." >&2
  exit 1
}

verification=$($verifier "$package")
verified_version=$(plutil -extract version raw -o - - <<<"$verification")
[[ "$verified_version" == "$version" ]] || {
  echo "Signed package version does not match the release manifest." >&2
  exit 1
}

completed=true
printf '{"ready_to_install":true,"version":"%s","package_path":"%s","temporary_directory":"%s"}\n' \
  "$version" "$package" "$destination"
