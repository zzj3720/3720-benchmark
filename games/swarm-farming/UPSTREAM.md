# Upstream Swarm

The rules engine and Farming scenario are derived from
[swarm-game/swarm](https://github.com/swarm-game/swarm):

- release: `0.8.0.0`
- commit: `da708abbe5a3d3b339f27e2090d92fe9f4f97ea3`
- upstream license: BSD-3-Clause
- upstream copyright: Brent Yorgey and other contributors, 2021-2025

The benchmark adapter uses the original engine libraries directly. The frozen
scenario removes the embedded reference-solution field and adds the repository
canary comment; game semantics are otherwise unchanged.
