# Live platform v2 delivery — 2026-09-06

Data/service work is deployed at https://live.benchmark.3720.org, release `live-v2-controls-20260906`. The visual redesign is deployed and was checked with local Playwright after explicit user authorization.

## Validation

- 37 Rust tests passed (34 runtime, 3 audit); 17 web tests passed, including TypeScript checking and production build.
- 57 journals: SHA-256 of the concatenated decoded segments matched the exact original JSONL before retirement. Largest decoded segment: 16,672,478 bytes, below 16 MiB.
- 48,563 legacy gzip objects converted to verified zstd. Old gzip objects and raw historical journals retired after verified copies and migration receipts were durable. No active writers were present; none were interrupted for migration.
- Full pre-migration backup passed `zstd --test`: `~/.local/share/3720-benchmark-live/backups/history-before-zstd-20260906.tar.zst`.
- Public API matched all 48 run identities, scores and live flags. Six referenced public page assets returned success.
- After retirement, all seven game detail endpoints were sampled, plus replay for games with recorded attempts. Sausage and Swarm samples had no attempts.
- Docker integration passed: host heartbeat, partial inbox record, SSE delta, orphan invalidation, segmented continuation. See `docker-integration.json`.
- Replay stress exercised Operator, Kitchen, Parabox, Sokoban and Minesweeper, including continuation pages. See `replay-stress.json`.
- Idle Docker memory observation after deployment: web 47.13 MiB, gateway 13.18 MiB. Both have a 256 MiB hard limit with swap disabled; idle observations are not peak guarantees.

## Operations

The native site/gateway launch agents are disabled and backed up. Docker serves loopback ports 3000/3740 through the existing Cloudflare route. The two observer data mounts are read-only; rebuildable query indexes and immutable web assets have separate volumes. Recorder/Harbor remain on the host. Publication staged and validated a candidate before switching, and has rollback handling.

See `docs/live-platform-v2.md` for schemas, budgets, migration, replay pagination and rollback. The prior native gateway requires the pre-zstd data backup after source retirement.

## Visual and interaction delivery

The final UI follows the user's requests to remove low-value content and raise useful information density:

- Persistent game switcher, compact model list, compact navigation and model cards; mobile uses game/run selectors.
- Model, score, status and effective duration above the scene. Removed the wall clock, duplicated context, implementation labels and empty notes panels.
- History opens on the group containing the newest attempt. One group's compact, numbered attempts are shown at a time; successful attempts retain score labels. Group/history pagination remains available.
- Playback controls appear when replay frames exist. Granular jumps and export are under More; returning to latest remains available during pending replay loads.
- Score chart, visible Agent activity, operations, saved notes and runtime diagnostics are expandable.
- Fixed mobile Parabox clipping and reset the scroll position when navigating between runs and game dashboards.

Verification: TypeScript, production build, lint of the edited page and all 17 web tests passed. Desktop and 320/375/414/768 CSS-pixel viewport checks passed; seven game detail pages passed mobile overflow and browser-error checks. Dashboard range selection, game/run selection, replay selection, return-to-latest and disclosure interactions passed. A deliberately delayed replay was cancelled and its late response could not restore the old state. GIF export produced a valid 595,759-byte file. Container deployment checked all 48 runs and public assets. Evidence is saved alongside this report as `ui-*.json` and `ui-*.png`.


## Control refinement

The user subsequently requested removal of search and called out inconsistent native controls. Both search inputs and their filtering state were removed. `app/live-controls.tsx` now supplies styled Radix Select, Slider, DropdownMenu and Collapsible components. Comparison uses styled Radix toggles. Search is not present on the page; range/game/run/history selections remain available.

Browser checks passed for selecting options, slider arrow keys and pointer dragging, playback speed, Escape dismissal with focus returned to the menu trigger, disclosure controls, mobile game switching, range selection and curve toggles. Dropdowns fit 320/375/414/768-pixel viewports. GIF export via the new menu passed. TypeScript, production build, page/component lint and 17 web tests passed. See `controls-browser.json`, `controls-export.json`, and `controls-*.png`.

Component API references: [Radix Select](https://www.radix-ui.com/primitives/docs/components/select), [Slider](https://www.radix-ui.com/primitives/docs/components/slider), [Dropdown Menu](https://www.radix-ui.com/primitives/docs/components/dropdown-menu), [Collapsible](https://www.radix-ui.com/primitives/docs/components/collapsible).

## WebGL migration and export

All seven game views now render their game geometry and status text in WebGL.
Six games use shared Pixi drawing commands; Sausage retains Three.js geometry
and adds a Pixi HUD in the same context. Navigation and replay controls use Radix.
The DOM screenshot dependency was removed. Export renders authoritative states
into a separate GPU canvas without changing the visible replay cursor.

On the same Terra c9 attempt 23 at 4×, GIF export changed from 13,611 ms / 595,759
bytes to 809 ms / 436,289 bytes. Video exported in 834 ms / 126,782 bytes; decoding
confirmed 900×308, 8.776 seconds, and different first/last frames. Chrome measurements.

Added four browser regressions for repeated rollout selection, pending double
clicks, duplicate history entries and switching between runs. Seven additional
browser cases cover actual game-state fixtures, narrow screens, GPU pixel output,
WebGL context loss/restoration and GIF downloads. Typecheck, production build and
17 existing web tests pass; lint has no errors (one pre-existing unused-variable
warning in rendered-html.test.mjs). All seven live game views fit 320/375/768-pixel
viewports and produced no JavaScript or WebGL errors.

Published as `live-v2-webgl-20260906`. Both production containers are healthy.
Public and local APIs match all 48 run identities, scores and live flags; all
seven referenced public assets match the local release and load successfully.
All seven game detail pages render a ready WebGL canvas through the public URL
without browser errors. Cancellation generated no download. See
`webgl-validation.json`, `webgl-deployment.json`, and `webgl-*.png`.
