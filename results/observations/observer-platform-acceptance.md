# Benchmark observer acceptance

Date: 2026-07-21

## Common protocol

All three current game tasks produced:

- `benchmark-observer-snapshot-v1` from `/v1/observe/snapshot`;
- `benchmark-observer-batch-v1` from cursor-based
  `/v1/observe/events`;
- ordered `benchmark-observer-event-v1` records with complete post-action
  state;
- `Access-Control-Allow-Origin: *` for the read-only browser client.

Live loopback snapshots were fetched concurrently:

| Port | Task | Initial observed sequence | Complete state |
|---:|---|---:|---|
| 3731 | `parabox-intro` | 2 | yes |
| 3732 | `swarm-farming` | 3 | yes |
| 3733 | `sausage-roll` | 2 | yes |

The exact sequence numbers include the lifecycle and smoke actions performed
before the concurrent fetch.

## Long-poll E2E

- Parabox: a poll after sequence 1 blocked until the real Agent client executed
  `parabox move up`; it returned sequence 2, action metadata, and the complete
  `a1` state.
- Swarm: the initial snapshot reported tick 0 and one robot. A poll after
  sequence 2 blocked until the real Agent client executed `swarm status`; it
  returned sequence 3 with tick 0, one robot, and no virtual-time advance.
- Sausage: a poll after sequence 1 blocked until the real Agent client executed
  `sausage status`; it returned sequence 2 with the complete Land's End state
  and score zero.

Parabox and Swarm passed through the standalone Rust observer relay. Sausage
served the same protocol directly. The Swarm E2E also exposed a client
networking defect: its default URL only tried loopback from the Agent
container. The client now probes both loopback and the Compose `game` service;
the repaired default path produced the recorded event.

## Operations console

- Production build completed.
- Server-render tests passed 2/2.
- ESLint passed.
- Tests assert all three task adapters, cursor subscriptions, abortable
  reconnect loops, browser-local endpoint persistence, event retention bounds,
  metadata, and social image.
- The source uses no mutation endpoint; offline fixtures are visibly labeled
  demo data.
- The private Sites deployment completed at
  `https://benchmark-live-ops-3720.zuozijian1994.chatgpt.site`.
- Exact source commit `975c99b97c393ea2685b3630f5d84728e28e7cdd` was
  saved as Sites version 2 and deployed successfully.

The deployed site requires workspace sign-in. Its source settings default to
the three operator-overlay loopback ports and remain editable in each viewer's
browser.
