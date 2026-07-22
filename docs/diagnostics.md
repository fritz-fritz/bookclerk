# Diagnostics & crash reports

## Operator config

After the deploy workflow has run at least once, enable sharing with no URL config:

```toml
[diagnostics]
share_reports = true
```

Libation uses `config/diagnostics-collector.url` (updated by deploy CI) at compile
time when `collector_url` is unset. Override with explicit `collector_url` or
`LIBATION_DIAGNOSTICS_COLLECTOR_URL`.

## Architecture

```text
libation / libationd  →  POST /submit  →  Worker (deployment-url from wrangler)
                              ↓
                         Backblaze B2
                              ↓
GitHub ingest (DIAGNOSTICS_COLLECTOR_BASE_URL)  →  Copilot CLI  →  Issues
```

## GitHub Actions

| Workflow | Purpose |
|----------|---------|
| `diagnostics-collector-deploy` | Deploy Worker; set `DIAGNOSTICS_COLLECTOR_BASE_URL` + commit `config/diagnostics-collector.url` |
| `diagnostics-ingest` | Daily `/report` pull + Copilot triage |

### Secrets

`CLOUDFLARE_API_TOKEN`, `CLOUDFLARE_ACCOUNT_ID`, `DIAGNOSTICS_REPORT_API_KEY`, `DIAGNOSTICS_B2_*`, optional `COPILOT_GITHUB_TOKEN`.

### Collector URL

Set automatically from [wrangler-action `deployment-url`](https://github.com/cloudflare/wrangler-action).
No manual subdomain assembly.

## Redaction & privacy

Exact-value redaction, upload abort, Worker heuristics, optional `CLIENT_IP_HASH_SALT`.
