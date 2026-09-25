#!/usr/bin/env python3
"""Compare a live-publisher directory export against a running live-gateway.

Every stored body must equal what the gateway serves for the same request,
apart from wall-clock fields (`generated_at`, `observed_at`). Run summaries are
checked against the last pushed `runs` message.

    compare_publish.py <export-dir> <gateway-url>
"""

import gzip
import json
import sys
import urllib.error
import urllib.request
from pathlib import Path

VOLATILE = {"generated_at", "observed_at"}


def normalize(value):
    if isinstance(value, dict):
        return {key: normalize(item) for key, item in value.items() if key not in VOLATILE}
    if isinstance(value, list):
        return [normalize(item) for item in value]
    return value


def fetch(gateway, path):
    try:
        with urllib.request.urlopen(gateway + path, timeout=300) as response:
            return json.loads(response.read())
    except urllib.error.HTTPError as error:
        return {"__status": error.code}


def gateway_path(key):
    parts = key.split("/")
    if parts[:2] == ["pub", "assets"]:
        return f"/v1/assets/{parts[2]}"
    run = parts[2]
    if parts[3] == "detail.json":
        return f"/v1/runs/{run}"
    if parts[3] == "catalog":
        return f"/v1/runs/{run}?catalog_before={parts[4][:-5]}"
    if parts[3] == "replay":
        cursor = parts[5][:-5]
        after = "" if cursor == "first" else f"&after_sequence={cursor}"
        return f"/v1/runs/{run}?replay_attempt={parts[4]}{after}"
    raise ValueError(key)


def main():
    export, gateway = Path(sys.argv[1]), sys.argv[2].rstrip("/")
    objects = export / "objects"
    keys = sorted(str(path.relative_to(objects)) for path in (objects / "pub").rglob("*") if path.is_file())
    mismatches = []
    counts = {}
    for index, key in enumerate(keys):
        kind = key.split("/")[1] if key.startswith("pub/assets") else key.split("/")[3]
        counts[kind] = counts.get(kind, 0) + 1
        stored = json.loads(gzip.decompress((objects / key).read_bytes()))
        served = fetch(gateway, gateway_path(key))
        if normalize(stored) != normalize(served):
            mismatches.append(key)
            if len(mismatches) <= 5:
                print(f"MISMATCH {key} -> {gateway_path(key)}", flush=True)
        if index % 2000 == 0:
            print(f"checked {index}/{len(keys)}", flush=True)
    # Summaries: fold every pushed runs message, then compare with /v1/runs.
    pushed = {}
    order = []
    for message in sorted((export / "messages").glob("*-runs.json")):
        value = json.loads(message.read_text())
        for run in value["runs"]:
            pushed[run["id"]] = run
        for removed in value["removed"]:
            pushed.pop(removed, None)
        order = value["order"]
    served = fetch(gateway, "/v1/runs")["runs"]
    summaries_match = normalize([pushed[run_id] for run_id in order]) == normalize(served)
    # Every run whose detail the gateway can serve must have been stored.
    missing_details = [
        run["id"] for run in served
        if not (objects / "pub/runs" / run["id"] / "detail.json").is_file()
        and "__status" not in fetch(gateway, f"/v1/runs/{run['id']}")
    ]
    print(json.dumps({
        "bodies": len(keys),
        "by_kind": counts,
        "mismatches": len(mismatches),
        "runs": len(served),
        "summaries_match": summaries_match,
        "missing_details": missing_details,
    }, indent=2))
    sys.exit(0 if not mismatches and summaries_match and not missing_details else 1)


if __name__ == "__main__":
    main()
