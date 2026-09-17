type RouteContext = {
  params: Promise<{ path: string[] }>;
};

const READ_ONLY_PATH =
  /^v1\/(?:runs(?:\/[^/]+)?|assets\/[^/]+|subscribe)$/;

export const dynamic = "force-dynamic";

export async function GET(request: Request, context: RouteContext) {
  const { path } = await context.params;
  const livePath = path.join("/");
  if (!READ_ONLY_PATH.test(livePath)) {
    return Response.json({ error: "live endpoint is read-only" }, { status: 404 });
  }

  const origin = process.env.LIVE_GATEWAY_ORIGIN ?? "http://127.0.0.1:3740";
  const upstream = new URL(`/${livePath}`, origin);
  upstream.search = new URL(request.url).search;

  try {
    const response = await fetch(upstream, {
      cache: "no-store",
      signal: request.signal,
      headers: { accept: request.headers.get("accept") ?? "application/json" },
    });
    // Stream the body straight through: /v1/subscribe is a never-ending SSE
    // stream, so reading it to completion would hang forever.
    return new Response(response.body, {
      status: response.status,
      headers: {
        "cache-control": response.headers.get("cache-control") ?? "no-store",
        "content-type": response.headers.get("content-type") ?? "application/json",
        "x-accel-buffering": "no",
      },
    });
  } catch {
    return Response.json(
      { error: "live gateway unavailable" },
      { status: 502, headers: { "cache-control": "no-store" } },
    );
  }
}
