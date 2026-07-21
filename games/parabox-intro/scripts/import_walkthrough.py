#!/usr/bin/env python3
"""Import direction-only oracle traces from the public Steam walkthrough."""

from __future__ import annotations

import argparse
import re
from pathlib import Path

import requests


DEFAULT_URL = "https://steamcommunity.com/sharedfiles/filedetails/?id=2786724419"
WORLDS = (
    ("Intro", "a", 9),
    ("Enter", "b", 18),
    ("Empty", "c", 14),
    ("Eat", "d", 13),
    ("Reference", "e", 12),
    ("Swap", "L", 5),
    ("Center", "f", 16),
    ("Clone", "g", 25),
    ("Transfer", "h", 29),
    ("Open", "i", 12),
    ("Flip", "j", 17),
    ("Cycle", "k", 18),
    ("Player", "m", 24),
    ("Possess", "n", 22),
    ("Wall", "o", 15),
    ("Infinite Exit", "p", 18),
    ("Infinite Enter", "q", 20),
    ("Multi Infinite", "r", 11),
    ("Challenge", "s", 38),
    ("Gallery", "t", 3),
    ("Appendix: Priority", "u", 9),
    ("Appendix: Extrude", "v", 8),
    ("Appendix: Inner Push", "w", 8),
)


def numbered_items(text: str) -> dict[int, str]:
    matches = list(re.finditer(r"(?m)^\*\*([1-9][0-9]*)(?: ?[^*]*)?\*\*", text))
    return {
        int(match.group(1)): text[
            match.end() : matches[index + 1].start()
            if index + 1 < len(matches)
            else None
        ]
        for index, match in enumerate(matches)
    }


def read_steam_markdown(url: str) -> str:
    reader_url = f"https://r.jina.ai/http://{url.removeprefix('https://').removeprefix('http://')}"
    response = requests.get(reader_url, timeout=60)
    response.raise_for_status()
    return response.text


def sections(markdown: str) -> dict[str, str]:
    lines = markdown.splitlines()
    positions = {}
    wanted = {world for world, _, _ in WORLDS} | {"Reception", "Thanks"}
    for index, line in enumerate(lines):
        heading = line.strip()
        if heading in wanted and heading not in positions:
            positions[heading] = index

    result = {}
    ordered = [world for world, _, _ in WORLDS]
    for index, world in enumerate(ordered):
        start = positions.get(world)
        if start is None:
            raise ValueError(f"walkthrough has no {world} section")
        if world == "Multi Infinite":
            end = positions["Reception"]
        elif index + 1 < len(ordered):
            end = positions[ordered[index + 1]]
        else:
            end = positions["Thanks"]
        result[world] = "\n".join(lines[start + 1 : end])
    return result


def directions(text: str, world: str, number: int) -> str:
    if (world, number) in {("Empty", 14), ("Clone", 5), ("Clone", 6)}:
        text = text.split("Normal:", 1)[1]
    text = re.sub(r"\[[^]]*]", " ", text)
    text = re.sub(r"\([^)]*\)", " ", text)
    moves = re.findall(r"(?<![A-Z])[UDLR]+(?![A-Z])", text)
    if not moves:
        raise ValueError(f"no moves found for {world} {number}: {text}")
    return " ".join(moves)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--url", default=DEFAULT_URL)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    guide_sections = sections(read_steam_markdown(args.url))

    rows = []
    for world, prefix, count in WORLDS:
        items = numbered_items(guide_sections[world])
        for number in range(1, count + 1):
            if number not in items:
                raise ValueError(f"walkthrough has no {world} level {number}")
            reference = f"{prefix}{number}"
            moves = directions(items[number], world, number)
            rows.append(f"{reference}\t{moves}")

    if len(rows) != 364:
        raise AssertionError(f"expected 364 oracle traces, found {len(rows)}")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        f"# source: {args.url}\n"
        "# imported direction traces; never package this file in the agent image\n"
        + "\n".join(rows)
        + "\n"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
