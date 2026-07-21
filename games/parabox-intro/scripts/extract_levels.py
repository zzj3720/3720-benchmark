#!/usr/bin/env python3
"""Extract legally installed Parabox TextAssets into a local benchmark task."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path

import UnityPy


APP_ID = "1260520"
PINNED_BUILD_ID = "8556490"
AREA_ORDER = (
    ("Area_Intro", "Intro", "start", "-", 0, 0, True),
    ("Area_Enter", "Enter", "gate", "Area_Intro", 0, 2, True),
    ("Area_Empty", "Empty", "gate", "Area_Enter", 0, 2, True),
    ("Area_Eat", "Eat", "gate", "Area_Empty", 0, 2, True),
    ("Area_Reference", "Reference", "gate", "Area_Eat", 0, 2, True),
    ("Area_Swap", "Swap", "gate", "Area_Reference", -1, 2, True),
    ("Area_Center", "Center", "gate", "Area_Swap", 0, 2, True),
    ("Area_Clone", "Clone", "gate", "Area_Center", 0, 2, True),
    ("Area_Transfer", "Transfer", "gate", "Area_Clone", 0, 2, True),
    ("Area_Open", "Open", "gate", "Area_Transfer", 0, 2, True),
    ("Area_Flip", "Flip", "gate", "Area_Open", 0, 2, True),
    ("Area_Cycle", "Cycle", "gate", "Area_Flip", 0, 2, True),
    ("Area_Player", "Player", "gate", "Area_Cycle", 0, 2, True),
    ("Area_Possess", "Possess", "gate", "Area_Player", 0, 2, True),
    ("Area_Wall", "Wall", "gate", "Area_Possess", -1, 2, True),
    (
        "Area_InfiniteExit",
        "Infinite Exit",
        "gate",
        "Area_Wall",
        0,
        2,
        True,
    ),
    (
        "Area_InfiniteEnter",
        "Infinite Enter",
        "gate",
        "Area_InfiniteExit",
        0,
        2,
        True,
    ),
    (
        "Area_MultiInfinite",
        "Multi Infinite",
        "gate",
        "Area_InfiniteEnter",
        0,
        2,
        False,
    ),
    ("Area_Challenge", "Challenge", "nexus", "r11", 0, 2, False),
    ("Area_Gallery", "Gallery", "nexus", "r11", 0, 2, False),
    ("Area_Priority", "Priority", "nexus", "r11", 0, 1, False),
    ("Area_Extrude", "Extrude", "nexus", "r11", 0, 1, False),
    ("Area_Push", "Push", "nexus", "r11", 0, 1, False),
)
AREA_INDEX = {area: index for index, (area, *_rest) in enumerate(AREA_ORDER)}
KINDS = ("core", "challenge", "side")
DEFAULT_APP = (
    Path.home()
    / "Library/Application Support/Steam/steamapps/common"
    / "Patrick's Parabox/Patrick's Parabox.app"
)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def read_manifest(path: Path) -> dict[str, str]:
    values = dict(re.findall(r'"([^"]+)"\s+"([^"]*)"', path.read_text()))
    if values.get("appid") != APP_ID:
        raise ValueError(f"expected Steam app {APP_ID}, found {values.get('appid')}")
    return values


def text_assets(resources: Path) -> dict[str, str]:
    assets = {}
    for obj in UnityPy.load(str(resources)).objects:
        if obj.type.name != "TextAsset":
            continue
        data = obj.read()
        script = data.m_Script
        if isinstance(script, (bytes, bytearray)):
            script = script.decode("utf-8")
        assets[data.m_Name] = script
    return assets


def hub_areas(hub: str) -> dict[str, str]:
    """Return puzzle asset -> original hub area from the serialized hub."""
    levels: dict[int, dict[str, object]] = {}
    stack: list[int] = []
    references: list[list[str]] = []
    in_objects = False

    for raw_line in hub.replace("\r", "").splitlines():
        if not raw_line:
            continue
        if not in_objects:
            in_objects = raw_line == "#"
            continue
        depth = len(raw_line) - len(raw_line.lstrip("\t"))
        while depth < len(stack):
            stack.pop()
        fields = raw_line.lstrip("\t").split()
        if fields[0] == "Block":
            level_id = int(fields[3])
            levels.setdefault(
                level_id,
                {"area": None, "portals": []},
            )
            if int(fields[16]) == 6:
                levels[level_id]["area"] = "Area_Intro"
            stack.append(level_id)
        elif fields[0] == "Floor" and fields[3] == "Portal":
            levels[stack[-1]]["portals"].append(fields[4])
        elif fields[0] == "Ref":
            references.append(fields)

    for fields in references:
        area = fields[16]
        if area != "_":
            levels[int(fields[3])]["area"] = area

    result = {}
    for level in levels.values():
        area = level["area"]
        if area not in AREA_INDEX:
            continue
        for asset in level["portals"]:
            if asset in result:
                raise ValueError(f"puzzle {asset} appears in multiple hub areas")
            result[asset] = area
    return result


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--app", type=Path, default=DEFAULT_APP)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--refs",
        nargs="+",
        default=None,
        help="extract only these references (default: the complete original game)",
    )
    parser.add_argument("--allow-other-build", action="store_true")
    args = parser.parse_args()

    steamapps = args.app.parents[2]
    manifest_path = steamapps / f"appmanifest_{APP_ID}.acf"
    resources = args.app / "Contents/Resources/Data/resources.assets"
    manifest = read_manifest(manifest_path)
    if (
        manifest.get("buildid") != PINNED_BUILD_ID
        and not args.allow_other_build
    ):
        raise ValueError(
            f"expected build {PINNED_BUILD_ID}, found {manifest.get('buildid')}; "
            "review changes before passing --allow-other-build"
        )

    assets = text_assets(resources)
    puzzle_rows = [line.split() for line in assets["puzzle_data"].splitlines()]
    if any(len(row) != 7 for row in puzzle_rows):
        raise ValueError("unexpected puzzle_data format")
    by_ref = {row[6]: row for row in puzzle_rows}
    by_asset = {row[0]: row for row in puzzle_rows}
    if len(by_ref) != len(puzzle_rows) or len(by_asset) != len(puzzle_rows):
        raise ValueError("puzzle_data contains duplicate identifiers")

    areas = hub_areas(assets["hub"])
    if set(areas) != set(by_asset):
        missing = sorted(set(by_asset) - set(areas))
        raise ValueError(f"hub area metadata is incomplete: {missing}")

    incoming = {}
    graph = []
    for line in assets["puzzle_lines"].splitlines():
        source, target, immediate = line.split()
        if target in incoming:
            raise ValueError(f"puzzle {target} has multiple predecessors")
        incoming[target] = (source, immediate == "1")
        graph.append(
            {
                "from": by_asset[source][6],
                "to": by_asset[target][6],
                "immediate": immediate == "1",
            }
        )

    selected_refs = set(args.refs or by_ref)
    unknown = selected_refs - set(by_ref)
    if unknown:
        raise ValueError(f"unknown level references: {sorted(unknown)}")
    rows = [by_ref[reference] for reference in selected_refs]
    rows.sort(key=lambda row: (AREA_INDEX[areas[row[0]]], int(row[6][1:])))
    args.output.mkdir(parents=True, exist_ok=True)

    extracted = []
    index = []
    for row in rows:
        name = row[0]
        reference = row[6]
        if name not in assets:
            raise ValueError(f"could not find level asset {name}")
        path = args.output / f"{reference}.level"
        path.write_text(assets[name])
        predecessor = incoming.get(name)
        extracted.append(
            {
                "ref": reference,
                "asset": name,
                "area": areas[name],
                "kind": KINDS[int(row[4])],
                "sha256": sha256(path),
            }
        )
        title = name.replace("_", " ").title()
        index.append(
            "\t".join(
                (
                    reference,
                    title,
                    path.name,
                    areas[name],
                    KINDS[int(row[4])],
                    by_asset[predecessor[0]][6] if predecessor else "-",
                    "1" if predecessor and predecessor[1] else "0",
                )
            )
        )

    source = {
        "notice": "Extracted from a locally owned game copy; do not redistribute.",
        "steam_app_id": APP_ID,
        "build_id": manifest.get("buildid"),
        "depot_manifest": manifest.get("manifest"),
        "resources_sha256": sha256(resources),
        "puzzle_count": len(puzzle_rows),
        "puzzle_lines_sha256": hashlib.sha256(
            assets["puzzle_lines"].encode()
        ).hexdigest(),
        "graph": [
            edge
            for edge in graph
            if edge["from"] in selected_refs and edge["to"] in selected_refs
        ],
        "areas": [
            {
                "id": area,
                "name": name,
                "access": access,
                "from": source,
                "required_adjustment": adjustment,
                "lookahead": lookahead,
                "has_gate": has_gate,
            }
            for (
                area,
                name,
                access,
                source,
                adjustment,
                lookahead,
                has_gate,
            ) in AREA_ORDER
        ],
        "levels": extracted,
    }
    (args.output.parent / "source.json").write_text(
        json.dumps(source, indent=2) + "\n"
    )
    (args.output.parent / "index.tsv").write_text("\n".join(index) + "\n")
    (args.output.parent / "areas.tsv").write_text(
        "\n".join(
            "\t".join(
                (
                    area,
                    name,
                    access,
                    source,
                    str(adjustment),
                    str(lookahead),
                    "1" if has_gate else "0",
                )
            )
            for (
                area,
                name,
                access,
                source,
                adjustment,
                lookahead,
                has_gate,
            ) in AREA_ORDER
        )
        + "\n"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
