# Diagnostics & crash reports

## Operator config

```toml
[diagnostics]
share_reports = true
workers_subdomain = "fritztech"
```

Resolves to `https://libation-diagnostics.fritztech.workers.dev`.

Clients POST redacted JSON to `/submit`. GitHub Actions deploy the Worker and
run daily ingest → Copilot CLI → Issues.

## Architecture

```text
libation / libationd
    │  POST /submit
    ▼
https://libation-diagnostics.fritztech.workers.dev
    │  B2 upload
    ▼
Backblaze B2  (diagnostics/incoming/*.json)
    │
    │  GET /report?since=…
    ▼
GitHub Action (diagnostics-ingest) → Copilot CLI → Issues
```

## GitHub Actions

| Workflow | Trigger | Purpose |
|----------|---------|---------|
| [`diagnostics-collector-deploy.yml`](../.github/workflows/diagnostics-collector-deploy.yml) | Push to `main` changing `tools/diagnostics-collector/**` | Deploy Worker + upload secrets via [wrangler-action](https://github.com/cloudflare/wrangler-action) |
| [`diagnostics-ingest.yml`](../.github/workflows/diagnostics-ingest.yml) | Daily + manual | Pull `/report`, Copilot triage |

### Repository secrets

| Secret | Purpose |
|--------|---------|
| `CLOUDFLARE_API_TOKEN` | Deploy Worker |
| `CLOUDFLARE_ACCOUNT_ID` | Deploy Worker |
| `DIAGNOSTICS_REPORT_API_KEY` | Worker `REPORT_API_KEY` + ingest `/report` auth |
| `DIAGNOSTICS_B2_KEY_ID` | B2 write (deploy → Worker secret) |
| `DIAGNOSTICS_B2_APPLICATION_KEY` | B2 write |
| `DIAGNOSTICS_B2_BUCKET_ID` | B2 bucket id |

**Copilot (ingest only):** `COPILOT_GITHUB_TOKEN` (personal repo) or workflow `GITHUB_TOKEN` (org).

**Collector URL:** auto-derived (`libation-diagnostics` + `fritztech`); override with repo variable `DIAGNOSTICS_COLLECTOR_BASE_URL`.

### Prompt-injection guarding

See [`analyze-with-copilot.sh`](../tools/diagnostics-collector/scripts/analyze-with-copilot.sh).

## Redaction & privacy

Exact-value redaction, upload abort if secrets remain, Worker heuristics, optional
`CLIENT_IP_HASH_SALT` for hashed client IPs.
