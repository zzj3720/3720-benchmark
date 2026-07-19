#!/bin/bash

# Reject Harbor's legacy allow_internet key. Use network_mode so policy is
# explicit and can represent offline, allowlisted, and public tracks.

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

if "allow_internet" in data.get("environment", {}):
    print("legacy [environment].allow_internet is not allowed; use network_mode")
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
    echo "One or more tasks use Harbor's legacy allow_internet key."
    exit 1
fi

echo "All task.toml files use the current network policy schema"
