#!/bin/bash

set -euo pipefail

game_dir=$(cd "$(dirname "$0")" && pwd)
repo_root=$(cd "$game_dir/../.." && pwd)
data_dir="$game_dir/data"
image=swarm-harbor:test
resume_dir=$(mktemp -d "${TMPDIR:-/tmp}/swarm-resume-test.XXXXXX")
source_container="swarm-resume-source-$$"
target_container="swarm-resume-target-$$"

cleanup() {
  docker rm -f "$source_container" "$target_container" >/dev/null 2>&1 || true
  rm -rf "$resume_dir"
}
trap cleanup EXIT

wait_for_server() {
  local port=$1
  local attempts=0
  until curl -fsS "http://127.0.0.1:$port/v1/status" >/dev/null 2>&1; do
    attempts=$((attempts + 1))
    if [ "$attempts" -ge 100 ]; then
      echo "Swarm test server did not become ready" >&2
      return 1
    fi
    sleep 0.1
  done
}

docker build -t "$image" -f "$game_dir/Dockerfile" "$repo_root"

oracle=$(
  docker run --rm \
    -v "$data_dir/oracle:/oracle:ro" \
    "$image" \
    oracle /opt/swarm-task/farming.yaml /oracle/farming.sw 1000000
)
printf '%s\n' "$oracle" | grep -Fxq "score: 997627"

repeated=$(
  docker run --rm \
    -v "$data_dir/oracle:/oracle:ro" \
    "$image" \
    oracle /opt/swarm-task/farming.yaml /oracle/farming.sw 1000000
)
test "$oracle" = "$repeated"

unsolved=$(
  docker run --rm \
    -v "$data_dir/testdata:/testdata:ro" \
    "$image" \
    verify /opt/swarm-task/farming.yaml /testdata/unsolved-audit.jsonl 1000000
)
printf '%s\n' "$unsolved" | grep -Fxq "score: 0"

if docker run --rm \
  -v "$data_dir/testdata:/testdata:ro" \
  "$image" \
  verify /opt/swarm-task/farming.yaml /testdata/malformed-audit.jsonl 1000000; then
  echo "malformed audit unexpectedly verified" >&2
  exit 1
fi

if docker run --rm \
  -v "$data_dir/testdata:/testdata:ro" \
  "$image" \
  verify /opt/swarm-task/farming.yaml /testdata/tampered-audit.jsonl 1000000; then
  echo "tampered audit unexpectedly verified" >&2
  exit 1
fi

mkdir -p "$resume_dir/source" "$resume_dir/target"
docker run -d -P --name "$source_container" \
  -v "$resume_dir/source:/audit" \
  "$image" \
  serve /opt/swarm-task/farming.yaml /audit/audit.jsonl 3720 1000000 \
  >/dev/null
source_port=$(docker port "$source_container" 3720/tcp | head -n 1 | sed 's/.*://')
wait_for_server "$source_port"
curl -fsS -X POST \
  "http://127.0.0.1:$source_port/v1/advance/123" \
  >/dev/null
curl -fsS "http://127.0.0.1:$source_port/v1/status" >/dev/null

docker run -d -P --name "$target_container" \
  -v "$resume_dir/target:/audit" \
  "$image" \
  serve /opt/swarm-task/farming.yaml /audit/audit.jsonl 3720 1000000 \
  >/dev/null
target_port=$(docker port "$target_container" 3720/tcp | head -n 1 | sed 's/.*://')
wait_for_server "$target_port"
cp "$resume_dir/source/audit.jsonl" "$resume_dir/target/audit.jsonl"
restored=$(curl -fsS "http://127.0.0.1:$target_port/v1/status")
printf '%s\n' "$restored" | grep -Fq '"tick":123'

replayed=$(
  docker run --rm \
    -v "$resume_dir/target:/audit:ro" \
    "$image" \
    verify /opt/swarm-task/farming.yaml /audit/audit.jsonl 1000000
)
printf '%s\n' "$replayed" | grep -Fxq "ticks: 123"

echo "Swarm adapter checks passed"
