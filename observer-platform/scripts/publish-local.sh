#!/bin/sh
set -eu

project_dir=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
repo_root=$(CDPATH= cd -- "$project_dir/.." && pwd)
release_root=${LIVE_RELEASE_ROOT:-"$HOME/.local/share/3720-benchmark-live"}
launch_agent="$HOME/Library/LaunchAgents/org.3720.benchmark-live-site.plist"
gateway_launch_agent="$HOME/Library/LaunchAgents/org.3720.benchmark-live-gateway.plist"
gateway_launch_agent_template="$project_dir/deploy/org.3720.benchmark-live-gateway.plist.in"
gateway_log="$HOME/.cloudflared/benchmark-live-gateway.log"
uid=$(id -u)

test -f "$project_dir/dist/server/index.js"
test -d "$project_dir/dist/client/assets"
test -d "$project_dir/node_modules"
test -f "$gateway_launch_agent_template"

cargo build --quiet --release \
  --manifest-path "$repo_root/tools/observer/runtime/Cargo.toml" \
  --bins

release_id=$(date -u +%Y%m%dT%H%M%SZ)-$$
staging="$release_root/.staging-$release_id"
release="$release_root/releases/$release_id"
link="$release_root/.current-$release_id"

mkdir -p "$release_root/releases"
rm -rf "$staging" "$link"
mkdir -p "$staging/bin"
cp -R "$project_dir/dist" "$staging/dist"
cp "$project_dir/package.json" "$staging/package.json"
install -m 755 \
  "$repo_root/tools/observer/runtime/target/release/live-gateway" \
  "$staging/bin/live-gateway"
ln -s "$project_dir/node_modules" "$staging/node_modules"

# The edge caches HTML briefly. Keep prior content-addressed chunks in the new
# release so an older cached document can still load during a deployment.
if test -d "$release_root/current/dist/client/assets"; then
  cp -R "$release_root/current/dist/client/assets/." "$staging/dist/client/assets/"
fi

mv "$staging" "$release"
ln -s "releases/$release_id" "$link"
mv -fh "$link" "$release_root/current"

mkdir -p "$(dirname "$gateway_launch_agent")" "$(dirname "$gateway_log")"
gateway_launch_agent_staging="$gateway_launch_agent.$$.tmp"
sed \
  -e "s|@GATEWAY@|$release_root/current/bin/live-gateway|g" \
  -e "s|@ROOT@|$repo_root|g" \
  -e "s|@LOG@|$gateway_log|g" \
  "$gateway_launch_agent_template" > "$gateway_launch_agent_staging"
plutil -lint "$gateway_launch_agent_staging" >/dev/null
mv "$gateway_launch_agent_staging" "$gateway_launch_agent"

restart_launch_agent() {
  label=$1
  plist=$2
  service="gui/$uid/$label"
  # `kickstart` reuses launchd's already-loaded definition. Reload the plist
  # so runtime migrations (for example Python gateway -> Rust gateway) take
  # effect during the same release.
  launchctl bootout "$service" >/dev/null 2>&1 || true
  launchctl enable "$service"
  launchctl bootstrap "gui/$uid" "$plist"
}

if test -f "$launch_agent"; then
  restart_launch_agent org.3720.benchmark-live-site "$launch_agent"
fi
if test -f "$gateway_launch_agent"; then
  restart_launch_agent org.3720.benchmark-live-gateway "$gateway_launch_agent"
fi

printf '%s\n' "$release"
