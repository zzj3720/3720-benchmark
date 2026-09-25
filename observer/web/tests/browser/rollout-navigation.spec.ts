import { expect, test, type Page } from "@playwright/test";

const summaries = ["A", "B"].map(name => ({
  id: `rollout-${name.toLowerCase()}`, game: "sokoban", model: `Rollout ${name}`,
  task: "Sokoban", agent: "fixture", effort: "default", objective: `Level ${name}`,
  live: false, score: 1, total: 4, consumed_ms: 1000, latest_sequence: 1,
  detail_revision: "fixed", termination: { kind: "stopped" }, score_history: [],
}));

async function mockRuns(page: Page, holdDetail: Promise<void> = Promise.resolve()) {
  await page.route("**/api/live/v1/**", async route => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/subscribe")) {
      await route.fulfill({ contentType: "text/event-stream", body: `data: ${JSON.stringify({ schema: "benchmark-live-subscription-v2", revision: 1, reset: true, removed: [], runs: summaries, generated_at: Date.now() })}\n\n` });
      return;
    }
    const summary = summaries.find(run => path.endsWith(`/${run.id}`));
    if (summary) {
      await holdDetail;
      await route.fulfill({ json: { run: { ...summary, state: {}, replay_groups: [], live_replay: null, recent_activity: [] } } });
      return;
    }
    await route.fulfill({ json: { runs: summaries } });
  });
}

const rollout = (page: Page, name: string) => page.locator(".model-run").filter({ hasText: `Rollout ${name}` });

test("reselecting a loaded rollout preserves its detail, expanded panels and browser history", async ({ page }) => {
  await mockRuns(page);
  await page.goto("/?run=rollout-a");
  await expect(page.locator(".environment-card")).toBeVisible();
  await page.getByRole("button", { name: "运行分析与活动", exact: true }).click();
  const entries = await page.evaluate(() => history.length);
  await rollout(page, "A").dblclick();
  await expect(page.locator(".environment-card")).toBeVisible();
  await expect(page.locator(".detail-chart")).toBeVisible();
  expect(await page.evaluate(() => history.length)).toBe(entries);
});

test("double-clicking while the initial detail is pending still completes one navigation", async ({ page }) => {
  let release!: () => void;
  const pending = new Promise<void>(resolve => { release = resolve; });
  await mockRuns(page, pending);
  await page.goto("/?game=sokoban");
  await expect(rollout(page, "A")).toBeVisible();
  const entries = await page.evaluate(() => history.length);
  try {
    await rollout(page, "A").dblclick();
    await expect(page.getByText("正在加载运行", { exact: true })).toBeVisible();
  } finally { release(); }
  await expect(page.locator(".environment-card")).toBeVisible();
  expect(await page.evaluate(() => history.length)).toBe(entries + 1);
});

test("back navigation between URLs for the same rollout preserves detail and game selection", async ({ page }) => {
  await mockRuns(page);
  await page.goto("/?run=rollout-a");
  await expect(page.locator(".environment-card")).toBeVisible();
  await page.evaluate(() => history.pushState({}, "", "?run=rollout-a&v=duplicate"));
  await page.goBack();
  await expect(page.locator(".environment-card")).toBeVisible();
  await expect(page.locator(".game-switcher button").filter({ hasText: "SOKOBAN" })).toHaveAttribute("aria-pressed", "true");
});

test("switching between different rollouts still loads the selected detail", async ({ page }) => {
  await mockRuns(page);
  await page.goto("/?run=rollout-a");
  await expect(page.locator(".environment-title")).toHaveText("Level A");
  await rollout(page, "B").click();
  await expect(page.locator(".environment-title")).toHaveText("Level B");
  await rollout(page, "A").click();
  await expect(page.locator(".environment-title")).toHaveText("Level A");
});
