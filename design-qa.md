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

## Reopened geometry findings

- [P1] Recursive boxes render at only 74–76% of a logical cell, so each nested board is scaled from an undersized container and the recursion proportions drift.
- [P1] Every space receives an unconditional rectangular frame, including cells where the level map has an opening. This closes visible exits that should connect to the parent space.

The earlier pass checked data presence and viewport overflow but failed to verify these core grid invariants.

Fixes and post-fix evidence:

- Every recursive box face now measures exactly one logical cell at both inspected viewports: 66.06 × 66.05 px on desktop and 41.69 × 41.69 px on mobile. Nested box faces use the same rule.
- The unconditional grid border and shadow were removed. Each real wall cell now records its exposed top/right/bottom/left edges from adjacent map cells and draws bevels only on those edges.
- In the real `a4 / Enter` recursive state, the top opening and the player-side left opening both measured 0 px on all four borders. A real exposed wall edge measured 3 px only on its recorded sides.
- Post-fix desktop viewport: `$HOME/.codex/visualizations/2026/07/22/parabox-live-renderer/geometry-fill-boundary-desktop-viewport-v2.jpg`.
- Post-fix mobile viewport: `$HOME/.codex/visualizations/2026/07/22/parabox-live-renderer/geometry-fill-boundary-mobile-v2.jpg`.
- Post-fix focused source/implementation comparison: `$HOME/.codex/visualizations/2026/07/22/parabox-live-renderer/geometry-fill-boundary-comparison-v2.jpg`.

The focused comparison confirms that boxes retain one-cell scale, actual wall runs remain visually connected, and map openings remain open to the parent context. The official reference is a different level/state, so the comparison is limited to recursive scale and boundary grammar. No new typography, color, image-quality, copy, interaction, accessibility, or responsive regression was found; the mobile document remains exactly 390 px wide and browser diagnostics contain no warnings or errors.

## Corrected exit-boundary finding

- [P1] The left-side player exit had no map wall, but it still showed a rectangular boundary. The actual sources were not the wall renderer: the parent-context focus cell still rendered the complete colored box face behind the child space, the focused space and parent context had whole-rectangle shadows, and every cell had a generic 0.5 px inset grid line.

An intermediate diagnosis incorrectly proposed changing wall thickness. That wall change was reverted completely; walls retain the prior map-driven full-cell rendering and exposed-edge treatment.

Fixes and post-fix evidence:

- The parent focus cell no longer renders the containing `BoxFace`; it becomes the aperture occupied by the focused child scene.
- Whole-rectangle focus and parent-context shadows were removed.
- The generic per-cell inset line was removed. Scene floor and colored box/wall surfaces now provide the visual separation without closing exits.
- At the real left-side `a4 / Enter` exit, the exit cell, focused grid, focused-space wrapper, and parent focus cell each report 0 px borders, `box-shadow: none`, and `filter: none`. The parent focus cell has no child shell.
- Current implementation capture: `$HOME/.codex/visualizations/2026/07/22/parabox-live-renderer/exit-border-fixed.jpg`.
- Focused before/after evidence: `$HOME/.codex/visualizations/2026/07/22/parabox-live-renderer/exit-border-before-after.jpg`. The left image shows the incorrect complete cyan shell; the right image shows the same walls and boxes with the exit open directly into the parent scene.

## Final result

passed
