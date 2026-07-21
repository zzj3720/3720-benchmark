type RouteContext = {
  params: Promise<{ path: string[] }>;
};

const RUNS_PATH = /^v1\/runs(?:\/[^/]+)?$/;

export const dynamic = "force-dynamic";

export async function GET(request: Request, context: RouteContext) {
  const { path } = await context.params;
  const livePath = path.join("/");
  if (!RUNS_PATH.test(livePath)) {
    return Response.json({ error: "live endpoint is read-only" }, { status: 404 });
  }

  const origin = process.env.LIVE_GATEWAY_ORIGIN ?? "http://127.0.0.1:3740";
  const upstream = new URL(`/${livePath}`, origin);
  upstream.search = new URL(request.url).search;

  try {
    const response = await fetch(upstream, {
      cache: "no-store",
      headers: { accept: "application/json" },
    });
    const body = await response.arrayBuffer();
    return new Response(body, {
      status: response.status,
      headers: {
        "cache-control": "no-store",
        "content-type": response.headers.get("content-type") ?? "application/json",
      },
    });
  } catch {
    return Response.json(
      { error: "live gateway unavailable" },
      { status: 502, headers: { "cache-control": "no-store" } },
    );
  }
}
