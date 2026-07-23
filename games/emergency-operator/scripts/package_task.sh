#!/bin/bash

set -euo pipefail

repo_root=$(cd "$(dirname "$0")/../../.." && pwd)
game_dir="$repo_root/games/emergency-operator"
data_dir="$game_dir/data"
task_dir="$repo_root/tasks/emergency-operator"
staging=$(mktemp -d "${TMPDIR:-/tmp}/operator-package.XXXXXX")
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
    "$source_dir/operator-server" \
    "$task_dir/environment/game/bin/$architecture/operator-server"
  install -d "$task_dir/environment/bin/$architecture"
  install -m 755 \
    "$source_dir/operator" \
    "$task_dir/environment/bin/$architecture/operator"

  install -d "$task_dir/tests/bin/$architecture"
  install -m 755 \
    "$source_dir/operator-verifier" \
    "$task_dir/tests/bin/$architecture/operator-verifier"
done

for destination in environment/game/campaign tests/campaign; do
  install -d "$task_dir/$destination"
  rm -f "$task_dir/$destination/pilot.json"
  install -m 644 \
    "$data_dir/campaign/911-career.json" \
    "$task_dir/$destination/911-career.json"
  install -m 644 \
    "$data_dir/campaign/source.json" \
    "$task_dir/$destination/source.json"
done

echo "Packaged emergency-operator for linux/amd64 and linux/arm64"
