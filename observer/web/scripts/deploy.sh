#!/bin/sh
# Build and deploy the live console Worker (web app, read API, ingest, LiveHub).
#
#   observer/web/scripts/deploy.sh            # build, then deploy
#   observer/web/scripts/deploy.sh --no-build # deploy an existing dist/
#
# One-time setup on the Cloudflare account:
#   npx wrangler r2 bucket create benchmark-live
#   npx wrangler secret put LIVE_INGEST_TOKEN -c dist/server/wrangler.json
# Set LIVE_CUSTOM_DOMAIN (for example live.benchmark.3720.org) to attach the
# public hostname; without it the Worker is reachable on workers.dev only.
set -eu

cd "$(dirname "$0")/.."
if [ "${1:-}" != "--no-build" ]; then
  npm run build
fi
exec npx --yes wrangler@4 deploy -c dist/server/wrangler.json
