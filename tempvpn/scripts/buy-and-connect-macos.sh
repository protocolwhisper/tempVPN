#!/bin/bash
set -euo pipefail

usage() {
  echo "Usage: $0 <duration, e.g. 30m>"
}

if [[ ${1:-} == "--help" || ${1:-} == "-h" ]]; then
  usage
  exit 0
fi

duration=${1:-30m}
if [[ ! $duration =~ ^([0-9]+)([smh]?)$ ]]; then
  echo "Invalid duration: $duration" >&2
  usage >&2
  exit 2
fi

duration_seconds=${BASH_REMATCH[1]}
case ${BASH_REMATCH[2]} in
  m) duration_seconds=$((duration_seconds * 60)) ;;
  h) duration_seconds=$((duration_seconds * 3600)) ;;
esac

if (( duration_seconds <= 0 )); then
  echo "Duration must be greater than zero" >&2
  exit 2
fi

if [[ $(uname -s) != "Darwin" ]]; then
  echo "This demo launcher requires macOS" >&2
  exit 2
fi

for command_name in wg mppx osascript; do
  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "Missing required command: $command_name" >&2
    exit 1
  fi
done

script_dir=$(cd "$(dirname "$0")" && pwd)
repo_dir=$(cd "$script_dir/.." && pwd)
client="$repo_dir/target/debug/vpn-client"
admin_script="$script_dir/connect-with-admin.applescript"

if [[ ! -x "$client" ]]; then
  echo "Missing VPN client: $client" >&2
  echo "Build it with: cargo build -p vpn-client-cli" >&2
  exit 1
fi

# MPPX accounts live in macOS Keychain. When this launcher is invoked from a
# sandbox, account discovery can look empty even though the user's real
# Keychain contains `main`. Fail safely and never suggest creating/replacing an
# account from this path: some account-creation failures can print generated
# private-key material.
if ! mppx account view --account main >/dev/null 2>&1; then
  echo "MPPX account 'main' is unavailable in the current macOS Keychain context." >&2
  echo "Rerun this launcher with host/Keychain access; do not create a replacement account automatically." >&2
  exit 1
fi

work_dir=$(mktemp -d /tmp/tempvpn-demo.XXXXXX)
private_key="$work_dir/client.key"
public_key="$work_dir/client.pub"
session_response="$work_dir/session.json"
cleanup() {
  rm -rf "$work_dir"
}
trap cleanup EXIT
umask 077

wg genkey > "$private_key"
wg pubkey < "$private_key" > "$public_key"
client_public_key=$(tr -d '\r\n' < "$public_key")
request_body=$(printf '{"client_public_key":"%s","duration_seconds":%s}' \
  "$client_public_key" "$duration_seconds")

echo "Buying $duration of VPN access with Tempo..."
set +e
mppx http://34.30.107.52:8080/sessions \
  --account main \
  --network testnet \
  --json-body "$request_body" \
  --verbose > "$session_response"
payment_status=$?
set -e
if (( payment_status != 0 )); then
  cat "$session_response" >&2
  exit "$payment_status"
fi

echo "Payment succeeded. Approve the macOS administrator dialog to connect."
osascript "$admin_script" "$client" "$session_response" "$private_key"

echo "Verifying VPN connection..."
"$client" status
