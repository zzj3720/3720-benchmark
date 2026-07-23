#!/bin/bash

set -u

repo_root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$repo_root"
export PYTHONPATH="$repo_root${PYTHONPATH:+:$PYTHONPATH}"

logs_dir=".harbor/launch-logs/parabox-v9-notes"
mkdir -p "$logs_dir"

group=${PARABOX_MATRIX_GROUP:-all}
run_suffix=${PARABOX_RUN_SUFFIX:-}
codex_stagger_seconds=${PARABOX_CODEX_STAGGER_SECONDS:-20}
codex_wave_size=${PARABOX_CODEX_WAVE_SIZE:-3}
codex_efforts=${PARABOX_CODEX_EFFORTS:-"low medium high xhigh"}

environment_hosts=(
  --allow-environment-host deb.debian.org
  --allow-environment-host raw.githubusercontent.com
  --allow-environment-host nodejs.org
  --allow-environment-host registry.npmjs.org
)
codex_hosts=(
  --allow-agent-host chatgpt.com
  --allow-agent-host api.openai.com
)

pids=()
names=()
finished=()
status=0

launch() {
  local name=$1
  shift
  tools/observer/harbor-run \
    -p tasks/parabox-intro \
    --jobs-dir .harbor/jobs \
    --job-name "$name" \
    -n 1 \
    -y \
    "${environment_hosts[@]}" \
    "$@" >"$logs_dir/$name.log" 2>&1 &
  pids+=("$!")
  names+=("$name")
  finished+=(0)
  printf 'started %-52s pid=%s\n' "$name" "$!"
}

wait_for_index() {
  local index=$1
  if wait "${pids[$index]}"; then
    printf 'finished %-51s ok\n' "${names[$index]}"
  else
    local code=$?
    printf 'finished %-51s exit=%s\n' "${names[$index]}" "$code"
    status=1
  fi
  finished[$index]=1
}

stop_children() {
  trap - INT TERM
  local index
  for index in "${!pids[@]}"; do
    if [ "${finished[$index]:-0}" -eq 0 ]; then
      kill -INT "${pids[$index]}" 2>/dev/null || true
    fi
  done
  wait
  exit 130
}
trap stop_children INT TERM

if [ "$group" = all ] || [ "$group" = deepseek ]; then
  for model in flash pro; do
    launch \
      "parabox-v9-notes-deepseek-$model$run_suffix" \
      --env-file .env \
      -a terminus-2 \
      -m "deepseek/deepseek-v4-$model" \
      --agent-include-logs trajectory.json \
      --agent-include-logs recording.cast \
      --agent-include-logs terminus_2.pane
  done
  for index in "${!pids[@]}"; do
    wait_for_index "$index"
  done
fi

if [ "$group" = all ] || [ "$group" = codex ]; then
  wave=()
  for effort in $codex_efforts; do
    for family in luna terra sol; do
      launch \
        "parabox-v9-notes-codex-$family-$effort-isolated$run_suffix" \
        -a tools.agents.isolated_codex:IsolatedCodex \
        -m "openai/gpt-5.6-$family" \
        --ae "CODEX_AUTH_JSON_PATH=$HOME/.codex/auth.json" \
        --ak "reasoning_effort=$effort" \
        --ak web_search=disabled \
        --ak version=0.144.0 \
        "${codex_hosts[@]}" \
        --agent-include-logs "sessions/**" \
        --agent-include-logs codex.txt
      wave+=("$((${#pids[@]} - 1))")
      sleep "$codex_stagger_seconds"
      if [ "${#wave[@]}" -ge "$codex_wave_size" ]; then
        for index in "${wave[@]}"; do
          wait_for_index "$index"
        done
        wave=()
      fi
    done
  done
  for index in "${wave[@]}"; do
    wait_for_index "$index"
  done
fi

exit "$status"
