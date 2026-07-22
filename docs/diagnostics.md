# Diagnostics & crash reports

## Operator config

**Option A — derive workers.dev URL** (recommended with Cloudflare Builds):

```toml
[diagnostics]
share_reports = true
workers_subdomain = "fritztech"
# collector_worker_name = "libation-diagnostics"  # optional
```

Resolves to `https://libation-diagnostics.fritztech.workers.dev` (must match
`name` + `[vars].WORKERS_DEV_SUBDOMAIN` in `tools/diagnostics-collector/wrangler.toml`).

**Option B — explicit URL:**

```toml
collector_url = "https://libation-diagnostics.fritztech.workers.dev"
```

Clients POST redacted JSON to `/submit`. A GitHub Action pulls `/report` and
uses Copilot CLI to open Issues.

## Architecture

```text
libation / libationd
    │  POST /submit
    ▼
https://libation-diagnostics.fritztech.workers.dev  (Cloudflare Builds)
    │  B2 upload
    ▼
Backblaze B2  (diagnostics/incoming/*.json)
    │
    │  GET /report?since=…  (DIAGNOSTICS_REPORT_API_KEY)
    ▼
GitHub Action → Copilot CLI → Issues
```

Worker: `tools/diagnostics-collector` · Workflow: `.github/workflows/diagnostics-ingest.yml`

## Secrets & URLs

### One API key, two places

`REPORT_API_KEY` must match in:

| Where | Name |
|-------|------|
| Cloudflare Builds → Build secrets | `REPORT_API_KEY` |
| GitHub → Actions secrets | `DIAGNOSTICS_REPORT_API_KEY` |

Cloudflare Builds does **not** read GitHub secrets. Set the same value in both
dashboards (or set Worker runtime secrets once under Variables & Secrets).

### Cloudflare Builds deploy

1. Connect repo; root `tools/diagnostics-collector`; deploy command `npm run deploy`.
2. Add **Build variables and secrets** for `B2_*` and `REPORT_API_KEY`.
3. `npm run deploy` runs `wrangler deploy --secrets-file` so build-time secrets
   become Worker runtime secrets ([docs](https://developers.cloudflare.com/workers/ci-cd/builds/configuration/)).

### GitHub Action

**Required secret:** `DIAGNOSTICS_REPORT_API_KEY`

**Collector URL:** auto-derived — `https://libation-diagnostics.fritztech.workers.dev`
(repository variables `DIAGNOSTICS_WORKER_NAME`, `DIAGNOSTICS_WORKERS_SUBDOMAIN`, or
`DIAGNOSTICS_COLLECTOR_BASE_URL` to override).

**Copilot:** `COPILOT_GITHUB_TOKEN` (personal) or workflow `GITHUB_TOKEN` (org).

### Prompt-injection guarding

See [`analyze-with-copilot.sh`](../tools/diagnostics-collector/scripts/analyze-with-copilot.sh).

## Redaction & privacy

See previous sections — exact-value redaction, upload abort, Worker heuristics,
optional salted IP hash via `CLIENT_IP_HASH_SALT`.
