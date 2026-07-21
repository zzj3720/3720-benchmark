# Parabox live renderer Design QA

## Visual truth

- Official product page: <https://store.steampowered.com/app/1260520/Patricks_Parabox/>
- Recursive blue-room reference: `$HOME/.codex/visualizations/2026/07/22/parabox-live-renderer/steam-fc9994b3.jpg`.
- The commercial screenshot is a QA reference only and is not committed to this repository.

The comparison uses the official screenshot for visual grammar: blue recursive rooms, beveled boundaries, strong player/box/goal colors, and visible space nesting. It is a different level and state, so positional matching is intentionally not claimed.

## Comparison evidence

- Desktop implementation, 1280 × 1000: `$HOME/.codex/visualizations/2026/07/22/parabox-live-renderer/recursive-outer-final.png`.
- Mobile implementation, 390 × 844: `$HOME/.codex/visualizations/2026/07/22/parabox-live-renderer/recursive-mobile-final.jpg`.
- Source and desktop implementation inspected together: `$HOME/.codex/visualizations/2026/07/22/parabox-live-renderer/recursive-comparison-final.png`.
- Focused recursive-state capture: `$HOME/.codex/visualizations/2026/07/22/parabox-live-renderer/recursive-interior-v1.png`.

The final view shows the actual immediate parent space around the focused room and the actual child space inside a recursive box. Room boundaries, nested scale, entity proportions, target distinction, and hierarchy read consistently with the reference. The faint cell grid, labels, and status text remain intentionally observer-specific.

## Positive E2E and manual checks

- Started the real Rust sidecar entry point, wrote its private raw event stream, and subscribed through the real live gateway and observer UI.
- Verified a recursive state with path `ROOT › BOX 2`: parent exterior visible, focused room centered, and one real child-space interior visible.
- At 1280 × 1000, verified one outer-space context, one recursive interior, no legacy warning, and no horizontal overflow.
- At 390 × 844, verified a 390 px document width, a 362 px scene stage, a 208 px focused room, visible outer context, and a real child-space interior.
- Selected the oldest replay state and confirmed its recorded recursive interior remained visible.
- Loaded a legacy event without the recursive snapshot and confirmed the explicit missing-data notice, zero fabricated interiors, and zero fabricated outer-space context.
- Checked browser diagnostics after the final mobile pass: no warnings or errors from the application.
- Confirmed the model response and the public per-game observer relay do not expose the private observer scene.

## Self-review 1 — state, data, and fairness

Findings fixed:

- The first renderer flattened only the focused character map. The sidecar now emits a private, observer-only graph snapshot containing the root, focus, actual parent relation, relevant child spaces, blocks, flips, and compact string rows.
- The graph traversal now includes only focus ancestors and reachable child spaces, is cycle-safe, and avoids unrelated infinite zones.
- The model-facing command response remains unchanged, and the common observer relay strips the private `scene` field. The live gateway alone projects it into `observer_scene` for the trusted dashboard.
- Parent focus cells no longer render a hidden duplicate subtree, empty cells cannot become focus containers, and camera/box horizontal flips are applied consistently.
- Run switching clears stale detail, replay attachment is independent from the newest cursor position, and expired replay cursors return to live state explicitly.
- Continuation resume lookup accepts direct artifacts, legacy recovery directories, and pause checkpoint manifests; an atomically replaced note no longer fails the whole run.

## Self-review 2 — visual clarity, accessibility, and regression risk

Findings fixed:

- The authoritative game scene now appears before the score chart, with the action explanation and replay controls next to the state they affect.
- The parent space continuously surrounds the focused room; recursive boxes render real mini-boards rather than decorative faces. Recursion is capped at four visible levels because deeper content is sub-pixel, and cycles have an explicit marker.
- Solid room geometry and beveled boundaries replace noisy per-cell diagonal walls. Player, boxes, both goal types, and changes are named in the legend and semantic cell labels.
- Important labels were enlarged, muted contrast was raised, and native replay controls expose keyboard focus plus `aria-pressed` / `aria-current` state.
- The legacy path is honest: old events state that interior and outer-scene data were not recorded and never invent either view.
- Desktop and mobile have no horizontal overflow; the legacy notice is anchored to the scene rather than the page.

No P0, P1, or P2 visual issue remains in the final comparison. The remaining difference from the official screenshot is the expected level/state mismatch and observer-specific telemetry.

## Automated verification

- `cargo fmt --manifest-path games/parabox-intro/Cargo.toml -- --check`
- `cargo test --manifest-path games/parabox-intro/Cargo.toml`
- `cargo fmt --manifest-path tools/observer-relay/Cargo.toml -- --check`
- `cargo test --manifest-path tools/observer-relay/Cargo.toml`
- `uv run python -m unittest live-gateway/test_server.py`
- `vp lint`
- `vp run test` (production build plus Node tests)
- `git diff --check`

## Superseded P0

The earlier version omitted real box interiors and the outer parent scene. The final observer-only recursive snapshot and renderer resolve both omissions, with positive real-entry E2E evidence above.

## Final result

passed
