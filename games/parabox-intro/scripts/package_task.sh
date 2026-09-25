#!/bin/bash
set -euo pipefail

repo_root=$(cd "$(dirname "$0")/../../.." && pwd)
game_dir="$repo_root/games/parabox-intro"
task_dir="$repo_root/tasks/parabox-intro"
staging=$(mktemp -d "${TMPDIR:-/tmp}/parabox-package.XXXXXX")
relay_staging=$(mktemp -d "${TMPDIR:-/tmp}/observer-relay-package.XXXXXX")
trap 'rm -rf "$staging" "$relay_staging"' EXIT

docker buildx build \
  --progress plain \
  --platform linux/amd64,linux/arm64 \
  --output "type=local,dest=$staging" \
  --file "$game_dir/Dockerfile" \
  "$repo_root"

docker buildx build \
  --progress plain \
  --platform linux/amd64,linux/arm64 \
  --output "type=local,dest=$relay_staging" \
  "$repo_root/observer/relay"

for architecture in amd64 arm64; do
  source_dir="$staging/linux_$architecture/$architecture"
  install -d "$task_dir/environment/bin/$architecture"
  install -m 755 \
    "$source_dir/parabox" \
    "$task_dir/environment/bin/$architecture/parabox"

  install -d "$task_dir/environment/game/bin/$architecture"
  install -m 755 \
    "$source_dir/parabox-server" \
    "$task_dir/environment/game/bin/$architecture/parabox-server"
  install -m 755 \
    "$relay_staging/linux_$architecture/$architecture/observer-relay" \
    "$task_dir/environment/game/bin/$architecture/observer-relay"

  install -d "$task_dir/tests/bin/$architecture"
  install -m 755 \
    "$source_dir/parabox-verifier" \
    "$task_dir/tests/bin/$architecture/parabox-verifier"
done

if [ -d "$task_dir/environment/campaign" ]; then
  find "$task_dir/environment/campaign" -depth -delete
fi
install -d "$task_dir/environment/game/campaign/levels"
install -d "$task_dir/tests/campaign/levels"
find "$task_dir/environment/game/campaign/levels" -type f -name '*.level' -delete
find "$task_dir/tests/campaign/levels" -type f -name '*.level' -delete
data_dir="$game_dir/data"
for level in "$data_dir"/campaign/levels/*.level; do
  install -m 644 "$level" "$task_dir/environment/game/campaign/levels/"
  install -m 644 "$level" "$task_dir/tests/campaign/levels/"
done
for metadata in source.json index.tsv areas.tsv; do
  install -m 644 \
    "$data_dir/campaign/$metadata" \
    "$task_dir/environment/game/campaign/$metadata"
  install -m 644 \
    "$data_dir/campaign/$metadata" \
    "$task_dir/tests/campaign/$metadata"
done
install -m 644 \
  "$data_dir/oracle/oracle.tsv" \
  "$task_dir/solution/oracle.tsv"

echo "Packaged parabox-intro for linux/amd64 and linux/arm64"
