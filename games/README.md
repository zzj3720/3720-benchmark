# Games

Each directory under `games/` owns one complete benchmark game. A game keeps
its rules implementation, model-facing API, scoring and replay verification,
campaign or scenario data, development and packaging scripts, and live-console
renderer in the same vertical slice.

The common layout is:

```text
games/<game>/
├── data/       # Authoritative campaign, scenario, Oracle, and provenance data
├── observer/   # Game-specific live-state metadata and rendering
├── scripts/    # Import, extraction, packaging, and game-specific checks
├── src/        # Game engine, API, scoring, and verifier implementation
└── ...         # Build manifests, tests, licenses, and game documentation
```

Not every game uses the same language or needs every subdirectory. The
ownership boundary is the important part: game-specific behavior belongs here.
`observer-platform/` provides only the shared live-console shell and transport,
while `tasks/` contains generated, self-contained Harbor packages.
