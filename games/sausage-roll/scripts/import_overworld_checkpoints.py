#!/usr/bin/env python3
"""Package original-engine post-puzzle overworld saves in campaign order."""

import argparse
import gzip
import io
import tarfile
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("entries", type=Path)
    parser.add_argument("save_directory", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--run-prefix", required=True)
    args = parser.parse_args()

    with tarfile.open(args.entries, "r:gz") as archive:
        ordered = sorted(
            member.name.rsplit("/", 1)[-1].removesuffix(".state")
            for member in archive.getmembers()
            if member.isfile() and member.name.endswith(".state")
        )

    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("wb") as output:
        with gzip.GzipFile(filename="", mode="wb", fileobj=output, mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w") as archive:
                for stem in ordered:
                    _, level_id = stem.split("-", 1)
                    matches = list(
                        args.save_directory.glob(f"{args.run_prefix}*_{level_id}.sav")
                    )
                    if len(matches) != 1:
                        raise SystemExit(
                            f"expected one save for {level_id!r}, found {len(matches)}"
                        )
                    source = matches[0].read_bytes()
                    info = tarfile.TarInfo(f"{stem}.state")
                    info.size = len(source)
                    info.mtime = 0
                    info.mode = 0o644
                    archive.addfile(info, io.BytesIO(source))


if __name__ == "__main__":
    main()
