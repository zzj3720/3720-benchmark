# Parabox source tooling

`extract_levels.py` reads the official `TextAsset` levels from a locally
installed, legally owned copy of Patrick's Parabox. The default command
extracts all 364 puzzles together with their original areas, branch
relationships, optional status, and unlock metadata:

```bash
uv run --with UnityPy games/parabox-intro/scripts/extract_levels.py \
  --output games/parabox-intro/data/campaign/levels
```

The script deliberately does not upload, download, or discover game copies. It
pins the inspected Steam build and records source hashes without recording the
Steam owner's account ID.

The public direction walkthrough importer is Rust:

```bash
cargo run --release --manifest-path games/parabox-intro/Cargo.toml \
  --bin parabox-import-walkthrough -- \
  --output games/parabox-intro/data/oracle/oracle.tsv
```

After extraction, package the tested game into the self-contained Harbor task:

```bash
games/parabox-intro/scripts/package_task.sh
```

Original game data is copyrighted by its respective owner. Keep extracted
levels private unless redistribution permission is obtained.
