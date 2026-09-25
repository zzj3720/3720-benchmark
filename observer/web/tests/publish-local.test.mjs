import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import test from "node:test";

test("Docker publication stages before switching and rolls back on failure", () => {
  const result = spawnSync("python3", [new URL("publish_docker_test.py", import.meta.url).pathname], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stdout + result.stderr);
});
