# Live platform: protocol, storage and operations

Benchmarks run locally; the public console runs on Cloudflare. The run
recorder writes each run's authoritative journal on the benchmark host. The
`live-publisher` beside it projects every run with the same Rust code the local
gateway serves and pushes the results to Cloudflare, where a Worker, one
Durable Object and an R2 bucket serve [live.benchmark.3720.org](https://live.benchmark.3720.org).
If the benchmark host goes away, the site keeps serving the last published
state and tells viewers the feed is offline; when the host returns, the
publisher resumes from what Cloudflare already holds.

The logical-run identity, effective-time rules and checkpoint ownership in
`tracks/live-observability.md` remain in force.

## Components

| Path | Role |
|---|---|
| `games/<game>/observer/` | Game-specific live-state metadata and WebGL scene |
| `observer/relay/` | Sidecar relay that projects native Parabox/Swarm logs into the common observer API |
| `observer/runtime/` | Rust `run-recorder`, `live-publisher`, `live-gateway` (local inspection), `run-archive`, `run-audit` |
| `observer/web/` | The console: vinext app, Worker read API and ingest (`worker/live-api.ts`), `LiveHub` Durable Object |
| `tools/observer/` | Harbor-side hook: `harbor-run` and `RunJournalPlugin`, which start the recorder |

```text
benchmark host                                   Cloudflare
  Harbor ─► run-recorder ─► .harbor/run-journals   R2 bucket benchmark-live
                              │                      pub/…  published bodies
                              ▼                      raw/…  journal backup (never routed)
                        live-publisher ── HTTPS ──►  LiveHub (run summaries, SSE fan-out)
                                                     Worker (web app + /api/live read API)
```

## Per-game observer protocol

Each scored sidecar supports:

```text
GET /v1/observe/snapshot
GET /v1/observe/events?after=<sequence>&limit=<1..1000>&wait_ms=<0..30000>
```

Normalized events use `benchmark-observer-event-v1` and carry a monotonically
increasing sequence, source timestamp, task identity, action, post-action
dynamic state, and compact result. Large immutable scene data is emitted once
as a gzip-compressed observer asset and referenced by content hash. Reads do
not advance a virtual clock, trigger cooldowns, alter scoring, or enter the
Agent command audit.

- Parabox projects its native `parabox-events-v1` log.
- Swarm projects its `swarm-audit-v1` initial state and command responses.
- Sausage writes the common event schema directly.

Parabox native records additionally contain a private recursive scene graph for
the host dashboard. The common sidecar relay strips that field so richer human
rendering does not expand the state available to the benchmark Agent.

## Recorder

Runtime lifecycle, sidecar game records, visible Agent messages, tool actions
and workspace notes arrive in one chain journal with one identity, one
sequence, and an already-stamped effective Agent time. Runs created before the
recorder are frozen in `.harbor/live-archive`.

Recorder-aware sidecars append to a durable trial inbox. The recorder tails it
incrementally and drains it idempotently by source sequence; the file itself is
also the recovery buffer. Live ingestion never polls sidecar state.

An unsealed segment is live only while its recorder holds the chain's writer
lock. Losing it without a durable `segment_finished` record projects as
`orphaned`, not a false `running`.

Score charts normalize every run to cumulative Agent execution time starting
at `0h`; the horizontal axis never uses calendar time. Continuation gaps and
infrastructure-only failed segments are excluded.

## Local layout

```text
.harbor/
  run-journals/<chain_id>/
    writer.lock
    writer-lease.json                  # short-lived recorder heartbeat
    journal.jsonl                      # open append-only tail, when present
    journal-index.json                 # atomic, versioned segment inventory
    history/<first>-<last>-<hash>.jsonl.zst
    objects/<decoded-content-sha256>.json.zst
  live-archive/                        # legacy public projections
  live-publish/
    index/<chain_id>/projection-v6.sqlite  # disposable query index
    ledger.sqlite                          # what Cloudflare has accepted
```

The query index and the ledger are disposable. Deleting the index loses no
scores or replay evidence; deleting the ledger makes the publisher resend
everything.

## Event and object formats

`benchmark-run-event-v1` remains the authoritative envelope. Its `chain_id`,
`segment_id`, `sequence`, `source_sequence`, source timestamp and
`effective_elapsed_ms` retain their original meanings. Compression never
rewrites the envelope or recalculates score/time. Score changes include both
rewards and penalties. The recorder commits the complete encoded record and
newline before advancing its sequence or acknowledging the inbox source.

New game payloads contain a compact context beside their object references:

```json
{
  "context": {
    "schema": "benchmark-observation-context-v1",
    "kind": "shift",
    "reference": "shift:1:Skyscraper_Test_1p",
    "title": "Skyscraper_Test_1p",
    "boundary": "episode",
    "complete": false
  },
  "state_snapshot": {
    "schema": "benchmark-object-v2",
    "object": "<sha256 of decoded JSON bytes>",
    "encoding": "zstd",
    "media_type": "application/json",
    "uncompressed_bytes": 34882,
    "compressed_bytes": 2800,
    "content_sha256": "<same sha256>"
  }
}
```

Sizes above are illustrative. Supported context kinds are `level`, `overworld`,
`shift` and `world`. Puzzle attempts can close on a positive score; continuous
games close on an episode boundary. Failed and unfinished attempts are retained.
An attempt's score is its net score change, not the run's cumulative score.
Legacy state fields and Sausage's pre-action context are normalized while
building the index; the old journal bytes remain unchanged.

The new object ID hashes decoded bytes, so it does not depend on a codec.
Migrated legacy objects keep their old IDs as aliases. Readers support both
`.json.gz` and `.json.zst`, prefer zstd, and verify decoded size/hash when the
descriptor provides them. HTTP JSON still uses ordinary gzip negotiation;
storage compression and wire compression are independent. SSE is not compressed.

## Segments and memory

The default limit is **16 MiB of decoded JSONL**, configurable to 32 MiB with
`BENCHMARK_HISTORY_CHUNK_MIB=32` for a new chain or `run-archive --chunk-mib 32`
for migration. Existing chains retain their chosen segment size. A record is
never split; the final segment can be smaller. An individual oversized record
is rejected by the archive writer rather than silently exceeding the limit.

```json
{
  "schema": "benchmark-journal-index-v2",
  "chunk_bytes": 16777216,
  "chunks": [{
    "file": "history/<first>-<last>-<hash>.jsonl.zst",
    "first_sequence": 1,
    "last_sequence": 8000,
    "records": 8000,
    "uncompressed_bytes": 16777000,
    "compressed_bytes": 900000,
    "content_sha256": "<sha256 of exact decoded segment bytes>"
  }]
}
```

The recorder rotates a full tail while it owns the writer lock, and seals a
finalized tail at shutdown. Each segment is streamed through a zstd encoder,
checksummed, read back for verification, and made durable before the atomic
index switch. Only then is the retired tail removed. A crash between index
publication and tail retirement can leave a duplicate prefix; readers/indexing
deduplicate that prefix by authoritative sequence. Continuation appends a new
tail and streams prior segments for recovery, without expanding all history to
disk or RAM.

## Publishing

`live-publisher` watches every chain's authority and writer lock. For each run
that changed it stores, under `pub/` in R2:

| Key | Body | Cache |
|---|---|---|
| `pub/runs/<run>/detail.json` | `GET /v1/runs/<run>` | `no-store` |
| `pub/runs/<run>/catalog/<before>.json` | `?catalog_before=<before>` | immutable |
| `pub/runs/<run>/attempts/<attempt>.json` | `?replay_attempt=<attempt>` | immutable once the attempt closes |
| `pub/runs/<run>/attempts/<attempt>.preview.json` | `…&preview=1` (first frame only, for thumbnails) | as above |
| `pub/assets/<sha256>` | `GET /v1/assets/<sha256>` | immutable |
| `pub/covers/<run>/<attempt>.webp` | `GET /v1/covers/<run>/<attempt>` (baked, see below) | immutable |

Bodies are byte-identical to the local gateway's responses (both call the same
projection functions) and stored gzip-encoded. Older catalog pages and closed
attempts never change, so each is uploaded once; the open attempt's replay is
refreshed at most every 15 seconds. The authority itself is backed up under
`raw/<chain>/`: sealed history and objects in content-named tar bundles
(`bundles/<digest>.tar`, about 8 MiB each), the index and the open tail (every
10 minutes) as individual files.

A replay body is one whole attempt: one level (or one stretch of the
overworld) between resets or level changes. It is not split by event count,
so undo history and the starting board never cross a page boundary. The
largest attempts are a few MiB gzip-encoded. Every recorded frame is kept;
frames the agent undid (and the undo/redo that moved over them, and resets)
carry `undone: true`. The console hides them by default and a toggle plays
the attempt as it really happened. An undo whose target is not in the attempt
stays visible, so the board steps back rather than jumping.

After the bodies, it pushes changed run summaries (with score history) to
`LiveHub`, then heartbeats every 10 seconds. The ledger is written only after
Cloudflare accepts a batch, so a crash or network failure resends rather than
skips. Verify a publisher export against the gateway with
`observer/runtime/scripts/compare_publish.py`.

Ingest is `POST /api/ingest/{objects,runs,heartbeat}` with
`Authorization: Bearer $LIVE_INGEST_TOKEN`. Objects arrive batched in one frame
(u32 big-endian manifest length, manifest JSON, then each body); keys must be
under `pub/` or `raw/`.

## Serving

The Worker answers `/api/live/*` before the web app:

```text
GET /api/live/health
GET /api/live/v1/runs
GET /api/live/v1/runs/<id>
GET /api/live/v1/runs/<id>?catalog_before=<attempt-id>
GET /api/live/v1/runs/<id>?replay_attempt=<attempt-id>[&preview=1]
GET /api/live/v1/assets/<sha256>
GET /api/live/v1/covers/<run>/<attempt>
GET /api/live/v1/subscribe?protocol=2[&run_id=<id>]
```

Run bodies and assets come from R2 as stored (gzip), with their cache
headers; browsers cache immutable ones. `raw/` is never routed. `LiveHub` holds run summaries in its SQLite
storage and serves the subscription as Server-Sent Events: a `reset` snapshot,
then only changed runs (with `score_history_delta` when history grew by an
unchanged prefix), removals, and the selected run's `detail_revision`. Every
message carries `feed: {connected, last_seen_ms, publisher}`.

Without a heartbeat for 45 seconds `LiveHub` marks the feed disconnected and
broadcasts it. Runs keep their last published state; the console shows the
time of the last heartbeat and stops extrapolating live durations there. The
next heartbeat reconnects the feed.

`LiveHub` is a single Durable Object that stays idle when nobody watches and
the feed is offline. SSE streams keep it resident while viewers are connected;
at this site's scale that stays within the Workers free allowance.

## Deploying

One-time setup, from a machine logged in with `wrangler login`:

```sh
npx wrangler r2 bucket create benchmark-live
openssl rand -hex 32 > ~/.config/3720-benchmark/live-ingest-token
npx wrangler secret put LIVE_INGEST_TOKEN -c observer/web/dist/server/wrangler.json < ~/.config/3720-benchmark/live-ingest-token
```

Deploy the console (set `LIVE_CUSTOM_DOMAIN=live.benchmark.3720.org` to attach
the public hostname):

```sh
LIVE_CUSTOM_DOMAIN=live.benchmark.3720.org observer/web/scripts/deploy.sh
```

On the benchmark host, install the publisher as a LaunchAgent:

```sh
observer/runtime/scripts/install_publisher.sh https://live.benchmark.3720.org ~/.config/3720-benchmark/live-ingest-token
```

Roll the Worker back with `npx wrangler rollback`. The R2 bodies and `LiveHub`
state are unaffected by Worker versions.

## Level covers

Level cards (the replay library and the shelf under a run's live view) show
the first frame of an attempt as a 640x400 WebP cover. Rendering these in the
viewer's browser took seconds per page, most of all for Sausage's 3D scenes, so
they are baked once: `observer/web/scripts/bake-covers.mjs` collects every
card's attempt from the public API, renders the covers missing from R2 in
headless Chrome through the console's own `?bake=covers` page, and uploads them
through the ingest endpoint as `pub/covers/<run>/<attempt>.webp`. An attempt's
first frame never changes, so a cover is baked once and kept.
`observer/web/scripts/install_cover_baker.sh <site> <token-file>` runs it every
15 minutes as a LaunchAgent. A card whose cover is not baked yet renders it in
the browser, the same way.

Covers use the renderers' cover mode (`createSession(..., { cover: true })`,
`Viewport.cover`): the board alone at cover size, without headers, side panels
or the Sausage HUD. The result is trimmed to its content and centred in 16:10.

## WebGL scenes and direct replay export

All seven game scenes now use WebGL. Six 2D adapters emit geometry/text commands
for a shared Pixi WebGL renderer. Sausage keeps its Three.js terrain, entities
and camera; its status and cooking HUD share the same WebGL context. Application
navigation, controls and accessible hover descriptions remain React/Radix.

Replay export creates an independent offscreen renderer from authoritative frame
states. It never moves the visible replay cursor or clones DOM. Frames are drawn
and encoded one at a time; output is capped at 900×900 and 80 million total
pixels, while visible framebuffers are capped at four million pixels. A lost
WebGL context can recover and repaint. Renderer teardown releases GPU resources;
the QA gallery paginates at eight scenes to avoid exceeding browser context limits.

GIF uses 128-color quantization. Video prefers H.264 MP4 and uses WebCodecs through
Mediabunny with quality mode, explicit frame timestamps, encoder backpressure,
and a completed-frame check. The MediaRecorder fallback holds and encodes each
still independently, verifies its keyframe, then remuxes one packet per replay
frame onto the same explicit timeline. It retries an uncaptured still instead of
advancing past it. Neither path samples a running playback clock. Cancellation
is checked between frames and the encoder yields for progress/UI updates.

Chrome measurement, same Terra c9 attempt 23 at 4×: GIF 13,611 ms before versus
809 ms after (about 17× faster); video 834 ms for an 8.776-second 900×308 WebM.
These are local measurements, not a cross-device performance guarantee.
