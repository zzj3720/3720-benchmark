#!/bin/sh
# Install the live publisher as a per-user LaunchAgent on the benchmark host.
#
#   observer/runtime/scripts/install_publisher.sh <endpoint> <token-file>
#
# The endpoint is the deployed console origin (for example
# https://live.benchmark.3720.org); the token file holds LIVE_INGEST_TOKEN.
set -eu

endpoint=${1:?usage: install_publisher.sh <endpoint> <token-file>}
token_file=${2:?usage: install_publisher.sh <endpoint> <token-file>}
root=$(CDPATH= cd -- "$(dirname "$0")/../../.." && pwd)
label=org.3720.benchmark-live-publisher
plist="$HOME/Library/LaunchAgents/$label.plist"
log="$HOME/Library/Logs/benchmark-live-publisher.log"

cargo build --release --locked --manifest-path "$root/observer/runtime/Cargo.toml" --bin live-publisher
publisher="$root/observer/runtime/target/release/live-publisher"

sed -e "s#@PUBLISHER@#$publisher#g" -e "s#@ROOT@#$root#g" -e "s#@ENDPOINT@#$endpoint#g" \
    -e "s#@TOKEN_FILE@#$token_file#g" -e "s#@LOG@#$log#g" \
    "$root/observer/runtime/deploy/$label.plist.in" > "$plist"

launchctl bootout "gui/$(id -u)/$label" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "$plist"
echo "Installed $label; logs in $log"
