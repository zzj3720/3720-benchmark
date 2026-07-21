# Live observability architecture

The live system must represent one logical benchmark run across pauses,
continuations, infrastructure failures, and multiple Harbor trials. A game
sidecar owns game truth and the Agent runtime owns execution truth, but those
facts must be committed by one runtime-owned recorder into one authoritative
run journal when they are produced. The live gateway reads that journal; it
does not join independent histories later. No layer may infer another layer's
identity from a filesystem path or Docker name.

## Required behavior

- A logical run has one stable identity from its first segment through every
  valid continuation.
- The score chart's x-axis is cumulative eligible Agent execution time, not
  wall-clock time and not the age of the latest container.
- Pauses and gaps do not consume time. Attempts that never enter Agent
  execution do not consume time. Explicitly discarded infrastructure segments
  do not consume time.
- A checkpoint preserves the native Agent session, workspace, private game
  state, game audit, observer events, and runtime lineage as one atomic bundle.
- The browser uses one Server-Sent Events subscription. Selecting a run changes
  the subscribed projection; it does not start a polling loop.
- Game-specific state projection and rendering stay in `games/<game>/`; common
  lifecycle, timing, discovery, and transport stay outside individual games.
- Every published score point is one durable journal event that already
  contains its cumulative effective execution time.

## Three planes

### 1. Game data plane

Each game sidecar remains authoritative for only:

- current private game state;
- accepted Agent actions and their results;
- score and objective changes;
- deterministic audit/replay data;
- an append-only, read-only observer stream.

The common observer event is `benchmark-observer-event-v1`. New sidecars should
offer a cursor-resumable streaming endpoint in addition to snapshots:

```text
GET /v1/observe/snapshot
GET /v1/observe/events?after=<sequence>&wait_ms=<bounded wait>
GET /v1/observe/stream?after=<sequence>    # SSE target
```

The sidecar receives an opaque `segment_id` from the runtime and sends observer
events to the recorder with a monotonically increasing `source_sequence`. It
does not know the model, native session, Harbor job name, parent segment,
execution budget, or cumulative elapsed time. It must never decide whether an
infrastructure attempt counts.

### 2. Runtime control plane and run recorder

The Harbor adapter or a thin coordinator creates a stable `chain_id` once and
a new `segment_id` for each trial. One recorder process is the only writer for
that chain. Runtime lifecycle changes enter it directly; sidecar observer
events enter through a local ingestion endpoint. It writes both as
`benchmark-run-event-v1` records:

```json
{
  "schema": "benchmark-run-event-v1",
  "sequence": 8,
  "recorded_at_ms": 1780000000000,
  "effective_elapsed_ms": 7200123,
  "chain_id": "run_...",
  "segment_id": "seg_...",
  "source": "game",
  "source_sequence": 42,
  "type": "score_changed",
  "payload": {"score": 26, "objective": "b8 / Multi Infinite Enter"}
}
```

Runtime records and game records share this envelope. The minimal runtime
lifecycle is:

- `chain_created`;
- `segment_registered` with `parent_segment_id`, task, model, trial, observer
  source, and native session identity;
- `agent_execution_started` and `agent_execution_finished`;
- `checkpoint_created`;
- `segment_finished`, including a structured disposition such as `eligible`,
  `infrastructure_discarded`, `refused`, or `completed`.

The recorder is the sole authority for lineage and effective time. It stamps
`effective_elapsed_ms` before the record is durably appended, so score and time
cannot later drift apart. Docker labels, container IDs, Harbor paths, and job
names are source locators recorded by the runtime, never identities
reconstructed by the gateway.

The journal is append-only and uses one total `sequence`. Sidecar ingestion is
idempotent on `(segment_id, source_sequence)`: a reconnect may resend records,
but it cannot duplicate them. The sidecar retains its private audit and local
observer log until the recorder acknowledges the source cursor. Pausing waits
for that cursor to catch up before sealing a checkpoint. This permits recovery
from a brief recorder outage without creating two public authorities.

The first implementation should be an append-only JSONL journal under ignored
runtime state, not a database. The current scale does not justify another
persistence system. A SQLite projection can be added later only if replaying
the journal becomes measurably expensive.

### 3. Live projection plane

The gateway consumes only the authoritative run journal. It maintains one
in-memory projection per `chain_id` and exposes:

```text
GET /v1/runs
GET /v1/runs/<chain_id>
GET /v1/subscribe?run_id=<chain_id>
```

The browser subscribes only to this projection. The gateway is read-only, has
no game mutation credentials, and does not combine runtime results with game
logs. Game-specific normalization metadata and UI renderers remain under
`games/<game>/observer`; the gateway does not contain a branch per model or per
continuation generation.

During migration, the recorder's sidecar input may still describe a Docker
container and append-only file. That descriptor must be written by the runtime,
not discovered by scanning container names. The target input is the common
sidecar stream; the compatibility file reader can then be removed without
changing the journal, browser, or game renderer contracts.

## Effective-time projection

When a game event with source timestamp `t` is ingested, the recorder computes:

```text
effective(t) = sum over eligible execution windows [start, end]
               of max(0, min(t, end) - start)
```

For the active window, `end` is `t`. The recorder appends the resulting value in
the same immutable event as the game payload. Consumers never recompute it. A
score inherited by a new segment retains the effective offset accumulated
before that segment; it must not appear at `0h`. A checkpoint may cache this
offset for fast startup, but the journal remains the auditable source.

Detailed score history is a projection of `score_changed` records already in
the journal. A continuation does not copy historical observer events into the
new sidecar. If old detail is archived, the journal retains the latest score
record and its effective timestamp so the continuation cannot invent a
vertical line at the origin.

## Checkpoint manifest

Every new pause or continuation must consume a versioned manifest rather than a
loose collection of resume paths:

```json
{
  "schema": "benchmark-checkpoint-v1",
  "chain_id": "run_...",
  "segment_id": "seg_...",
  "parent_segment_id": "seg_...",
  "created_at": "2026-07-21T12:49:37Z",
  "effective_elapsed_ms": 7200000,
  "native_session": {"kind": "codex", "id": "..."},
  "run_journal": {"sequence": 812, "sha256": "..."},
  "artifacts": {
    "game_state": {"path": "game/state", "sha256": "..."},
    "game_events": {"path": "game/events.jsonl", "sha256": "..."},
    "game_audit": {"path": "game/audit", "sha256": "..."},
    "workspace": {"path": "workspace", "sha256": "..."},
    "agent_session": {"path": "sessions", "sha256": "..."}
  }
}
```

Creation is atomic: drain and acknowledge sidecar events, append
`checkpoint_created`, seal the journal sequence and artifacts, then write the
manifest last. Resume rejects a missing or hash-mismatched manifest and resumes
the same journal after the sealed sequence. Secrets are never copied into the
manifest.

## Migration

1. **Compatibility repair:** read direct Harbor artifact parents, legacy
   recovery job directories, and checkpoint manifests so current runs display
   correct accumulated time. Keep this code isolated and covered by regression
   tests.
2. **Run recorder:** generate `chain_id`/`segment_id`, create the single-writer
   journal, stamp lifecycle and sidecar records with effective time, and write
   versioned checkpoint manifests for every new run. Make the gateway prefer
   the journal while retaining the legacy reader for old runs.
3. **Explicit ingestion:** have the runtime register the active sidecar source
   with the recorder. Remove Docker-name scanning from the normal path.
4. **Common streaming transport:** add the SSE observer endpoint to the shared
   relay and direct sidecars; replace recorder `docker exec tail -F` with one
   registered sidecar subscription per active segment.
5. **Retire inference:** after existing runs are migrated or archived, remove
   recovery-path, checkpoint-path, and container-name lineage inference.

This sequence fixes correctness before changing transport. It keeps the only
cross-source composition inside the single writer at production time and
avoids a second runtime implementation inside either the game sidecar or the
live gateway.
