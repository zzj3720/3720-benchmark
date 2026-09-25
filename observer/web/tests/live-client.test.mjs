import assert from "node:assert/strict";
import test from "node:test";
import { LiveResource, RequestGate, applySubscription, effectiveDuration } from "../app/live-client.ts";

const turn = () => new Promise(resolve => setImmediate(resolve));
const deferred = () => { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; };

test("detail revisions coalesce into one request and cannot complete out of order", async () => {
  const pending = [], values = [];
  const resource = new LiveResource({load: () => { const request = deferred(); pending.push(request); return request.promise; }, commit: value => values.push(value), status() {}});
  try {
    resource.request("1"); resource.request("2"); resource.request("3");
    assert.equal(pending.length, 1);
    pending[0].resolve(1); await turn();
    assert.equal(pending.length, 2);
    pending[1].resolve(3); await turn();
    assert.deepEqual(values, [1, 3]);
    resource.request("3"); assert.equal(pending.length, 2);
    resource.request("3:orphaned"); assert.equal(pending.length, 3);
  } finally { resource.dispose(); }
});

test("detail errors retry without a new game event", async () => {
  let calls = 0; const ready = deferred();
  const resource = new LiveResource({load: async () => { if (++calls === 1) throw new Error("offline"); return 42; }, commit: value => ready.resolve(value), status() {}, retryDelayMs: 1});
  try { resource.request("1"); assert.equal(await ready.promise, 42); assert.equal(calls, 2); } finally { resource.dispose(); }
});

test("timeout and disposal work even if transport ignores abort", async () => {
  const errors = deferred(), values = []; const pending = deferred();
  const resource = new LiveResource({load: () => pending.promise, commit: value => values.push(value), status: (state) => { if (state === "error") errors.resolve(); }, timeoutMs: 5});
  resource.request("1"); await errors.promise; resource.dispose(); pending.resolve("old"); await turn();
  assert.deepEqual(values, []);
});

test("returning live invalidates a pending replay and a newer selection invalidates the prior one", () => {
  const gate = new RequestGate(); const old = gate.begin(); const next = gate.begin();
  assert.equal(old.current(), false); assert.equal(next.current(), true);
  gate.cancel(); assert.equal(next.current(), false); assert.equal(next.signal.aborted, true);
});

test("v2 deltas preserve histories and errors never replace the last good state", () => {
  const run = {id: "one", latest_sequence: 1, score_history: [{score: 1}]};
  const initial = applySubscription([], {schema: "benchmark-live-subscription-v2", generated_at: 1, reset: true, removed: [], runs: [run]}).runs;
  const next = applySubscription(initial, {schema: "benchmark-live-subscription-v2", generated_at: 2, reset: false, removed: [], runs: [{id: "one", latest_sequence: 2, score_history_delta: [{score: 2}]}]}).runs;
  assert.deepEqual(next[0].score_history, [{score: 1}, {score: 2}]);
  assert.throws(() => applySubscription(next, {error: "projection failed"}));
  assert.deepEqual(initial[0].score_history, [{score: 1}]);
  const removed = applySubscription(next, {schema: "benchmark-live-subscription-v2", generated_at: 3, reset: false, removed: ["one"], runs: []}).runs;
  assert.deepEqual(removed, []);
});

test("execution time advances only in an eligible active window", () => {
  const run = {live: true, consumed_ms: 500, execution: {active: true, anchor_timestamp_ms: 1000, anchor_elapsed_ms: 500}};
  assert.equal(effectiveDuration(run, 2000), 1500);
  assert.equal(effectiveDuration({...run, live: false}, 2000), 500);
  assert.equal(effectiveDuration({...run, execution: {...run.execution, active: false}}, 2000), 500);
});
