#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# ///
"""Compile the imported 911 Operator career into one deterministic benchmark run."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import re
import tempfile
import xml.etree.ElementTree as ET
from pathlib import Path


GAME = Path(__file__).resolve().parents[1]
SOURCE = GAME / "data/911-operator"
OUTPUT = GAME / "data/campaign/911-career.json"
SCHEMA = "emergency-operator-campaign-v2"
CALL_GAP_MS = 90_000
DUTY_TAIL_MS = 240_000

ROLE_BY_SCENE = {
    "criminal": "police",
    "suspect": "police",
    "injured": "medical",
    "injuried": "medical",
    "fire": "fire",
    "tech": "fire",
    "work": "fire",
}
WORK_MS = {"police": 90_000, "medical": 120_000, "fire": 150_000}
SCORE_ACTION = re.compile(
    r"^(opinioneffect|onignore|score|addcash)(?:\+=|=)([-+]?[0-9]+(?:\.[0-9]+)?)$"
)
MAP_POINTS: dict[str, list[tuple[float, float]]] = {}


def property_items(raw: str) -> list[tuple[str, str]]:
    result = []
    for item in raw.replace("\r", "").replace("\n", "").split(";"):
        key, separator, value = item.partition("=")
        if separator and key.strip():
            result.append((key.strip().lower(), value.strip()))
    return result


def properties(raw: str) -> dict[str, str]:
    return dict(property_items(raw))


def number(value: str | int | float) -> float:
    return float(str(value).replace(",", "."))


def map_points(map_id: str) -> list[tuple[float, float]]:
    if map_id not in MAP_POINTS:
        path = SOURCE / "maps" / f"{map_id}.xml.gz"
        with gzip.open(path) as source:
            root = ET.parse(source).getroot()
        bounds = root.find("./waynodes")
        if bounds is None:
            raise ValueError(f"{map_id}: waynodes missing")
        left = number(bounds.attrib["pleft"])
        right = number(bounds.attrib["pright"])
        bottom = number(bounds.attrib["pbottom"])
        top = number(bounds.attrib["ptop"])
        points = []
        for node in bounds.findall("./node"):
            x = (number(node.attrib["lon"]) - left) / (right - left) * 16 - 8
            y = (number(node.attrib["lat"]) - bottom) / (top - bottom) * 16 - 8
            points.append((x, y))
        if not points:
            raise ValueError(f"{map_id}: road nodes missing")
        MAP_POINTS[map_id] = points
    return MAP_POINTS[map_id]


def deterministic_point(instance_id: str, map_id: str) -> dict[str, int]:
    digest = hashlib.sha256(instance_id.encode()).digest()
    points = map_points(map_id)
    index = int.from_bytes(digest[:8], "big") % len(points)
    raw_x, raw_y = points[index]
    x = max(-8, min(8, round(raw_x)))
    y = max(-8, min(8, round(raw_y)))
    if x == y == 0:
        x = 1
    return {"x": x, "y": y}


def substitute(text: str, point: dict[str, int], instance_id: str) -> str:
    plate = hashlib.sha256((instance_id + ":plate").encode()).hexdigest()[:6].upper()
    replacements = {
        "[[ADDRESS]]": f"sector {point['x']:+d}, {point['y']:+d}",
        "[[PLATES]]": plate,
        "[[NAME]]": "Alex",
    }
    for marker, value in replacements.items():
        text = text.replace(marker, value)
    return text


def answers(element: ET.Element) -> list[str]:
    result = [
        element.attrib[key].strip().lower()
        for key in ("default", "option2", "option3")
        if element.attrib.get(key, "").strip()
    ]
    result.extend(
        value.strip().lower()
        for key, value in property_items(element.attrib.get("elementProperties", ""))
        if key == "option" and value.strip()
    )
    return list(dict.fromkeys(result))


def aar_catalog(conversation: ET.Element) -> dict[str, str]:
    return {
        element.attrib["id"]: element.attrib.get("texten", element.attrib["id"])
        for element in conversation.findall("./aar/reportElement")
        if element.attrib.get("id")
    }


def dialogue_nodes(
    conversation: ET.Element, point: dict[str, int], instance_id: str
) -> list[dict]:
    root_actions = conversation.attrib.get("actionOnEnd", "").strip()
    root_aar = conversation.attrib.get("addToAAR", "").strip()
    aar_text = aar_catalog(conversation)

    def expand_aar(value: str) -> str:
        return aar_text.get(value, value)

    nodes = []
    for element in conversation.findall("./dialog/dialogOption"):
        node_id = element.attrib.get("id", "").strip().lower()
        if not node_id:
            continue
        node_answers = answers(element)
        actions = [element.attrib["actionOnEnd"]] if element.attrib.get("actionOnEnd") else []
        aar = (
            [expand_aar(element.attrib["addToAAR"])]
            if element.attrib.get("addToAAR")
            else []
        )
        if root_actions and (
            any("actionhangup" in action.lower() for action in actions) or not node_answers
        ):
            actions.append(root_actions)
        if root_aar and (
            any("actionhangup" in action.lower() for action in actions) or not node_answers
        ):
            aar.append(expand_aar(root_aar))
        node_properties = properties(element.attrib.get("elementProperties", ""))
        try:
            chance = number(node_properties.get("chance", "0.33"))
        except ValueError as error:
            raise ValueError(f"{instance_id} node {node_id}: invalid chance") from error
        node = {
            "id": node_id,
            "operator": "operator" in element.attrib.get("operator", "").lower(),
            "text": substitute(element.attrib.get("texten", node_id), point, instance_id),
            "answers": node_answers,
            "chance_weight": max(1, round(chance * 1000)),
            "actions": actions,
            "aar": aar,
        }
        duplicate = next((existing for existing in nodes if existing["id"] == node_id), None)
        if duplicate:
            if duplicate != node:
                raise ValueError(f"{instance_id}: conflicting duplicate node {node_id}")
            continue
        nodes.append(node)
    return nodes


def incident(
    conversation: ET.Element, instance_id: str, point: dict[str, int]
) -> dict | None:
    scene = conversation.find("scene")
    if scene is None or not list(scene):
        return None
    elements = []
    aar_text = aar_catalog(conversation)
    for element in scene:
        scene_properties = properties(element.attrib.get("elementProperties", ""))
        kind = "injured" if element.tag == "injuried" else element.tag
        role = ROLE_BY_SCENE.get(element.tag)
        health = scene_properties.get("hp")
        health_change = number(scene_properties.get("hpchange", "0"))
        work = number(scene_properties.get("work", "0"))
        if role and work <= 0:
            work = WORK_MS[role] / 1000
        compiled = {
            "id": element.attrib.get("id", kind).strip().lower(),
            "label": element.attrib.get("texten", kind),
            "kind": kind,
            "active": scene_properties.get("isactive", "true").lower() == "true",
            "role": role,
            "health_milli": (
                round(number(health or "100") * 1000)
                if health is not None or kind == "injured"
                else None
            ),
            "health_decay_milli_per_minute": (
                round(-health_change * 60_000) if health_change < 0 else 0
            ),
            "work_ms": round(max(0, work) * 1000),
            "work_growth_ms_per_minute": round(
                max(0, number(scene_properties.get("workchange", "0"))) * 60_000
            ),
            "blocked_by": (
                element.attrib["blockedBy"].strip().lower()
                if element.attrib.get("blockedBy")
                else None
            ),
            "weapon": scene_properties.get("weapon"),
            "fight_risk_milli": round(
                number(scene_properties.get("fightrisk", "0")) * 1000
            ),
            "prison_chance_milli": round(
                number(scene_properties.get("prisonchance", "0")) * 1000
            ),
            "bill": (
                round(number(scene_properties["bill"]))
                if scene_properties.get("bill")
                else None
            ),
            "timer_ms": (
                round(number(scene_properties["time"]) * 1000)
                if kind == "timer" and scene_properties.get("time")
                else None
            ),
            "actions": (
                [element.attrib["actionOnEnd"]]
                if element.attrib.get("actionOnEnd")
                else []
            ),
            "aar": (
                [aar_text.get(element.attrib["addToAAR"], element.attrib["addToAAR"])]
                if element.attrib.get("addToAAR")
                else []
            ),
        }
        elements.append(compiled)
    return {
        "id": f"{instance_id}-incident",
        "title": conversation.attrib.get("texten", instance_id),
        "location": point,
        "base_score": 25 + 25 * len(scene),
        "elements": elements,
    }


def compile_call(
    spec: dict,
    chapter: dict,
    duty: dict,
    arrival_ms: int,
) -> dict:
    call_id = spec["call_id"]
    instance_id = (
        f"chapter-{chapter['number']}-duty-{duty['number']}-"
        f"call-{spec['order']}-{call_id}"
    )
    point = deterministic_point(instance_id, chapter["map_id"])
    conversation = ET.parse(SOURCE / "calls" / f"{call_id}.xml").getroot().find(
        ".//conversation"
    )
    if conversation is None:
        raise ValueError(f"{call_id}: conversation missing")
    nodes = dialogue_nodes(conversation, point, instance_id)
    node_ids = {node["id"] for node in nodes}
    if "1" not in node_ids:
        raise ValueError(f"{call_id}: initial dialogue node 1 missing")
    disabled = [
        node_id.strip().lower()
        for node_id in spec["deactivated_dialogue_ids"]
        if node_id.strip()
    ]
    missing = sorted(set(disabled) - node_ids)
    if missing:
        raise ValueError(f"{call_id}: unknown disabled nodes {missing}")
    call = {
        "id": instance_id,
        "caller": (
            f"{chapter['city']} D{duty['number']} · "
            f"{conversation.attrib.get('texten', call_id)}"
        ),
        "kind": "phone",
        "duty_id": f"chapter-{chapter['number']}-duty-{duty['number']}",
        "arrival_ms": arrival_ms,
        "answer_window_ms": 45_000,
        "conversation_window_ms": 240_000,
        "initial_stage": "1",
        "nodes": nodes,
        "disabled_nodes": disabled,
    }
    scene = incident(conversation, instance_id, point)
    if scene:
        call["incident"] = scene
        if not any(
            "actionsetlocation" in action.lower()
            for node in nodes
            for action in node["actions"]
        ):
            nodes[0]["actions"].append("actionSetLocation")
    return call


def report_catalog() -> dict[str, list[tuple[int, ET.Element]]]:
    categories = {"police": [], "medical": [], "fire": []}
    root = ET.parse(SOURCE / "definitions/report-types.xml").getroot()
    for element in root.findall("./reportTypes/type"):
        if element.attrib.get("dlc") or int(element.attrib.get("popularity", "0")) <= 0:
            continue
        criminal_count = int(element.attrib.get("criminalsNumber", "0"))
        injured_count = int(element.attrib.get("injuriedLight", "0")) + int(
            element.attrib.get("injuriedHeavy", "0")
        )
        fire_work = int(element.attrib.get("firework", "0")) + int(
            element.attrib.get("techwork", "0")
        )
        report_type = element.attrib.get("type", "")
        if criminal_count or report_type in {
            "steal",
            "road",
            "abuse",
            "drugs",
            "pathology",
            "terrorist",
            "event",
        }:
            categories["police"].append((max(1, criminal_count) * 60, element))
        if injured_count or report_type == "medical":
            categories["medical"].append((max(1, injured_count) * 90, element))
        if fire_work or report_type == "fire":
            categories["fire"].append((max(60, fire_work), element))
    for rows in categories.values():
        rows.sort(
            key=lambda row: (
                row[0],
                number(row[1].attrib.get("opinionEffect", "0")),
                row[1].attrib["id"],
            )
        )
    return categories


def compile_report(
    element: ET.Element,
    role: str,
    duty_id: str,
    map_id: str,
    ordinal: int,
    arrival_ms: int,
) -> dict:
    report_id = element.attrib["id"]
    instance_id = f"{duty_id}-report-{ordinal}-{report_id}"
    criminal_count = int(element.attrib.get("criminalsNumber", "0"))
    light = int(element.attrib.get("injuriedLight", "0"))
    heavy = int(element.attrib.get("injuriedHeavy", "0"))
    fire_work = int(element.attrib.get("firework", "0"))
    tech_work = int(element.attrib.get("techwork", "0"))
    work = {}
    if criminal_count:
        work["police"] = max(60_000, criminal_count * 60_000)
    if light or heavy:
        work["medical"] = max(90_000, (light + heavy) * 90_000)
    if fire_work or tech_work:
        work["fire"] = max(60_000, (fire_work + tech_work) * 1_000)
    work.setdefault(role, WORK_MS[role])
    title = report_id.removeprefix("rep_").replace("_", " ").title()
    opinion = number(element.attrib.get("opinionEffect", "1"))
    elements = []
    for required_role, work_ms in sorted(work.items()):
        elements.append(
            {
                "id": required_role,
                "label": f"{title} · {required_role.title()} response",
                "kind": {
                    "police": "suspect",
                    "medical": "injured",
                    "fire": "fire",
                }[required_role],
                "active": True,
                "role": required_role,
                "health_milli": 100_000 if required_role == "medical" else None,
                "health_decay_milli_per_minute": (
                    min(60_000, light * 4_000 + heavy * 12_000)
                    if required_role == "medical"
                    else 0
                ),
                "work_ms": work_ms,
                "work_growth_ms_per_minute": 0,
                "blocked_by": None,
                "weapon": None,
                "fight_risk_milli": round(
                    number(element.attrib.get("fightRisk", "0")) * 1000
                ),
                "prison_chance_milli": round(
                    number(element.attrib.get("prisonChance", "0")) * 1000
                ),
                "bill": None,
            }
        )
    return {
        "id": instance_id,
        "caller": f"CAD · {title}",
        "kind": "report",
        "duty_id": duty_id,
        "arrival_ms": arrival_ms,
        "answer_window_ms": 0,
        "conversation_window_ms": 0,
        "initial_stage": "",
        "nodes": [],
        "disabled_nodes": [],
        "incident": {
            "id": f"{instance_id}-incident",
            "title": title,
            "location": deterministic_point(instance_id, map_id),
            "base_score": round(25 + max(0, opinion) * 20),
            "elements": elements,
        },
    }


def score_ceiling(calls: list[dict]) -> int:
    """Return a documented upper bound, not an undisclosed ideal playthrough."""
    score = sum(
        call["incident"]["base_score"] + 100
        for call in calls
        if call.get("incident")
    )
    for call in calls:
        actions = [
            action
            for node in call.get("nodes", [])
            for action in node.get("actions", [])
        ]
        actions.extend(
            action
            for element in call.get("incident", {}).get("elements", [])
            for action in element.get("actions", [])
        )
        for raw in actions:
            for action in raw.split(";"):
                match = SCORE_ACTION.fullmatch("".join(action.lower().split()))
                if not match:
                    continue
                field, raw_value = match.groups()
                value = number(raw_value)
                if value > 0:
                    score += round(value * (10 if field in {"opinioneffect", "onignore"} else 1))
    return score


def build() -> dict:
    source_campaign = json.loads((SOURCE / "campaign.json").read_text())
    calls = []
    duties = []
    cursor = 0
    duty_count = 0
    catalogs = report_catalog()
    total_duties = sum(
        len(chapter["duties"]) for chapter in source_campaign["chapters"]
    )
    for chapter in source_campaign["chapters"]:
        for duty in chapter["duties"]:
            duty_count += 1
            duty_id = f"chapter-{chapter['number']}-duty-{duty['number']}"
            duty_start = cursor
            duty_duration = max(
                420_000,
                30_000 + len(duty["fixed_calls"]) * CALL_GAP_MS + DUTY_TAIL_MS,
            )
            duty_end = duty_start + duty_duration
            duties.append(
                {
                    "id": duty_id,
                    "chapter": chapter["number"],
                    "number": duty["number"],
                    "city": chapter["city"],
                    "map_id": chapter["map_id"],
                    "start_ms": duty_start,
                    "end_ms": duty_end,
                }
            )
            first_call = cursor + 30_000
            for index, spec in enumerate(duty["fixed_calls"]):
                calls.append(
                    compile_call(
                        spec,
                        chapter,
                        duty,
                        first_call + index * CALL_GAP_MS,
                    )
                )
            progress = (duty_count - 1) / max(1, total_duties - 1)
            used_reports = set()
            for report_index, role in enumerate(("police", "medical", "fire"), 1):
                catalog = catalogs[role]
                catalog_index = round(progress * (len(catalog) - 1))
                while catalog[catalog_index][1].attrib["id"] in used_reports:
                    catalog_index = (catalog_index + 1) % len(catalog)
                report_element = catalog[catalog_index][1]
                used_reports.add(report_element.attrib["id"])
                calls.append(
                    compile_report(
                        report_element,
                        role,
                        duty_id,
                        chapter["map_id"],
                        report_index,
                        min(duty_end - 1, duty_start + 75_000 + (report_index - 1) * 135_000),
                    )
                )
            cursor = duty_end
    campaign = {
        "schema": SCHEMA,
        "id": "911-operator-base-career-v1",
        "title": "911 Operator · Base Career",
        "seed": 0x91103720,
        "max_score": score_ceiling(calls),
        "shift": {
            "id": "base-career",
            "title": f"Five chapters · {duty_count} duties",
            "duration_ms": cursor,
            "duties": duties,
            "units": [
                {
                    "id": f"{role}-{index}",
                    "label": f"{role.title()} {index}",
                    "role": role,
                    "base": {"x": base_x, "y": base_y},
                    "speed_cells_per_minute": speed,
                }
                for role, base_x, base_y in (
                    ("police", -1, 0),
                    ("fire", 1, 0),
                    ("medical", 0, 1),
                )
                for index, speed in ((1, 5), (2, 4))
            ],
            "calls": calls,
        },
    }
    if len(calls) != 60 + total_duties * 3:
        raise ValueError(f"unexpected career event count: {len(calls)}")
    return campaign


def encoded(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=False, indent=2) + "\n").encode()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=OUTPUT)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    generated = encoded(build())
    output = args.output.resolve()
    if args.check:
        if not output.is_file() or output.read_bytes() != generated:
            raise SystemExit(f"{output} is stale; rebuild it without --check")
        print(f"verified {output}")
        return
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=output.parent, delete=False) as temporary:
        temporary.write(generated)
        temporary_path = Path(temporary.name)
    temporary_path.replace(output)
    print(f"wrote {output}")


if __name__ == "__main__":
    main()
