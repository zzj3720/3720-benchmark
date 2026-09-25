# Benchmark observer relay

The relay gives sidecars with append-only JSONL state histories one common
read-only subscription API:

- `GET /v1/observe/snapshot`
- `GET /v1/observe/events?after=SEQUENCE&limit=COUNT&wait_ms=30000`
- `GET /health`

`after` is an exclusive cursor. The event endpoint optionally long-polls and
returns `benchmark-observer-event-v1` records without changing game state or
entering the model-facing audit. Each event has the same top-level fields:

```json
{
  "schema": "benchmark-observer-event-v1",
  "sequence": 1,
  "timestamp_ms": null,
  "task": {"id": "task-id", "label": "Task label", "kind": "game"},
  "type": "command",
  "action": {},
  "state": {},
  "result": {"ok": true}
}
```

The snapshot response uses `benchmark-observer-snapshot-v1`; the event response
uses `benchmark-observer-batch-v1`. `latest_sequence` is the cursor to retain
after processing a response. A caller may reconnect with that cursor without
replaying earlier events.

Configuration is supplied through `OBSERVER_SOURCE`, `OBSERVER_FORMAT`
(`generic`, `parabox`, or `swarm`), `OBSERVER_TASK_ID`,
`OBSERVER_TASK_LABEL`, and `OBSERVER_LISTEN_ADDR`.

Parabox writes a full state into each native event. Swarm writes its initial
state into the audit header and a complete response state for every command.
Sausage emits this common schema directly from its sidecar and does not need
the relay.

Run the normalization tests with:

```bash
cargo test --manifest-path observer/relay/Cargo.toml
```
