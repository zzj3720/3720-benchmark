# 3720 Live Operations

`observer/web/` is the public console at
[live.benchmark.3720.org](https://live.benchmark.3720.org), deployed as one
Cloudflare Worker. It holds the vinext web app, the read-only `/api/live` API
and authenticated ingest (`worker/live-api.ts`), and the `LiveHub` Durable
Object that keeps run summaries and streams updates (`worker/live-hub.ts`,
protocol logic in `worker/live-hub-state.ts`). Run bodies live in the
`benchmark-live` R2 bucket. See [`docs/live-platform.md`](../../docs/live-platform.md)
for the publishing contract, storage layout, and deployment.

It is read-only and shows real Harbor runs rather than browser fixtures:

- a separate scoreboard for each game registered in `app/game-registry.tsx`
  (currently Parabox, Swarm, Sausage, Emergency Operator, Kitchen Terminal,
  Minesweeper, and Sokoban);
- one score-over-effective-agent-time series per model, measured from the
  logical run's first eligible Agent execution window and excluding pauses,
  infrastructure-only attempts, and gaps between continuation segments;
- a drill-down for the selected run with its current objective, authoritative
  environment state, latest action/result, visible native Agent session
  messages, and recent append-only events;
- both active trials and the most recent saved result for each game/model.

The browser holds one Server-Sent Events subscription
(`GET /api/live/v1/subscribe?protocol=2[&run_id=<run-id>]`). The first message
is a snapshot; later messages contain changed runs, removed IDs, appended score
points, and the feed status. When the benchmark host stops publishing, the page
says so, keeps the last state, and stops advancing live durations at the last
heartbeat. Run detail, older catalog pages and bounded replay windows are
fetched on demand; assets are immutable and use a bounded browser cache.

Emergency Operator publishes read-only clock snapshots once per second after a
shift starts, so calls, ETAs, incident health, alarms, and score remain live
while the Agent is waiting. These observer ticks never deliver an alarm or
enter the scoring audit.

## Development and deployment

```bash
npm --prefix observer/web ci
npm --prefix observer/web test        # typecheck, build, node tests
observer/web/scripts/deploy.sh        # build and `wrangler deploy`
```

`npm run dev` serves the app with a local `LiveHub` and R2 (Miniflare). Point
a local `live-publisher --endpoint http://127.0.0.1:<port>` at it with the
`LIVE_INGEST_TOKEN` you give `wrangler dev --var`. `VITE_LIVE_GATEWAY_ORIGIN`
makes the browser read another origin's API, for example the production site.
The browser tests (`npm run test:browser`) serve the standalone build against
fixtures.

## Render QA

The development-only gallery renders ignored, locally generated fixtures through
the production observer components. Parabox samples 64 recursive states from
its complete walkthrough. Sausage samples 64 entry/mid/final states across the
86-puzzle, 11,769-action walkthrough and includes height, grills, ladders,
detached forks, cooked faces, multi-island puzzles, and exit-ready states.

Generate the fixture you need before starting the dev server:

```bash
vp run qa:parabox:data
vp run qa:sausage:data
vp dev
```

Then open `http://localhost:5173/qa-gallery` for Parabox or
`http://localhost:5173/qa-gallery?game=sausage` for Sausage (using the port
printed by Vite). Sausage is paginated to eight WebGL scenes at a time so the
browser never creates all 64 contexts at once.
