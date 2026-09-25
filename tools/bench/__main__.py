"""bench: launch, pause, and resume scored runs; package and check tasks.

    bench run new <game> --profile <profile> [--id ID] [--account NAME] [--foreground] [--dry-run]
    bench run pause <run>                     stop the active segment and checkpoint it
    bench run stop <run> | checkpoint <run>   the two halves of pause
    bench run resume <run> [--account NAME] [--note FILE]... [--foreground] [--dry-run]
    bench run status [run]
    bench package [game...]                   rebuild task packages and dataset digests
    bench check [task...]                     static checks, data copies, digests
    bench audit <run>                         results/audits/<run>.json from the journal
    bench compact [job...]                    zstd large agent logs of finished jobs
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from tools.bench import runs, tasks

ROOT = Path(__file__).resolve().parents[2]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="bench", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)

    run = commands.add_parser("run").add_subparsers(dest="action", required=True)
    new = run.add_parser("new")
    new.add_argument("game")
    new.add_argument("--profile", required=True)
    new.add_argument("--id")
    new.add_argument("--account")
    new.add_argument("--foreground", action="store_true")
    new.add_argument("--dry-run", action="store_true", help="write the manifest and config without launching")
    for name in ("stop", "pause"):
        action = run.add_parser(name)
        action.add_argument("run")
        action.add_argument("--timeout", type=float, default=900)
    point = run.add_parser("checkpoint")
    point.add_argument("run")
    point.add_argument("--allow-unsealed", action="store_true")
    again = run.add_parser("resume")
    again.add_argument("run")
    again.add_argument("--account")
    again.add_argument("--note", type=Path, action="append", default=[], help="extra instruction for this segment")
    again.add_argument("--foreground", action="store_true")
    again.add_argument("--dry-run", action="store_true")
    run.add_parser("status").add_argument("run", nargs="?")

    commands.add_parser("package").add_argument("games", nargs="*")
    commands.add_parser("check").add_argument("tasks", nargs="*")
    commands.add_parser("audit").add_argument("run")
    commands.add_parser("compact").add_argument("jobs", nargs="*")

    args = parser.parse_args(argv)
    if args.command == "package":
        tasks.package(ROOT, args.games)
    elif args.command == "check":
        return 0 if tasks.check(ROOT, args.tasks) else 1
    elif args.command == "audit":
        print(tasks.audit(ROOT, args.run))
    elif args.command == "compact":
        files, saved = runs.compact(ROOT, args.jobs or None)
        print(f"compressed {files} logs, saved {saved / 1e9:.2f} GB")
    elif args.action == "new":
        created = runs.new_run(ROOT, args.game, args.profile, args.id, args.account, args.dry_run, args.foreground)
        print(f"{created.id}: {'prepared' if args.dry_run else 'started'} ({created.dir})")
    elif args.action == "stop":
        runs.stop(ROOT, args.run, args.timeout)
    elif args.action == "checkpoint":
        print(runs.checkpoint(ROOT, args.run, args.allow_unsealed))
    elif args.action == "pause":
        runs.stop(ROOT, args.run, args.timeout)
        print(runs.checkpoint(ROOT, args.run))
    elif args.action == "resume":
        index = runs.resume(ROOT, args.run, args.account, args.note, args.foreground, args.dry_run)
        print(f"{args.run}: segment {index} {'prepared' if args.dry_run else 'started'}")
    elif args.action == "status":
        for row in runs.status(ROOT, args.run):
            print(json.dumps(row, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
