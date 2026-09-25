import assert from "node:assert/strict";
import test from "node:test";
import { FEED_TIMEOUT_MS, HubState } from "../worker/live-hub-state.ts";
import { applySubscription } from "../app/live-client.ts";

const run = (id, history, extra = {}) => ({ id, latest_sequence: history.length, detail_revision: `${history.length}`, score_history: history, ...extra });

test("publisher batches become protocol-v2 deltas the browser can apply", () => {
  const hub = new HubState();
  hub.heartbeat("studio", 1_000);
  hub.applyRuns({ order: ["a", "b"], runs: [run("a", [{ score: 1 }]), run("b", [])], removed: [] }, 1_000);
  const first = hub.snapshot("a", 1_000);
  assert.equal(first.reset, true);
  assert.deepEqual(first.selected, { id: "a", revision: "1" });

  const update = hub.applyRuns({ order: ["a", "b"], runs: [run("a", [{ score: 1 }, { score: 2 }])], removed: [] }, 2_000);
  const message = hub.message(update, "a", 2_000);
  assert.equal(message.runs[0].score_history, undefined);
  assert.deepEqual(message.runs[0].score_history_delta, [{ score: 2 }]);
  assert.ok(message.revision > first.revision);

  const initial = applySubscription([], first).runs;
  const applied = applySubscription(initial, message).runs;
  assert.deepEqual(applied.find(value => value.id === "a").score_history, [{ score: 1 }, { score: 2 }]);
});

test("a rewritten history is sent whole, and removals propagate", () => {
  const hub = new HubState();
  hub.applyRuns({ order: ["a", "b"], runs: [run("a", [{ score: 3 }]), run("b", [])], removed: [] }, 0);
  const update = hub.applyRuns({ order: ["a"], runs: [run("a", [{ score: 1 }])], removed: ["b"] }, 1);
  const message = hub.message(update, null, 1);
  assert.deepEqual(message.runs[0].score_history, [{ score: 1 }]);
  assert.deepEqual(message.removed, ["b"]);
  assert.deepEqual(hub.runsResponse(1).runs.map(value => value.id), ["a"]);
});

test("the feed goes offline without heartbeats and keeps the last runs", () => {
  const hub = new HubState();
  assert.equal(hub.heartbeat("studio", 0), true);
  hub.applyRuns({ order: ["a"], runs: [run("a", [{ score: 5 }], { live: true })], removed: [] }, 10);
  assert.equal(hub.expire(10 + FEED_TIMEOUT_MS), false);
  const revision = hub.revision;
  assert.equal(hub.expire(11 + FEED_TIMEOUT_MS), true);
  assert.equal(hub.feed.connected, false);
  assert.equal(hub.feed.last_seen_ms, 10);
  assert.ok(hub.revision > revision);
  assert.equal(hub.runsResponse(0).runs[0].score_history[0].score, 5);
  assert.equal(hub.heartbeat("studio", 99_999), true);
  assert.equal(hub.feed.connected, true);
});

test("runs missing from the publisher's order are removed", () => {
  const hub = new HubState();
  hub.applyRuns({ order: ["a", "b"], runs: [run("a", []), run("b", [])], removed: [] }, 0);
  const update = hub.applyRuns({ order: ["a"], runs: [], removed: [] }, 1);
  assert.deepEqual(update.removed, ["b"]);
  assert.deepEqual(hub.runsResponse(1).runs.map(value => value.id), ["a"]);
});
