# Parabox live renderer QA

## Rendering contract

The renderer follows two original-game sources:

- public Patrick's Parabox screenshots and recursion animation captured under
  `$HOME/.codex/visualizations/2026/07/22/parabox-original-reference/`;
- the `SceneLevel.ChildLocalScale()` and `SceneBlock.UpdateColor()` code in the
  Unity editor project bundled with the installed game.

Those sources establish one rendering rule for focused, parent, nested, and
cyclic spaces:

- A scene is a box and always occupies the complete parent cell.
- The scene floor fills that box. Its authored grid uses square cells sized
  `1 / max(width, height)` and is centered inside it.
- Recursive boxes contain that same scene projection directly. There is no
  separate box shell, inset frame, or unconditional outline.
- Only authored wall cells draw wall surfaces and exposed bevels. A boundary
  without a wall remains floor all the way to the box edge, so exits stay open.
- The parent scene uses the same centered-grid geometry and is positioned so
  the focused child cell exactly coincides with the focused scene.
- Cycles render real repeated scenes until the next nested cell is roughly
  pixel-sized; they never become a text placeholder.
- Level color triples are HSV values. They are converted to RGB before the
  observer applies floor, wall, and player lighting.

## 64-state coverage

`render-qa` replays the complete imported walkthrough catalog and selects one
high-value recursive state per eligible level before campaign-stratified
sampling. The current fixture contains 64 states from 345 eligible levels:

- depth: 1×9, 2×13, 3×27, 4×10, 5×3, 8×2;
- 44 cyclic states;
- 6 horizontally flipped states;
- the four available rectangular-level representatives: `k16`, `k17`,
  `k18`, and `s18`;
- focus-space boundary openings ranging from 1 to 30.

The gallery is served from the real Vite entry at
`http://localhost:4174/qa-gallery`. Its cards use the production Parabox
observer component, not a duplicate QA renderer.

## Visual E2E

The clean in-app-browser pass loaded 64 cards through the real gallery entry
and reported no application warning or error. All cards were inspected in
four-column rows, including:

- cards 5, 7, 8, 10, 11, and 12 for cyclic recursion;
- cards 33–35 and 56 for rectangular geometry;
- cards 29, 31, 59, and 63 for horizontal flips;
- cards 41 and 55 for depth-eight/player-scale extremes;
- cards 57–64 for late-campaign nested and open-boundary states.

The pass found and fixed four additional failures rather than accepting the
first gallery output:

- recursive intrinsic content expanded a 220 px camera to 4,374 px;
- a fixed seven-level recursion could request roughly 651 million cells for
  `m23`;
- rectangular spaces stretched their cells instead of using the original
  `1 / max(width, height)` scale;
- authored HSV values were interpreted as RGB channels.

After the fixes, the inspected camera and focus space are both exactly
240×240 px, the 64-state projection is about 10,844 cells, and a fresh gallery
load remains responsive.

## Evidence

- Original/current comparison:
  `$HOME/.codex/visualizations/2026/07/22/parabox-live-renderer-v2/comparison-original-current.jpg`
- Authored colors and full-cell boxes:
  `$HOME/.codex/visualizations/2026/07/22/parabox-live-renderer-v2/01-top.jpg`
- Cyclic recursion and open boundaries:
  `$HOME/.codex/visualizations/2026/07/22/parabox-live-renderer-v2/02-cycles.jpg`
- Rectangular scenes:
  `$HOME/.codex/visualizations/2026/07/22/parabox-live-renderer-v2/03-rectangles.jpg`
- Final campaign states:
  `$HOME/.codex/visualizations/2026/07/22/parabox-live-renderer-v2/04-final.jpg`

## Automated verification

- `cargo fmt --manifest-path games/parabox-intro/Cargo.toml -- --check`
- `cargo test --manifest-path games/parabox-intro/Cargo.toml`
- `vp lint` in `observer-platform/`
- `vp run test` in `observer-platform/`
- `git diff --check`
