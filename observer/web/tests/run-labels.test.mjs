import assert from "node:assert/strict";
import test from "node:test";
import { harnessName, modelName, rankRuns, runLabels, seriesColors, viewerStatus } from "../app/run-labels.ts";

test("model ids become readable names", () => {
  assert.equal(modelName("openai/gpt-5.6-luna"), "GPT-5.6 Luna");
  assert.equal(modelName("openai/gpt-6-astra"), "GPT-6 Astra");
  assert.equal(modelName("deepseek/deepseek-v4.1-flash-expires-on-0910"), "DeepSeek V4.1 Flash");
  assert.equal(modelName("deepseek-v4-flash"), "DeepSeek V4 Flash");
  assert.equal(modelName("openrouter/stealth/union-alpha"), "Union Alpha");
  assert.equal(modelName("Qwen3.8-Max-Preview"), "Qwen3.8 Max Preview");
});

test("labels add a harness or date only when runs would collide", () => {
  const run = (id, model, effort, agent, started_at = 0) => ({ id, model, effort, agent, started_at, live: false, score: 0 });
  const labels = runLabels([
    run("a", "openai/gpt-5.6-luna", "max", "tools.agents.isolated_codex:GoalIsolatedCodex"),
    run("b", "openai/gpt-5.6-luna", "xhigh", "tools.agents.isolated_codex:GoalIsolatedCodex"),
    run("c", "deepseek-v4-flash", "xhigh", "tools.agents.deepseek_claude_code:GoalDeepSeekClaudeCode"),
    run("d", "deepseek/deepseek-v4-flash", "xhigh", "tools.agents.pi_goal:CampaignGoalPi"),
    run("e", "deepseek/deepseek-v4-flash", "default", "tools.agents.pi_goal:CampaignGoalPi", Date.UTC(2026, 6, 23)),
    run("f", "deepseek/deepseek-v4-flash", "default", "tools.agents.pi_goal:CampaignGoalPi", Date.UTC(2026, 6, 24)),
  ]);
  assert.equal(labels.get("a"), "GPT-5.6 Luna · max");
  assert.equal(labels.get("b"), "GPT-5.6 Luna · xhigh");
  assert.equal(labels.get("c"), "DeepSeek V4 Flash · xhigh · Claude Code");
  assert.equal(labels.get("d"), "DeepSeek V4 Flash · xhigh · Pi");
  assert.notEqual(labels.get("e"), labels.get("f"));
  assert.equal(harnessName("tools.agents.qoder_cli_cn:QoderCliCn"), "Qoder");
});

test("viewers see three states with the reason as detail", () => {
  assert.equal(viewerStatus({ live: true, termination: { kind: "live" } }).label, "直播中");
  assert.equal(viewerStatus({ live: false, termination: { kind: "completed" } }).label, "已完赛");
  const ended = viewerStatus({ live: false, termination: { kind: "resumable", reason: "infrastructure error" } });
  assert.equal(ended.label, "已结束");
  assert.equal(ended.detail, "因基础设施故障中止");
  assert.equal(viewerStatus({ live: false, termination: { kind: "agent_stopped" } }).detail, "模型主动结束");
});

test("ranking puts live runs first, then score, and colors follow rank", () => {
  const runs = [
    { id: "low", live: false, score: 5, consumed_ms: 1 },
    { id: "high", live: false, score: 50, consumed_ms: 9 },
    { id: "live", live: true, score: 1, consumed_ms: 1 },
    { id: "fast", live: false, score: 50, consumed_ms: 2 },
  ];
  assert.deepEqual(rankRuns(runs).map(run => run.id), ["live", "fast", "high", "low"]);
  const colors = seriesColors(rankRuns(runs));
  assert.equal(new Set(colors.values()).size, 4);
});
