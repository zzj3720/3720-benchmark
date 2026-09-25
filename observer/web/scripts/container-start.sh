#!/bin/sh
set -eu
# Content-hashed chunks survive a release switch so already-open/cached HTML
# can still import the previous release. The shared volume contains assets only.
cp -Rn /app/immutable-assets/. /app/dist/client/assets/
exec node /app/server.js
