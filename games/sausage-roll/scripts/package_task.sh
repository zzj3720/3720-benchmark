#!/bin/bash

set -euo pipefail

repo_root=$(cd "$(dirname "$0")/../../.." && pwd)
game_dir="$repo_root/games/sausage-roll"
data_dir="$game_dir/data"
task_dir="$repo_root/tasks/sausage-roll"
staging=$(mktemp -d "${TMPDIR:-/tmp}/sausage-package.XXXXXX")
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
    "$source_dir/sausage-server" \
    "$task_dir/environment/game/bin/$architecture/sausage-server"

  install -d "$task_dir/tests/bin/$architecture"
  install -m 755 \
    "$source_dir/sausage-verifier" \
    "$task_dir/tests/bin/$architecture/sausage-verifier"
done

for destination in environment/game/campaign tests/campaign; do
  install -d "$task_dir/$destination"
  install -m 644 \
    "$data_dir/campaign/merged_binary.gz" \
    "$task_dir/$destination/merged_binary.gz"
  install -m 644 \
    "$data_dir/campaign/entries.tar.gz" \
    "$task_dir/$destination/entries.tar.gz"
  install -m 644 \
    "$data_dir/campaign/source.json" \
    "$task_dir/$destination/source.json"
done

install -m 644 \
  "$data_dir/oracle/segments.tar.gz" \
  "$task_dir/solution/segments.tar.gz"

echo "Packaged sausage-roll for linux/amd64 and linux/arm64"
