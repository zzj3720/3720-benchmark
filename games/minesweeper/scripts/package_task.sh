#!/bin/bash

set -euo pipefail

repo_root=$(cd "$(dirname "$0")/../../.." && pwd)
game_dir="$repo_root/games/minesweeper"
data_dir="$game_dir/data"
task_dir="$repo_root/tasks/minesweeper"
staging=$(mktemp -d "${TMPDIR:-/tmp}/minesweeper-package.XXXXXX")
trap 'rm -rf "$staging"' EXIT

docker buildx build \
  --progress plain \
  --platform linux/amd64,linux/arm64 \
  --output "type=local,dest=$staging" \
  "$game_dir"

for architecture in amd64 arm64; do
  source_dir="$staging/linux_$architecture/$architecture"
  install -d "$task_dir/environment/game/bin/$architecture"
  install -m 755 \
    "$source_dir/minesweeper-server" \
    "$task_dir/environment/game/bin/$architecture/minesweeper-server"
  install -d "$task_dir/environment/bin/$architecture"
  install -m 755 \
    "$source_dir/minesweeper" \
    "$task_dir/environment/bin/$architecture/minesweeper"
  install -d "$task_dir/tests/bin/$architecture"
  install -m 755 \
    "$source_dir/minesweeper-verifier" \
    "$task_dir/tests/bin/$architecture/minesweeper-verifier"
done

for destination in environment/game/campaign tests/campaign; do
  install -d "$task_dir/$destination"
  install -m 644 \
    "$data_dir/campaign/minesweeper.json" \
    "$task_dir/$destination/minesweeper.json"
  install -m 644 \
    "$data_dir/campaign/source.json" \
    "$task_dir/$destination/source.json"
done

echo "Packaged minesweeper for linux/amd64 and linux/arm64"
