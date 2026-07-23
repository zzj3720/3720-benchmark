# Imported 911 Operator gameplay data

This directory is a deterministic, gameplay-only extraction from a locally
owned Steam installation of 911 Operator (app 503560, macOS build
7533741).

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
