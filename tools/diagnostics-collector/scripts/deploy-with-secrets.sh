#!/usr/bin/env bash
# Cloudflare Builds deploy: upload runtime secrets from build env, then deploy.
#
# In Workers Builds → Settings → Build → Build variables and secrets, set:
#   B2_KEY_ID, B2_APPLICATION_KEY, B2_BUCKET_ID, REPORT_API_KEY
#   (optional CLIENT_IP_HASH_SALT)
#
# Build secrets are available only during the build/deploy step. This script
# passes them to `wrangler deploy --secrets-file` so they become Worker runtime
# secrets. See https://developers.cloudflare.com/workers/configuration/secrets/
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

SECRETS_FILE=$(mktemp)
trap 'rm -f "$SECRETS_FILE"' EXIT

for key in B2_KEY_ID B2_APPLICATION_KEY B2_BUCKET_ID REPORT_API_KEY CLIENT_IP_HASH_SALT; do
  val="${!key:-}"
  if [ -n "$val" ]; then
    printf '%s=%s\n' "$key" "$val" >> "$SECRETS_FILE"
  fi
done

ARGS=(deploy)
if [ -s "$SECRETS_FILE" ]; then
  echo "Uploading runtime secrets from build environment ($(wc -l < "$SECRETS_FILE") keys)"
  ARGS+=(--secrets-file "$SECRETS_FILE")
else
  echo "No build secrets in environment; deploying without --secrets-file (runtime secrets must exist in dashboard)"
fi

exec npx wrangler "${ARGS[@]}"
