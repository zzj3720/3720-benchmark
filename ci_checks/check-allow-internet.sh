#!/bin/bash

# Validate Harbor's explicit runtime network policy.
#
# Tracks may use "public", "allowlist", or "no-network" according to their
# requirements and Harbor backend support. Allowlist mode must name a host.

set -e

if [ $# -eq 0 ]; then
    FILES_TO_CHECK=$(find tasks -type f -name "task.toml")
else
    FILES_TO_CHECK=""
    for task_dir in "$@"; do
        if [ -d "$task_dir" ] && [ -f "$task_dir/task.toml" ]; then
            FILES_TO_CHECK="$FILES_TO_CHECK $task_dir/task.toml"
        fi
    done
fi

if [ -z "$FILES_TO_CHECK" ]; then
    echo "No task.toml files to check"
    exit 0
fi

FAILED=0
for file in $FILES_TO_CHECK; do
    echo "Checking $file..."

    RESULT=$(python3 - "$file" <<'PYEOF'
import sys

path = sys.argv[1]

try:
    import tomllib
except ModuleNotFoundError:
    import tomli as tomllib  # type: ignore

with open(path, "rb") as f:
    data = tomllib.load(f)

environment = data.get("environment", {})
mode = environment.get("network_mode")

if mode is None:
    print('missing [environment].network_mode; use "no-network", "allowlist", or "public"')
    sys.exit(1)

if mode not in {"no-network", "allowlist", "public"}:
    print(f"invalid [environment].network_mode={mode!r}")
    sys.exit(1)

if mode == "allowlist" and not environment.get("allowed_hosts"):
    print('[environment].network_mode="allowlist" requires allowed_hosts')
    sys.exit(1)

PYEOF
    ) || {
        while IFS= read -r line; do
            [ -n "$line" ] && echo "FAIL $file: $line"
        done <<< "$RESULT"
        FAILED=1
    }
done

if [ $FAILED -eq 1 ]; then
    echo ""
    echo "One or more tasks have an invalid runtime network policy."
    exit 1
fi

echo "All task.toml files declare a valid runtime network policy"
