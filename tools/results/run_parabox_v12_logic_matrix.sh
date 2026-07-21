#!/bin/bash

set -u

repo_root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$repo_root"
export PYTHONPATH="$repo_root${PYTHONPATH:+:$PYTHONPATH}"

logs_dir=".harbor/launch-logs/parabox-v12-logic"
mkdir -p "$logs_dir"

group=${PARABOX_MATRIX_GROUP:-all}
run_suffix=${PARABOX_RUN_SUFFIX:-r1}
stagger_seconds=${PARABOX_LAUNCH_STAGGER_SECONDS:-5}
max_concurrent=${PARABOX_MAX_CONCURRENT:-3}
goal_objective="Manually solve all 364 puzzles in this Patrick's Parabox campaign using only the displayed game state, deliberate logical reasoning, and reusable mechanics you discover yourself. Do not write or run a solver, search script, automated planner, or any program that generates, tests, or chooses moves; do not retrieve external answers. Make your greatest possible effort: a partial score, a difficult puzzle, or a submit checkpoint is not completion. Keep reasoning, use deliberate undo/restart, update concise notes after every material discovery, and work through other unlocked branches when useful. Continue until /usr/local/bin/parabox submit verifies score 364/364 or the task runtime ends."

environment_hosts=(
  --allow-environment-host deb.debian.org
  --allow-environment-host raw.githubusercontent.com
  --allow-environment-host nodejs.org
  --allow-environment-host registry.npmjs.org
  --allow-environment-host downloads.claude.ai
  --allow-environment-host qoder.com.cn
  --allow-environment-host static.qoder.com.cn
)
codex_hosts=(
  --allow-agent-host chatgpt.com
  --allow-agent-host api.openai.com
)

pids=()
names=()
finished=()
wave=()
status=0

launch() {
  local name=$1
  shift
  harbor run \
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
  wave+=("$((${#pids[@]} - 1))")
  printf 'started %-58s pid=%s\n' "$name" "$!"
  sleep "$stagger_seconds"
  if [ "${#wave[@]}" -ge "$max_concurrent" ]; then
    for index in "${wave[@]}"; do
      wait_for_index "$index"
    done
    wave=()
  fi
}

wait_for_index() {
  local index=$1
  if wait "${pids[$index]}"; then
    printf 'finished %-57s ok\n' "${names[$index]}"
  else
    local code=$?
    printf 'finished %-57s exit=%s\n' "${names[$index]}" "$code"
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
      "parabox-v12-logic-deepseek-$model-$run_suffix" \
      --env-file .env \
      --max-retries 2 \
      --retry-include NetworkConnectionError \
      --agent-timeout-multiplier "${PARABOX_DS_TIMEOUT_MULTIPLIER:-1.0}" \
      -a tools.agents.deepseek_claude_code:GoalDeepSeekClaudeCode \
      -m "deepseek-v4-$model" \
      --ak reasoning_effort=xhigh \
      --ak "goal_objective=$goal_objective" \
      --ak disallowed_tools=WebSearch,WebFetch \
      --allow-agent-host api.deepseek.com \
      --agent-include-logs "sessions/**" \
      --agent-include-logs claude-code.txt
  done
fi

if [ "$group" = all ] || [ "$group" = kimi ]; then
  launch \
    "parabox-v12-logic-kimi-k3-1m-max-$run_suffix" \
    --env-file .env \
    --max-retries 2 \
    --retry-include NetworkConnectionError \
    -a tools.agents.kimi_claude_code:GoalKimiClaudeCode \
    -m "k3[1m]" \
    --ak reasoning_effort=max \
    --ak credential_label=later-user-supplied-key \
    --ak "goal_objective=$goal_objective" \
    --ak disallowed_tools=WebSearch,WebFetch \
    --allow-agent-host api.kimi.com \
    --agent-include-logs "sessions/**" \
    --agent-include-logs claude-code.txt
fi

if [ "$group" = all ] || [ "$group" = qoder ]; then
  # Qoder CN 1.1.0 persists the GLM-5.2 selector as backend ID `gm51model`.
  for model_spec in \
    "qwen3-8-max-preview|Qwen3.8-Max-Preview" \
    "glm-5-2|gm51model"; do
    model_slug=${model_spec%%|*}
    model_name=${model_spec#*|}
    launch \
      "parabox-v12-logic-qoder-cn-$model_slug-max-$run_suffix" \
      -a tools.agents.qoder_cli_cn:QoderCliCn \
      -m "$model_name" \
      --ak reasoning_effort=max \
      --ak "auth_dir=$HOME/.qoder-cn/.auth" \
      --allow-agent-host gateway.qoder.com.cn \
      --allow-agent-host openapi.qoder.com.cn \
      --agent-include-logs qoder-cn.jsonl \
      --agent-include-logs qoder-cn.stderr \
      --agent-include-logs qoder-cn-supervisor.jsonl
  done
fi

if [ "$group" = all ] || [ "$group" = gpt ]; then
  for family in luna terra sol; do
    launch \
      "parabox-v12-logic-gpt-$family-xhigh-$run_suffix" \
      -a tools.agents.isolated_codex:GoalIsolatedCodex \
      -m "openai/gpt-5.6-$family" \
      --ae "CODEX_AUTH_JSON_PATH=$HOME/.codex/auth.json" \
      --ak reasoning_effort=xhigh \
      --ak "goal_objective=$goal_objective" \
      --ak web_search=disabled \
      --ak version=0.144.0 \
      "${codex_hosts[@]}" \
      --agent-include-logs "sessions/**" \
      --agent-include-logs codex.txt
  done
fi

if [ "${#wave[@]}" -gt 0 ]; then
  for index in "${wave[@]}"; do
    wait_for_index "$index"
  done
fi

exit "$status"
