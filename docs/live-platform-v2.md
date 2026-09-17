# Live platform: data, storage and operations

This is the current deployment and storage contract. The logical-run identity,
effective-time rules and checkpoint ownership in `tracks/live-observability.md`
remain in force. The v2 storage layout replaces whole-file gzip history and the
old in-memory/full-history gateway projection.

## Ownership and layout

Harbor and the recorder remain on the host. The recorder owns game/runtime
ingestion, writer.lock, and the authoritative event sequence. Docker runs two
read-only public services: `web` and `gateway`. Neither mounts a Docker socket,
Agent sessions, or the entire repository. Only the two observer data directories
are mounted; the query index lives in a separate writable cache volume.

The public Cloudflare Tunnel runs separately in `benchmark-live-cloudflared`,
on the `benchmark-live_default` network. It resolves edge endpoints using its
explicit container DNS servers, independent of the host's virtual DNS settings.
Its existing credentials and ingress config are mounted read-only; ingress
targets the `web` and `gateway` service names. The previous native live-tunnel
LaunchAgent is disabled. `.harbor/live-deploy/start-tunnel.sh` starts or recreates
the connector with the saved configuration.

```text
.harbor/
  run-journals/<chain_id>/
    writer.lock
    writer-lease.json                  # short-lived recorder heartbeat
    journal.jsonl                      # open append-only tail, when present
    journal-index.json                 # atomic, versioned segment inventory
    history/<first>-<last>-<hash>.jsonl.zst
    objects/<decoded-content-sha256>.json.zst
  live-archive/                        # legacy public projections, now zstd
  archive-migration.json               # most recent migration summary
  archive-receipts/<migration>/*.jsonl.zst # per-file checksums, also segmented
  live-deploy/current.json             # deployed image tag + rollback tag

Docker volumes:
  benchmark-live-cache-v2/<chain_id>/projection-v6.sqlite
  benchmark-live-assets/               # immutable chunks from web releases
```

The query index is disposable. Deleting it loses no scores or replay evidence;
the gateway rebuilds it by streaming the authority, one source segment at a time.
The health endpoint stays unready until the initial index scan completes.

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

The gateway uses these budgets:

| Resource | Budget |
|---|---:|
| Web container | 256 MiB, no swap; V8 heap 160 MiB |
| Gateway container | 256 MiB, no swap |
| Concurrent blocking query work | 2 |
| Concurrent replay assembly | 1 |
| SQLite page cache per connection | 4 MiB; temporary data on disk |
| Retained detail cache | 32 MiB weighted estimate; oversized entries bypass it |
| Replay input per page | about 1 MiB decoded state/trace data, at most 128 events / 1,024 traced instructions |
| Single replay operation | at most 4 MiB decoded state + trace |
| Attempt catalog page | 200 attempts |
| Score chart | at most about 2,050 sampled points per run |
| Recent Agent activity | 20 messages live; 100 per replay window |
| Browser immutable-asset cache | 16 MiB weighted estimate |

Weighted estimates account for object overhead; they are not measurements of
exact allocator usage. Container limits are the hard boundary. Large histories
are sampled only for drawing charts; the index and authoritative segments retain
every score event. `score_history_sampled` and `score_history_points` describe
this distinction. Replay/catalog pagination does not retain every prior page in
browser memory. Playback automatically fetches the next fragment. Export operates
on the current replay fragment and caps raster size/total pixel work. Video keeps
every selected replay frame and uses the playback speed to set its timestamps;
rendering or capture delays do not remove frames or change the output duration.

## Read API and state transitions

```text
GET /health
GET /v1/runs
GET /v1/runs/<id>
GET /v1/runs/<id>?catalog_before=<attempt-id>
GET /v1/runs/<id>?replay_attempt=<attempt-id>&after_sequence=<sequence>
GET /v1/assets/<sha256>
GET /v1/subscribe?protocol=2&run_id=<id>
```

The web proxy exposes these under `/api/live` and forwards cancellation. Assets
keep immutable cache headers. `catalog_before` returns `groups`, `more` and
`before`. Detail includes the first catalog page; replay returns
`next_after_sequence` when another fragment is available.

SSE v2 starts with `reset: true`, `runs` containing the initial snapshot and an
empty `removed` array. Later messages contain only changed runs, deleted IDs,
and `score_history_delta` when the history grows by an unchanged prefix. A
corrected/downsampled history is sent as a replacement `score_history` array.
Each subscriber retains fingerprints rather than copies of all historical
states. A reconnect starts a new snapshot. `protocol=1`/no protocol keeps the
legacy full-snapshot feed for old pages.

`detail_revision` includes lifecycle/lease state as well as sequence, so losing
the recorder invalidates detail even when no new event was written. The
`execution` object supplies an active-window timestamp/elapsed anchor; the
browser extrapolates only while that window and the connection are active.
Published historical points always use their recorded effective timestamps.

Details use one in-flight request with coalesced revisions, cancellation and
bounded retries independent of new game events. Errors preserve the last good
snapshot and show a retry action. Returning to live invalidates pending replay
requests. Holding an old frame freezes its replay batch instead of allowing
incoming details to pull the cursor forward. Export cancels on unmount and
blocks controls that could change its source frame.

The host writes `benchmark-writer-lease-v1` once per second. Docker uses its
freshness (five seconds), health, and explicit runtime identity rather than host
file-lock ownership. Filesystem notifications provide normal updates; a small
two-second metadata/lease scan also handles notifications lost across the VM
mount. Neither path polls game state. SSE closes on shutdown; restart policies
are managed by Docker.

## Migration and deployment

Back up the two data directories first. The archive CLI runs on the same host
as the recorder so its exclusive writer-lock checks are meaningful:

```sh
cargo build --release --locked --manifest-path tools/observer/runtime/Cargo.toml --bins
tools/observer/runtime/target/release/run-archive --root . --chunk-mib 16
tools/observer/runtime/target/release/run-archive --root . --chunk-mib 16 --apply
```

The dry run reports planned segments and size totals. `--apply` writes verified
zstd copies and receipts while keeping old sources. After the Docker service
passes identity/score/replay checks, add `--retire` to remove verified legacy
sources. Active writers and unsealed segments are skipped. Migration is
repeatable; original event bytes and legacy IDs are preserved. To inspect a
chain without materializing it:

```sh
tools/observer/runtime/target/release/run-archive --cat .harbor/run-journals/<id>
```

Deploy from the repository root:

```sh
npm --prefix observer-platform ci
npm --prefix observer-platform test
cargo test --manifest-path tools/observer/runtime/Cargo.toml
python3 observer-platform/scripts/publish_docker.py --preview
python3 observer-platform/scripts/publish_docker.py
```

The publisher builds immutable image tags and the host recorder, starts a
candidate at 14000/14740, checks HTML/assets/API/SSE and compares scores before
stopping anything. It then switches the existing 3000/3740 origins. Failure
restores the prior Docker tag or the existing native services. Shared immutable
web assets cover cached HTML during a switch. The Cloudflare hostname/Tunnel
routing remains unchanged.

For a Docker rollback, read the previous image tag from
`.harbor/live-deploy/current.json` and run Compose with `LIVE_RELEASE=<tag>`.
Do not remove the data directories or shared volumes. Returning to a pre-zstd
native gateway after retiring legacy sources requires restoring the data backup
first. Docker/OrbStack must be running for restart policies to take effect.

Relevant upstream references: [Compose health dependencies](https://docs.docker.com/compose/how-tos/startup-order/),
[zstd streaming encoder](https://docs.rs/zstd/latest/zstd/stream/write/struct.Encoder.html),
[rusqlite connections](https://docs.rs/rusqlite/latest/rusqlite/struct.Connection.html).

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
