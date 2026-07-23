const ORIGIN = "benchmark-live-origin.3720.org";
const ALLOWED_METHODS = new Set(["GET", "HEAD"]);
const CACHE_VERSION = "2026-07-22-v5";

function upstreamUrl(request) {
  const upstream = new URL(request.url);
  upstream.hostname = ORIGIN;
  if (upstream.pathname === "/api/live/subscribe") {
    upstream.pathname = "/v1/subscribe";
  } else if (upstream.pathname.startsWith("/api/live/v1/")) {
    upstream.pathname = upstream.pathname.slice("/api/live".length);
  }
  return upstream;
}

function cachePolicy(request) {
  if (request.method !== "GET") return null;
  const url = new URL(request.url);
  if (url.pathname.startsWith("/api/live/v1/assets/")) {
    return "public, max-age=31536000, immutable";
  }
  if (url.pathname.startsWith("/assets/")) return "public, max-age=31536000, immutable";
  if (url.pathname === "/" && (request.headers.get("accept") ?? "").includes("text/html")) {
    return "public, max-age=15, stale-while-revalidate=300";
  }
  return null;
}

function cacheKey(request) {
  const url = new URL(request.url);
  url.searchParams.set("__edge", CACHE_VERSION);
  return new Request(url, request);
}

function clientPolicy(request, policy) {
  return new URL(request.url).pathname === "/"
    ? "public, max-age=0, must-revalidate"
    : policy;
}

function cachedResponse(response, request, policy) {
  const headers = new Headers(response.headers);
  headers.set("cache-control", clientPolicy(request, policy));
  return new Response(response.body, {
    status: response.status,
    statusText: response.statusText,
    headers,
  });
}

const worker = {
  async fetch(request, _env, context) {
    if (!ALLOWED_METHODS.has(request.method)) {
      return Response.json(
        { error: "live observer is read-only" },
        { status: 405, headers: { allow: "GET, HEAD" } },
      );
    }

    const upstream = upstreamUrl(request);
    const isLiveApi = upstream.pathname.startsWith("/v1/");
    const policy = cachePolicy(request);
    const cache = globalThis.caches?.default;
    const key = policy ? cacheKey(request) : request;
    if (policy && cache) {
      const cached = await cache.match(key);
      if (cached) return cachedResponse(cached, request, policy);
    }

    const headers = new Headers(request.headers);
    headers.set("x-forwarded-host", "live.benchmark.3720.org");
    headers.set("x-forwarded-proto", "https");

    const response = await fetch(upstream, {
      method: request.method,
      headers,
      redirect: "manual",
    });
    const responseHeaders = new Headers(response.headers);

    if (isLiveApi && !policy) {
      responseHeaders.set("cache-control", "no-store");
      return new Response(request.method === "HEAD" ? null : response.body, {
        status: response.status,
        statusText: response.statusText,
        headers: responseHeaders,
      });
    }

    if (policy && response.ok) {
      responseHeaders.set("cache-control", clientPolicy(request, policy));
    }
    const proxied = new Response(request.method === "HEAD" ? null : response.body, {
      status: response.status,
      statusText: response.statusText,
      headers: responseHeaders,
    });
    if (policy && response.ok && cache && context) {
      const cacheHeaders = new Headers(proxied.headers);
      cacheHeaders.set("cache-control", policy);
      context.waitUntil(cache.put(key, new Response(proxied.clone().body, {
        status: proxied.status,
        statusText: proxied.statusText,
        headers: cacheHeaders,
      })));
    }
    return proxied;
  },
};

export default worker;
