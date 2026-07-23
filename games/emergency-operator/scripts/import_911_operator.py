#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "UnityPy==1.25.2",
# ]
# ///
"""Extract gameplay data from a locally owned 911 Operator installation."""

from __future__ import annotations

import argparse
import ast
import gzip
import hashlib
import json
import re
import shutil
import subprocess
import tempfile
import xml.etree.ElementTree as ET
from collections import Counter
from pathlib import Path

import UnityPy


APP_ID = "503560"
SUPPORTED_BUILD = "7533741"
ASSEMBLY_SHA256 = "689723028bbbee5f3b0401889486aaec3718386a53287eadc4c3cfa71ca491fb"
RESOURCES_SHA256 = "60ae2fe1f740de3b46dc328f79ffe6af04b62f0900ba537d9343e494327d906d"

CITY_BY_GENERATOR = {
    "generateKapolei": ("150921582", "Kapolei"),
    "generateAlbuquerque": ("151364049", "Albuquerque"),
    "generateChicago": ("153388690", "Chicago"),
    "generateSanFrancisco": ("26819236", "San Francisco"),
    "generateWashington": ("158368533", "Washington"),
}

TEXT_ASSETS = {
    "reportConversation5": "report-conversations.xml",
    "reportGroups": "report-groups.xml",
    "reportTypes7": "report-types.xml",
    "vehicleTypes2": "vehicle-types.xml",
    "PeopleDatabase": "people.xml",
}

TEXT_ASSET_ITEMS = {
    "reportConversation5": "conversation",
    "reportGroups": "type",
    "reportTypes7": "type",
    "vehicleTypes2": "type",
    "PeopleDatabase": "RandomPerson",
}

EXPECTED_FIXED_CALLS = [4, 12, 13, 17, 14]


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def json_bytes(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=False, indent=2) + "\n").encode()


def write(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)


def normalized_xml(data: bytes) -> bytes:
    text = data.decode("utf-8-sig").replace("\r\n", "\n").replace("\r", "\n")
    return ("\n".join(line.rstrip() for line in text.splitlines()) + "\n").encode()


def locate_data_root(explicit: Path | None) -> Path:
    candidates = []
    if explicit:
        candidates.extend(
            [
                explicit,
                explicit / "Contents/Resources/Data",
                explicit / "911 Operator Mac.app/Contents/Resources/Data",
            ]
        )
    steam_common = (
        Path.home() / "Library/Application Support/Steam/steamapps/common/911 Operator"
    )
    candidates.append(steam_common / "911 Operator Mac.app/Contents/Resources/Data")
    for candidate in candidates:
        if (candidate / "Managed/Assembly-CSharp.dll").is_file():
            return candidate.resolve()
    raise SystemExit(
        "911 Operator data directory not found; pass --game-root pointing to the "
        "Steam install or the app's Contents/Resources/Data directory"
    )


def verify_source(data_root: Path) -> tuple[Path, Path]:
    assembly = data_root / "Managed/Assembly-CSharp.dll"
    resources = data_root / "resources.assets"
    actual = {
        "Assembly-CSharp.dll": sha256(assembly.read_bytes()),
        "resources.assets": sha256(resources.read_bytes()),
    }
    expected = {
        "Assembly-CSharp.dll": ASSEMBLY_SHA256,
        "resources.assets": RESOURCES_SHA256,
    }
    if actual != expected:
        raise SystemExit(
            "unsupported 911 Operator build; expected Steam macOS build "
            f"{SUPPORTED_BUILD}, got hashes {actual}"
        )
    return assembly, resources


def disassemble(assembly: Path) -> str:
    monodis = shutil.which("monodis")
    if not monodis:
        raise SystemExit("monodis is required (install Mono before importing)")
    result = subprocess.run(
        [monodis, str(assembly)],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return result.stdout.decode("utf-8", errors="strict")


def method_body(il: str, owner: str, method: str) -> str:
    pattern = re.compile(
        rf"\.method .*?\n\s+.*?\b{re.escape(method)} \(\)  cil managed\s*"
        rf"\{{(.*?)\n    \}} // end of method {re.escape(owner)}::{re.escape(method)}",
        re.DOTALL,
    )
    match = pattern.search(il)
    if not match:
        raise ValueError(f"could not find {owner}::{method} in disassembly")
    return match.group(1)


def il_int(line: str) -> int | None:
    match = re.search(r"\bldc\.i4\.([0-8]|m1)\b", line)
    if match:
        return -1 if match.group(1) == "m1" else int(match.group(1))
    match = re.search(r"\bldc\.i4\.s\s+0x([0-9a-fA-F]+)\b", line)
    if match:
        value = int(match.group(1), 16)
        return value - 256 if value >= 128 else value
    match = re.search(r"\bldc\.i4\s+(-?\d+)\b", line)
    return int(match.group(1)) if match else None


def base_career_branch(body: str) -> str:
    """Select the non-free-play branch in a level generator."""
    match = re.search(r"\bbne\.un\s+(IL_[0-9A-Fa-f]+)", body)
    if not match:
        return body
    branch_label = match.group(1)
    prefix, branch = body.split(f"{branch_label}:", 1)
    joins = re.findall(r"\bbr\s+(IL_[0-9A-Fa-f]+)", prefix)
    if not joins:
        raise ValueError("career branch has no join label")
    join_label = joins[-1]
    return branch.split(f"{join_label}:", 1)[0]


def conversation_specs(body: str) -> list[dict[str, object]]:
    specs = []
    current: list[str] | None = None
    strings: list[str] = []
    integers: list[int] = []
    for line in body.splitlines():
        if "ScenarioConversations::conversationsSpec" in line:
            current = []
            strings = []
            integers = []
            continue
        if current is None:
            continue
        string_match = re.search(r'\bldstr\s+("(?:[^"\\]|\\.)*")', line)
        if string_match:
            strings.append(ast.literal_eval(string_match.group(1)))
        value = il_int(line)
        if value is not None:
            integers.append(value)
        if "ConversationSpec::'.ctor'" in line:
            if not strings or len(integers) < 2:
                raise ValueError("incomplete ConversationSpec IL sequence")
            specs.append(
                {
                    "call_id": strings[0],
                    "duty": integers[0],
                    "order": integers[1],
                    "deactivated_dialogue_ids": strings[1:],
                }
            )
            current = None
    return specs


def last_int_assignment(body: str, field: str) -> int:
    lines = body.splitlines()
    values = []
    for index, line in enumerate(lines):
        if f"LevelData::{field}" not in line or "stfld" not in line:
            continue
        for prior in reversed(lines[max(0, index - 3) : index]):
            value = il_int(prior)
            if value is not None:
                values.append(value)
                break
    if not values:
        raise ValueError(f"no assignment for LevelData::{field}")
    return values[-1]


def chapter_condition(body: str, field: str) -> int | float:
    lines = body.splitlines()
    for index, line in enumerate(lines):
        if f"ScenarioEndConditions::{field}" not in line or "stfld" not in line:
            continue
        for prior in reversed(lines[max(0, index - 3) : index]):
            integer = il_int(prior)
            if integer is not None:
                return integer
            float_match = re.search(r"\bldc\.r4\s+(-?\d+(?:\.\d+)?)", prior)
            if float_match:
                return float(float_match.group(1))
    raise ValueError(f"no assignment for ScenarioEndConditions::{field}")


def extract_campaign(il: str, call_ids: set[str]) -> dict[str, object]:
    chapters = []
    for number in range(1, 6):
        chapter_body = method_body(il, "Campaign", f"GetChapter_{number}")
        generator_match = re.search(
            r"LevelDataGenerator::(generate[A-Za-z]+)\(\)", chapter_body
        )
        if not generator_match:
            raise ValueError(f"chapter {number} has no city generator")
        generator = generator_match.group(1)
        generator_body = method_body(il, "LevelDataGenerator", generator)
        specs = (
            conversation_specs(base_career_branch(generator_body))
            if number == 4
            else conversation_specs(chapter_body)
        )
        specs.sort(key=lambda item: (item["duty"], item["order"]))
        if len(specs) != EXPECTED_FIXED_CALLS[number - 1]:
            raise ValueError(
                f"chapter {number}: expected {EXPECTED_FIXED_CALLS[number - 1]} "
                f"fixed calls, found {len(specs)}"
            )
        missing = sorted({str(item["call_id"]) for item in specs} - call_ids)
        if missing:
            raise ValueError(f"chapter {number} references missing calls: {missing}")

        city_id, city = CITY_BY_GENERATOR[generator]
        try:
            duties = last_int_assignment(chapter_body, "endCityAfterDuties")
        except ValueError:
            duties = last_int_assignment(generator_body, "endCityAfterDuties")
        duty_rows = []
        for duty in range(duties):
            duty_rows.append(
                {
                    "number": duty + 1,
                    "fixed_calls": [
                        {
                            "order": int(item["order"]) + 1,
                            "call_id": item["call_id"],
                            "deactivated_dialogue_ids": item[
                                "deactivated_dialogue_ids"
                            ],
                        }
                        for item in specs
                        if item["duty"] == duty
                    ],
                }
            )
        chapters.append(
            {
                "number": number,
                "city": city,
                "map_id": city_id,
                "duties": duty_rows,
                "generated_reports": True,
                "call_interval_seconds": {
                    "min": last_int_assignment(generator_body, "callTimesMin"),
                    "max": last_int_assignment(generator_body, "callTimesMax"),
                },
                "report_interval_seconds": {
                    "min": last_int_assignment(generator_body, "reportTimesMin"),
                    "max": last_int_assignment(generator_body, "reportTimesMax"),
                },
                "completion": {
                    "reputation": chapter_condition(chapter_body, "reputationBorder"),
                    "efficiency_percent": chapter_condition(chapter_body, "efficiency"),
                },
                "source": {
                    "chapter_method": f"Campaign::GetChapter_{number}",
                    "level_generator": f"LevelDataGenerator::{generator}",
                },
            }
        )
    return {
        "schema": "911-operator-career-v1",
        "steam_app_id": APP_ID,
        "source_build_id": SUPPORTED_BUILD,
        "chapters": chapters,
    }


def call_properties(raw: str) -> dict[str, str]:
    result = {}
    for item in raw.replace("\r", "").replace("\n", "").split(";"):
        key, separator, value = item.partition("=")
        if separator and key.strip():
            result[key.strip()] = value.strip()
    return result


def extract_calls(data_root: Path, output: Path) -> tuple[list[dict], set[str]]:
    source = data_root / "StreamingAssets/Calls"
    rows = []
    ids = set()
    for xml_path in sorted(source.glob("*/*.xml"), key=lambda path: path.parent.name):
        raw = xml_path.read_bytes()
        imported = normalized_xml(raw)
        root = ET.fromstring(imported)
        conversation = root.find(".//conversation")
        if conversation is None:
            raise ValueError(f"{xml_path} contains no conversation")
        call_id = conversation.attrib["id"]
        if call_id in ids:
            raise ValueError(f"duplicate call id {call_id}")
        ids.add(call_id)
        if call_id != xml_path.stem:
            raise ValueError(f"{xml_path}: directory and conversation id differ")
        scene = conversation.find("scene")
        scene_types = (
            Counter(child.tag for child in scene) if scene is not None else Counter()
        )
        dialogues = list(conversation.findall("./dialog/dialogOption"))
        output_path = output / "calls" / f"{call_id}.xml"
        write(output_path, imported)
        rows.append(
            {
                "id": call_id,
                "title": conversation.attrib.get("texten", ""),
                "properties": call_properties(
                    conversation.attrib.get("elementProperties", "")
                ),
                "dialogue_nodes": len(dialogues),
                "scene_elements": sum(scene_types.values()),
                "scene_types": dict(sorted(scene_types.items())),
                "source_sha256": sha256(raw),
                "file_sha256": sha256(imported),
                "file": f"calls/{call_id}.xml",
            }
        )
    write(output / "calls/index.json", json_bytes({"calls": rows}))
    return rows, ids


def extract_text_assets(resources: Path, output: Path) -> list[dict]:
    environment = UnityPy.load(str(resources))
    found: dict[str, bytes] = {}
    for obj in environment.objects:
        if obj.type.name != "TextAsset":
            continue
        data = obj.read()
        if data.m_Name not in TEXT_ASSETS:
            continue
        script = data.m_Script
        found[data.m_Name] = (
            script.encode() if isinstance(script, str) else bytes(script)
        )
    missing = sorted(set(TEXT_ASSETS) - set(found))
    if missing:
        raise ValueError(f"resources.assets is missing TextAssets: {missing}")
    rows = []
    for asset_name, file_name in TEXT_ASSETS.items():
        data = found[asset_name]
        imported = normalized_xml(data)
        root = ET.fromstring(imported)
        item_tag = TEXT_ASSET_ITEMS[asset_name]
        write(output / "definitions" / file_name, imported)
        rows.append(
            {
                "asset_name": asset_name,
                "root_element": root.tag,
                "items": sum(1 for _ in root.iter(item_tag)),
                "xml_elements": sum(1 for _ in root.iter()),
                "source_sha256": sha256(data),
                "file_sha256": sha256(imported),
                "file": f"definitions/{file_name}",
            }
        )
    return rows


def extract_maps(data_root: Path, output: Path) -> list[dict]:
    rows = []
    for xml_path in sorted((data_root / "StreamingAssets/Maps").glob("*.xml")):
        raw = xml_path.read_bytes()
        root = ET.fromstring(raw)
        city = root.find("./city")
        waynodes = root.find("./waynodes")
        if city is None or waynodes is None:
            raise ValueError(f"{xml_path} has no city or waynodes metadata")
        city_node = city.find("node")
        tags = {
            tag.attrib["k"]: tag.attrib.get("v", "")
            for tag in city.findall("tag")
            if "k" in tag.attrib
        }
        nodes = waynodes.findall("node")
        connections = sum(len(node.findall("conn")) for node in nodes)
        compressed = gzip.compress(raw, compresslevel=9, mtime=0)
        relative = f"maps/{xml_path.stem}.xml.gz"
        write(output / relative, compressed)
        rows.append(
            {
                "id": xml_path.stem,
                "name": tags.get("name", xml_path.stem),
                "country": tags.get("country", ""),
                "region": tags.get("region", ""),
                "center": {
                    "lat": float(city_node.attrib["lat"])
                    if city_node is not None
                    else None,
                    "lon": float(city_node.attrib["lon"])
                    if city_node is not None
                    else None,
                },
                "bounds": {
                    key: float(waynodes.attrib[key])
                    for key in ("pbottom", "pleft", "pright", "ptop")
                },
                "road_level": int(waynodes.attrib["road_level"]),
                "nodes": len(nodes),
                "connections": connections,
                "source_sha256": sha256(raw),
                "compressed_sha256": sha256(compressed),
                "file": relative,
            }
        )
    write(output / "maps/index.json", json_bytes({"maps": rows}))
    return rows


def aggregate_hash(paths: list[Path], root: Path) -> str:
    digest = hashlib.sha256()
    for path in sorted(paths):
        digest.update(str(path.relative_to(root)).encode())
        digest.update(b"\0")
        digest.update(path.read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()


def readme() -> bytes:
    return f"""# Imported 911 Operator gameplay data

This directory is a deterministic, gameplay-only extraction from a locally
owned Steam installation of 911 Operator (app {APP_ID}, macOS build
{SUPPORTED_BUILD}).

Included:

- all installed call dialogue/scene XML;
- all installed city road graphs, losslessly gzip-compressed;
- the five-chapter base career layout recovered from `Assembly-CSharp.dll`;
- report, vehicle, person, and internal scenario definitions extracted from
  `resources.assets`.

Excluded:

- audio, map JPGs, textures, models, UI, and other presentation assets;
- non-English localization;
- user saves, Steam account data, and absolute installation paths.

Regenerate from an owned installation:

```bash
uv run games/emergency-operator/scripts/import_911_operator.py
```

The importer refuses unknown assembly/resource hashes rather than silently
guessing at a changed game format. `manifest.json` records the supported source
hashes and a content inventory.
""".encode()


def build(data_root: Path, output: Path) -> None:
    assembly, resources = verify_source(data_root)
    write(output / "README.md", readme())
    calls, call_ids = extract_calls(data_root, output)
    campaign = extract_campaign(disassemble(assembly), call_ids)
    write(output / "campaign.json", json_bytes(campaign))
    definitions = extract_text_assets(resources, output)
    maps = extract_maps(data_root, output)
    definition_counts = {row["asset_name"]: row["items"] for row in definitions}

    files = [path for path in output.rglob("*") if path.is_file()]
    manifest = {
        "schema": "911-operator-gameplay-import-v1",
        "source": {
            "title": "911 Operator",
            "steam_app_id": APP_ID,
            "build_id": SUPPORTED_BUILD,
            "assembly_sha256": ASSEMBLY_SHA256,
            "resources_sha256": RESOURCES_SHA256,
        },
        "inventory": {
            "calls": len(calls),
            "call_dialogue_nodes": sum(row["dialogue_nodes"] for row in calls),
            "call_scene_elements": sum(row["scene_elements"] for row in calls),
            "internal_conversations": definition_counts["reportConversation5"],
            "report_groups": definition_counts["reportGroups"],
            "report_types": definition_counts["reportTypes7"],
            "vehicle_types": definition_counts["vehicleTypes2"],
            "maps": len(maps),
            "map_nodes": sum(row["nodes"] for row in maps),
            "map_connections": sum(row["connections"] for row in maps),
            "career_chapters": len(campaign["chapters"]),
            "career_duties": sum(
                len(chapter["duties"]) for chapter in campaign["chapters"]
            ),
            "career_fixed_call_slots": sum(
                len(duty["fixed_calls"])
                for chapter in campaign["chapters"]
                for duty in chapter["duties"]
            ),
        },
        "definitions": definitions,
        "artifact_sha256": aggregate_hash(files, output),
        "excluded": [
            "audio",
            "map_jpg",
            "textures",
            "models",
            "ui",
            "non_english_localization",
            "steam_account_data",
        ],
    }
    write(output / "manifest.json", json_bytes(manifest))


def directory_hash(root: Path) -> str:
    return aggregate_hash([path for path in root.rglob("*") if path.is_file()], root)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--game-root",
        type=Path,
        help="911 Operator install, app bundle, or Contents/Resources/Data",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path(__file__).resolve().parents[1] / "data/911-operator",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="verify committed output matches a fresh extraction",
    )
    args = parser.parse_args()
    data_root = locate_data_root(args.game_root)
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="911-operator-import-") as temp:
        generated = Path(temp) / "911-operator"
        build(data_root, generated)
        if args.check:
            if not output.is_dir() or directory_hash(output) != directory_hash(
                generated
            ):
                raise SystemExit(f"{output} does not match a fresh extraction")
            print(f"verified {output}")
            return
        if output.exists():
            shutil.rmtree(output)
        shutil.copytree(generated, output)
        print(f"wrote {output}")


if __name__ == "__main__":
    main()
