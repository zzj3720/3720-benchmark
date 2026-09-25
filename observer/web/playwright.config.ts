import { defineConfig } from "@playwright/test";

const port = process.env.LIVE_UI_TEST_PORT ?? "13002";
const baseURL = `http://127.0.0.1:${port}`;
export default defineConfig({
  testDir: "./tests/browser",
  workers: 1,
  use: { baseURL, browserName: "chromium", channel: process.env.PLAYWRIGHT_CHANNEL, viewport: { width: 1280, height: 900 } },
  webServer: {
    command: "node dist/standalone/server.js",
    env: { PORT: port, HOST: "127.0.0.1" },
    url: baseURL,
    reuseExistingServer: false,
    gracefulShutdown: { signal: "SIGTERM", timeout: 1000 },
  },
});
