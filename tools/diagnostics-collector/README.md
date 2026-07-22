# Diagnostics collector (Cloudflare Worker → Backblaze B2)

In-repo Worker published with **Cloudflare Builds** (project root:
`tools/diagnostics-collector`).

**Published URL** (workers.dev):

```text
https://libation-diagnostics.fritztech.workers.dev
```

Pattern: `https://{name}.{WORKERS_DEV_SUBDOMAIN}.workers.dev` — see `[vars]` in
`wrangler.toml`. Change `WORKERS_DEV_SUBDOMAIN` if your account subdomain differs.

| Method | Path | Who | Auth |
|--------|------|-----|------|
| `POST` | `/submit` | Libation clients | none (validated + secret heuristics) |
| `GET` | `/report?since=<ms>` | GitHub Action | `Authorization: Bearer <REPORT_API_KEY>` |
| `GET` | `/health` | probes | none |

## Cloudflare Builds setup

1. Connect this repo: Workers → **libation-diagnostics** → Settings → **Builds**.
2. **Root directory:** `tools/diagnostics-collector`
3. **Deploy command:** `npm run deploy` (runs `wrangler deploy --secrets-file` from build env)
4. **Build variables and secrets** (Settings → Build → *not* runtime Variables):

   | Name | Notes |
   |------|--------|
   | `B2_KEY_ID` | B2 application key id |
   | `B2_APPLICATION_KEY` | B2 application key |
   | `B2_BUCKET_ID` | B2 bucket id |
   | `REPORT_API_KEY` | Long random; **same value** as GitHub `DIAGNOSTICS_REPORT_API_KEY` |

   Per [Cloudflare Builds configuration](https://developers.cloudflare.com/workers/ci-cd/builds/configuration/),
   **build secrets are only available during the build/deploy step**. Our
   `scripts/deploy-with-secrets.sh` uploads them to the Worker at deploy time via
   [`wrangler deploy --secrets-file`](https://developers.cloudflare.com/workers/configuration/secrets/).
   They are **not** automatically synced from GitHub — set the same values in
   Cloudflare Builds and GitHub separately (or set runtime secrets once in
   Workers → Variables & Secrets and use plain `npx wrangler deploy`).

5. Optional runtime-only secret: `CLIENT_IP_HASH_SALT` (dashboard or build secret)

Local dev:

```bash
cd tools/diagnostics-collector
cp .dev.vars.example .dev.vars   # fill in values
npm install
npm run dev
```

## Libation client

Either full URL or workers.dev subdomain:

```toml
[diagnostics]
share_reports = true
workers_subdomain = "fritztech"
# collector_worker_name = "libation-diagnostics"  # optional; default matches wrangler name
```

Or explicit override:

```toml
collector_url = "https://libation-diagnostics.fritztech.workers.dev"
```

Libation POSTs to `{base}/submit`.

## GitHub Action ingest

Workflow: [`.github/workflows/diagnostics-ingest.yml`](../../.github/workflows/diagnostics-ingest.yml)

**Secret (required):**

- `DIAGNOSTICS_REPORT_API_KEY` — must match Worker `REPORT_API_KEY`

**Collector URL** — derived automatically (no secret):

```text
https://libation-diagnostics.fritztech.workers.dev
```

Override with repository **variable** `DIAGNOSTICS_COLLECTOR_BASE_URL`, or tune
`DIAGNOSTICS_WORKER_NAME` / `DIAGNOSTICS_WORKERS_SUBDOMAIN` (defaults:
`libation-diagnostics` / `fritztech`).

**Copilot:** `COPILOT_GITHUB_TOKEN` (personal repo) or workflow `GITHUB_TOKEN` (org).

See [`docs/diagnostics.md`](../../docs/diagnostics.md).
