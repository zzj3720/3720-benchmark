import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import http from "node:http";
import { once } from "node:events";
import test from "node:test";

const root = new URL("../", import.meta.url);
const vinextCli = new URL("../node_modules/vinext/dist/cli.js", import.meta.url);

const delay = (milliseconds) =>
  new Promise((resolve) => setTimeout(resolve, milliseconds));

async function availablePort() {
  const server = http.createServer();
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const address = server.address();
  assert(address && typeof address === "object");
  await new Promise((resolve) => server.close(resolve));
  return address.port;
}

function request(url) {
  return new Promise((resolve, reject) => {
    http
      .get(url, (response) => {
        const chunks = [];
        response.on("data", (chunk) => chunks.push(chunk));
        response.on("end", () =>
          resolve({
            status: response.statusCode,
            body: Buffer.concat(chunks).toString("utf8"),
          }),
        );
      })
      .on("error", reject);
  });
}

function abortRequest(url) {
  const outgoing = http.get(url, (response) => response.resume());
  outgoing.on("error", () => {});
  setTimeout(() => outgoing.destroy(), 5);
}

test("production server survives clients leaving during live refreshes", { timeout: 15_000 }, async (t) => {
  const gateway = http.createServer((_request, response) => {
    response.writeHead(200, { "content-type": "application/json" });
    setTimeout(() => response.end('{"runs":[]}'), 80);
  });
  gateway.listen(0, "127.0.0.1");
  await once(gateway, "listening");
  t.after(() => gateway.close());

  const gatewayAddress = gateway.address();
  assert(gatewayAddress && typeof gatewayAddress === "object");
  const port = await availablePort();
  const output = [];
  const child = spawn(process.execPath, [vinextCli.pathname, "start"], {
    cwd: root,
    env: {
      ...process.env,
      LIVE_GATEWAY_ORIGIN: `http://127.0.0.1:${gatewayAddress.port}`,
      PORT: String(port),
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  child.stdout.on("data", (chunk) => output.push(chunk.toString()));
  child.stderr.on("data", (chunk) => output.push(chunk.toString()));
  t.after(async () => {
    if (child.exitCode === null) child.kill("SIGTERM");
    if (child.exitCode === null) await once(child, "exit");
  });

  const baseUrl = `http://127.0.0.1:${port}`;
  for (let attempt = 0; attempt < 50; attempt += 1) {
    try {
      if ((await request(baseUrl)).status === 200) break;
    } catch {
      await delay(100);
      continue;
    }
    await delay(100);
  }

  for (let index = 0; index < 12; index += 1) {
    abortRequest(`${baseUrl}/api/live/v1/runs`);
  }
  await delay(300);

  assert.equal(child.exitCode, null, output.join(""));
  const response = await request(`${baseUrl}/api/live/v1/runs`);
  assert.equal(response.status, 200, output.join(""));
  assert.deepEqual(JSON.parse(response.body), { runs: [] });
});
