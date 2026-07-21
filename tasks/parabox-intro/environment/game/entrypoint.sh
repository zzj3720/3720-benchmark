#!/bin/sh
set -eu

/usr/local/bin/parabox-server &
game_pid=$!

/usr/local/bin/observer-relay &
observer_pid=$!

cleanup() {
  kill "$game_pid" "$observer_pid" 2>/dev/null || true
  wait "$game_pid" "$observer_pid" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

while kill -0 "$game_pid" 2>/dev/null && kill -0 "$observer_pid" 2>/dev/null; do
  sleep 1
done
exit 1
