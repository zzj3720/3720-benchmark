#!/bin/bash

set -euo pipefail

repo_root=$(cd "$(dirname "$0")/../../.." && pwd)
game_dir="$repo_root/games/sokoban"
data_dir="$game_dir/data"
task_dir="$repo_root/tasks/sokoban"
staging=$(mktemp -d "${TMPDIR:-/tmp}/sokoban-package.XXXXXX")
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
    "$source_dir/sokoban-server" \
    "$task_dir/environment/game/bin/$architecture/sokoban-server"
  install -d "$task_dir/environment/bin/$architecture"
  install -m 755 \
    "$source_dir/sokoban" \
    "$task_dir/environment/bin/$architecture/sokoban"

  install -d "$task_dir/tests/bin/$architecture"
  install -m 755 \
    "$source_dir/sokoban-verifier" \
    "$task_dir/tests/bin/$architecture/sokoban-verifier"
done

for destination in environment/game/campaign tests/campaign; do
  install -d "$task_dir/$destination"
  install -m 644 \
    "$data_dir/campaign/sokoban.json" \
    "$task_dir/$destination/sokoban.json"
  install -m 644 \
    "$data_dir/campaign/source.json" \
    "$task_dir/$destination/source.json"
done

echo "Packaged sokoban for linux/amd64 and linux/arm64"
