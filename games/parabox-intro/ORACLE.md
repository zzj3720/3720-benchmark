# Oracle provenance

The benchmark's reference histories are not presented as human-authored
solutions.

The source is the public
[Steam Community Direction Input Walkthrough](https://steamcommunity.com/sharedfiles/filedetails/?id=2786724419).
The importer maps all 364 original level references and chapters. The
authoritative reference, title, area, kind, predecessor, and immediate-unlock
metadata is recorded in
`data/campaign/index.tsv`.

Every transcribed direction history is replayed against the extracted original
level asset by the Rust rules engine. This makes the walkthrough a compatibility
test: if a known original-game solution does not solve locally, the engine is
wrong or the level/version mapping must be reviewed.

`solve-level` is an offline breadth-first search tool for development fallback.
It was not needed for the 364 imported reference histories. Neither the
walkthrough histories nor `solve-level` is included in the Agent environment.
