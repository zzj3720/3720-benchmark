import assert from "node:assert/strict";
import { chmod, mkdir, mkdtemp, readFile, readlink, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";

const project = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

test("local publish packages and installs the Rust live gateway", async (context) => {
  if (process.platform !== "darwin") {
    context.skip("LaunchAgent deployment is macOS-specific");
    return;
  }

  const temp = await mkdtemp(path.join(os.tmpdir(), "benchmark-live-publish-"));
  const home = path.join(temp, "home");
  const bin = path.join(temp, "bin");
  const launchctlLog = path.join(temp, "launchctl.log");
  const releaseRoot = path.join(temp, "releases");
  await writeFile(path.join(temp, ".keep"), "");
  await Promise.all([
    mkdir(path.join(home, "Library", "LaunchAgents"), { recursive: true }),
    mkdir(bin, { recursive: true }),
  ]);
  const launchctl = path.join(bin, "launchctl");
  await writeFile(
    launchctl,
    `#!/bin/sh\nprintf '%s\\n' "$*" >> "$LAUNCHCTL_LOG"\nexit 0\n`,
  );
  await chmod(launchctl, 0o755);

  try {
    const result = spawnSync("sh", ["scripts/publish-local.sh"], {
      cwd: project,
      encoding: "utf8",
      env: {
        ...process.env,
        HOME: home,
        LAUNCHCTL_LOG: launchctlLog,
        LIVE_RELEASE_ROOT: releaseRoot,
        PATH: `${bin}:${process.env.PATH}`,
      },
      timeout: 120_000,
    });
    assert.equal(result.status, 0, result.stderr || result.stdout);

    assert.match(await readlink(path.join(releaseRoot, "current")), /^releases\//);
    const plist = await readFile(
      path.join(home, "Library", "LaunchAgents", "org.3720.benchmark-live-gateway.plist"),
      "utf8",
    );
    assert.match(plist, /current\/bin\/live-gateway/);
    assert.doesNotMatch(plist, /python|server\.py/);

    const launchCalls = await readFile(launchctlLog, "utf8");
    assert.match(launchCalls, /bootout gui\/\d+\/org\.3720\.benchmark-live-gateway/);
    assert.match(launchCalls, /bootstrap gui\/\d+ .*benchmark-live-gateway\.plist/);
  } finally {
    await rm(temp, { recursive: true, force: true });
  }
});
