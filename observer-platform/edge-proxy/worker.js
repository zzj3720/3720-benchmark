const ORIGIN = "benchmark-live-origin.3720.org";
const ALLOWED_METHODS = new Set(["GET", "HEAD"]);

const worker = {
  async fetch(request) {
    if (!ALLOWED_METHODS.has(request.method)) {
      return Response.json(
        { error: "live observer is read-only" },
        { status: 405, headers: { allow: "GET, HEAD" } },
      );
    }

    const upstream = new URL(request.url);
    upstream.hostname = ORIGIN;
    const subscription = upstream.pathname === "/api/live/subscribe";
    if (subscription) upstream.pathname = "/v1/subscribe";

    const headers = new Headers(request.headers);
    headers.set("x-forwarded-host", "live.benchmark.3720.org");
    headers.set("x-forwarded-proto", "https");

    headers.delete("accept-encoding");
    const response = await fetch(upstream, {
      method: request.method,
      headers,
      redirect: "manual",
    });
    if (subscription && request.method !== "HEAD") {
      const responseHeaders = new Headers(response.headers);
      responseHeaders.delete("content-length");
      responseHeaders.delete("content-encoding");
      responseHeaders.set("cache-control", "no-store, no-transform");
      return new Response(response.body, {
        status: response.status,
        statusText: response.statusText,
        headers: responseHeaders,
      });
    }
    const body = request.method === "HEAD" ? null : await response.arrayBuffer();
    const responseHeaders = new Headers(response.headers);
    responseHeaders.delete("content-encoding");
    if (body) responseHeaders.set("content-length", String(body.byteLength));
    return new Response(body, {
      status: response.status,
      statusText: response.statusText,
      headers: responseHeaders,
    });
  },
};

export default worker;
