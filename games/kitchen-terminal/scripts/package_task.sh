#!/bin/bash

set -euo pipefail

repo_root=$(cd "$(dirname "$0")/../../.." && pwd)
game_dir="$repo_root/games/kitchen-terminal"
data_dir="$game_dir/data/overcooked-1"
task_dir="$repo_root/tasks/kitchen-terminal"
staging=$(mktemp -d "${TMPDIR:-/tmp}/kitchen-package.XXXXXX")
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
    "$source_dir/kitchen-server" \
    "$task_dir/environment/game/bin/$architecture/kitchen-server"
  install -d "$task_dir/environment/bin/$architecture"
  install -m 755 \
    "$source_dir/kitchen" \
    "$task_dir/environment/bin/$architecture/kitchen"

  install -d "$task_dir/tests/bin/$architecture"
  install -m 755 \
    "$source_dir/kitchen-verifier" \
    "$task_dir/tests/bin/$architecture/kitchen-verifier"
done

for destination in environment/game/data tests/data; do
  rm -rf "$task_dir/$destination"
  install -d "$task_dir/$destination/levels"
  install -m 644 "$data_dir/campaign.json" "$task_dir/$destination/campaign.json"
  install -m 644 "$data_dir/manifest.json" "$task_dir/$destination/manifest.json"
  install -m 644 "$data_dir/README.md" "$task_dir/$destination/README.md"
  install -m 644 \
    "$data_dir/levels/4P_Hell.json" \
    "$task_dir/$destination/levels/4P_Hell.json"
done

echo "Packaged kitchen-terminal for linux/amd64 and linux/arm64"
