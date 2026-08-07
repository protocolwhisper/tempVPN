#!/bin/bash
set -euo pipefail

script_dir=$(cd "$(dirname "$0")" && pwd)

"$script_dir/build-headless-app.sh"
"$script_dir/build-tempvpnctl.sh"

echo "macOS products are in tempvpn/target: TempVPN.app and tempvpnctl"
