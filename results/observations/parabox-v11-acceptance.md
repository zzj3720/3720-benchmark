# Parabox v11 acceptance

Date: 2026-07-20

## Scope

- Task: `3720/parabox-intro`
- Dataset package digest:
  `sha256:12579834a970f268d7652d7a7cbe9f0d5d7ffafb00126590490358007f383f8c`
- Pre-observer Harbor trial task checksum:
  `4cb1057976ed27e64a93ead17bc9b292476a59826a1e196e40c23672a6c9a0ea`
- Campaign: all 364 puzzles from inspected game build `8556490`
- Reward: one integer point per independently verified solved puzzle

The Agent instruction makes obtaining the highest possible score the primary
objective. It tells the Agent to continue while unlocked puzzles and meaningful
reasoning paths remain and not to stop at an adequate partial score.

## Deterministic gates

- All 364 frozen level files parse.
- All 364 imported reference histories solve in the Rust rules engine.
- Every puzzle is reachable through the nonlinear area and puzzle unlock graph.
- Rust: 11 library tests and 6 CLI/API integration tests passed.
- Result tooling: 21 tests passed.
- Strict Rust Clippy passed with warnings treated as errors.
- Every repository task static check passed.
- The packaged Agent environment contains no Rust source, verifier tests, or
  Oracle trace. It contains only the thin API client; the game sidecar and
  separate verifier each receive the required frozen runtime data.
- Repository scanning found no secret value and no tracked environment file.

## Harbor acceptance

Both clean acceptance jobs used the same final task checksum:

| Job | Agent | Reward | Trial exceptions |
|---|---|---:|---:|
| `parabox-v11-final-oracle2` | Oracle | 364 | 0 |
| `parabox-v11-final-nop` | Nop | 0 | 0 |

The successful Oracle sidecar audit contains 1,191 API requests. Exactly one
request failed: the first attempt to select `e7` while it was still locked. The
Oracle deferred it, followed other available branches, returned after its
predecessor unlocked it, and solved `e7` last. The collected state contains 364
`solved` records. The separate verifier checked all frozen hashes and replayed
the per-puzzle action histories, reporting `verified: solved 364/364 levels`.

An earlier same-checksum Oracle trial, `parabox-v11-final-oracle`, completed the
game and submitted 364 but Harbor could not start its verifier because a Docker
Hub authentication request hit a TLS handshake timeout. That trial remains an
infrastructure exception and is not counted as a pass. Its collected state was
successfully replayed with the same verifier image after the image became
available; the subsequent clean Harbor retry above supplies the authoritative
acceptance result.

## Ranking

The benchmark reward and displayed score are the integer solved-puzzle count.
For model result ordering, total token consumption only breaks ties between
equal integer scores:

```text
score = solved_puzzles
rank by score descending, then total_tokens ascending
```

Wall-clock time is retained only as a diagnostic because provider and service
stability can dominate it.

## 2026-07-21 observer package update

The sidecar now writes a complete post-action state into every native event and
starts a read-only observer relay on internal port 3721. Agent API responses,
cooldown behavior, scoring, campaign data, and verifier inputs are unchanged.
All 11 Rust unit tests and 6 CLI/API tests passed after the change. A live
cursor poll was released by a real `parabox move up` command and returned the
complete `a1` state. The final repackaged task digest is
`sha256:12579834a970f268d7652d7a7cbe9f0d5d7ffafb00126590490358007f383f8c`.
A clean Harbor Nop job, `2026-07-21__05-26-09`, exercised that exact package's
sidecar and separate verifier with reward zero and no exception.
