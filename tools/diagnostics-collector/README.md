# Diagnostics collector (Cloudflare Worker → Backblaze B2)

Deployed by **GitHub Actions** ([`wrangler-action`](https://github.com/cloudflare/wrangler-action))
when `tools/diagnostics-collector/` changes on `main`:
[`.github/workflows/diagnostics-collector-deploy.yml`](../../.github/workflows/diagnostics-collector-deploy.yml).

**Published URL:**

```text
https://libation-diagnostics.fritztech.workers.dev
```

Pattern: `https://{name}.{WORKERS_DEV_SUBDOMAIN}.workers.dev` — see `wrangler.toml`.

| Method | Path | Who | Auth |
|--------|------|-----|------|
| `POST` | `/submit` | Libation clients | none (validated + secret heuristics) |
| `GET` | `/report?since=<ms>` | GitHub Action | `Authorization: Bearer <REPORT_API_KEY>` |
| `GET` | `/health` | probes | none |

## GitHub secrets (deploy + ingest)

| Secret | Used for |
|--------|----------|
| `CLOUDFLARE_API_TOKEN` | wrangler deploy |
| `CLOUDFLARE_ACCOUNT_ID` | wrangler deploy |
| `DIAGNOSTICS_REPORT_API_KEY` | Worker `REPORT_API_KEY` **and** ingest `/report` |
| `DIAGNOSTICS_B2_KEY_ID` | Worker `B2_KEY_ID` |
| `DIAGNOSTICS_B2_APPLICATION_KEY` | Worker `B2_APPLICATION_KEY` |
| `DIAGNOSTICS_B2_BUCKET_ID` | Worker `B2_BUCKET_ID` |

Create a [Cloudflare API token](https://developers.cloudflare.com/workers/wrangler/ci-cd/#api-token)
with Workers edit permissions. The deploy workflow uploads secrets to the Worker
in the same step via wrangler-action's `secrets:` block.

## Local dev

```bash
cd tools/diagnostics-collector
# Create .dev.vars with B2_* and REPORT_API_KEY (see .gitignore)
npm install
npm run dev
```

## Libation client

```toml
[diagnostics]
share_reports = true
workers_subdomain = "fritztech"
```

Or `collector_url = "https://libation-diagnostics.fritztech.workers.dev"`.

## Ingest workflow

[`.github/workflows/diagnostics-ingest.yml`](../../.github/workflows/diagnostics-ingest.yml) —
daily `/report` pull + Copilot CLI triage. Collector URL is derived automatically;
uses the same `DIAGNOSTICS_REPORT_API_KEY`.

See [`docs/diagnostics.md`](../../docs/diagnostics.md).
