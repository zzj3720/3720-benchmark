#!/bin/sh
# Install the cover baker as a per-user LaunchAgent on the benchmark host: every
# 15 minutes it bakes covers for attempts the console shows but has none for.
#
#   observer/web/scripts/install_cover_baker.sh <site> <token-file>
set -eu

site=${1:?usage: install_cover_baker.sh <site> <token-file>}
token_file=${2:?usage: install_cover_baker.sh <site> <token-file>}
web=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
node=$(command -v node)
label=org.3720.benchmark-cover-baker
plist="$HOME/Library/LaunchAgents/$label.plist"
log="$HOME/Library/Logs/benchmark-cover-baker.log"

cat > "$plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$label</string>
  <key>ProgramArguments</key>
  <array>
    <string>$node</string>
    <string>$web/scripts/bake-covers.mjs</string>
    <string>--site</string><string>$site</string>
    <string>--token-file</string><string>$token_file</string>
  </array>
  <key>WorkingDirectory</key><string>$web</string>
  <key>StartInterval</key><integer>900</integer>
  <key>RunAtLoad</key><true/>
  <key>StandardOutPath</key><string>$log</string>
  <key>StandardErrorPath</key><string>$log</string>
</dict>
</plist>
PLIST

domain="gui/$(id -u)"
launchctl bootout "$domain/$label" 2>/dev/null || true
for _ in 1 2 3 4 5 6 7 8 9 10; do
  launchctl print "$domain/$label" >/dev/null 2>&1 || break
  sleep 1
done
launchctl bootstrap "$domain" "$plist" || { sleep 2; launchctl bootstrap "$domain" "$plist"; }
echo "Installed $label; logs in $log"
