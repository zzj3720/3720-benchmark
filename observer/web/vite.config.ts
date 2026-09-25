import vinext from "vinext";
import { defineConfig } from "vite";
import hostingConfig from "./.openai/hosting.json";
import { sites } from "./build/sites-vite-plugin";

const SITE_CREATOR_PLACEHOLDER_DATABASE_ID =
  "00000000-0000-4000-8000-000000000000";

const { d1, r2 } = hostingConfig;

// macOS Seatbelt blocks FSEvents, so Codex previews need polling for HMR.
const isCodexSeatbeltSandbox = process.env.CODEX_SANDBOX === "seatbelt";

const localBindingConfig = {
  name: "benchmark-live",
  main: "./worker/index.ts",
  compatibility_flags: ["nodejs_compat"],
  // The live console: run summaries and subscriptions in one Durable Object,
  // published bodies and the raw journal backup in R2.
  durable_objects: { bindings: [{ name: "LIVE_HUB", class_name: "LiveHub" }] },
  migrations: [{ tag: "live-hub-v1", new_sqlite_classes: ["LiveHub"] }],
  ...(process.env.LIVE_CUSTOM_DOMAIN
    ? { routes: [{ pattern: process.env.LIVE_CUSTOM_DOMAIN, custom_domain: true }] }
    : {}),
  d1_databases: d1
    ? [
        {
          binding: d1,
          database_name: "site-creator-d1",
          database_id: SITE_CREATOR_PLACEHOLDER_DATABASE_ID,
        },
      ]
    : [],
  r2_buckets: [
    { binding: "LIVE_BUCKET", bucket_name: "benchmark-live" },
    ...(r2 ? [{ binding: r2, bucket_name: "site-creator-r2" }] : []),
  ],
};

export default defineConfig(async () => {
  // Keep Wrangler and Miniflare state project-local. These are non-secret tool
  // settings; application environment belongs in ignored `.env*` files.
  process.env.WRANGLER_WRITE_LOGS ??= "false";
  process.env.WRANGLER_LOG_PATH ??= ".wrangler/logs";
  process.env.MINIFLARE_REGISTRY_PATH ??= ".wrangler/registry";

  // Wrangler snapshots its log path while the Cloudflare plugin is imported.
  const { cloudflare } = await import("@cloudflare/vite-plugin");

  return {
    resolve: { dedupe: ["react", "react-dom", "three"] },
    server: isCodexSeatbeltSandbox
      ? { watch: { useFsEvents: false, usePolling: true } }
      : undefined,
    plugins: [
      vinext(),
      sites(),
      cloudflare({
        viteEnvironment: { name: "rsc", childEnvironments: ["ssr"] },
        config: localBindingConfig,
      }),
    ],
  };
});
