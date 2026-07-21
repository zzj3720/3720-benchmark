const OBSERVER_ORIGINS = {
  parabox: () => process.env.PARABOX_OBSERVER_ORIGIN ?? "http://127.0.0.1:3731",
  swarm: () => process.env.SWARM_OBSERVER_ORIGIN ?? "http://127.0.0.1:3732",
  sausage: () => process.env.SAUSAGE_OBSERVER_ORIGIN ?? "http://127.0.0.1:3733",
} as const;

type ObserverSource = keyof typeof OBSERVER_ORIGINS;

type RouteContext = {
  params: Promise<{ source: string; path: string[] }>;
};

const READ_ONLY_PATHS = new Set([
  "v1/observe/events",
  "v1/observe/snapshot",
]);

export const dynamic = "force-dynamic";

export async function GET(request: Request, context: RouteContext) {
  const { source, path } = await context.params;
  if (!(source in OBSERVER_ORIGINS)) {
    return Response.json({ error: "unknown observer source" }, { status: 404 });
  }

  const observerPath = path.join("/");
  if (!READ_ONLY_PATHS.has(observerPath)) {
    return Response.json({ error: "observer endpoint is read-only" }, { status: 404 });
  }

  const origin = OBSERVER_ORIGINS[source as ObserverSource]();
  const upstream = new URL(`/${observerPath}`, origin);
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
        "x-observer-source": source,
      },
    });
  } catch {
    return Response.json(
      { error: "observer sidecar unavailable", source },
      { status: 502, headers: { "cache-control": "no-store" } },
    );
  }
}
