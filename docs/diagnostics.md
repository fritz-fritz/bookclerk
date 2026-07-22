# Diagnostics & crash reports

## Operator config

After deploy CI has set `DIAGNOSTICS_COLLECTOR_BASE_URL`, build Libation with that
URL baked in:

```bash
LIBATION_DIAGNOSTICS_COLLECTOR_URL="https://your-worker.workers.dev" cargo build --release -p libation-cli
```

Release builds on `main` in CI pass the repository variable automatically.

```toml
[diagnostics]
share_reports = true
```

Override with `collector_url` or runtime `LIBATION_DIAGNOSTICS_COLLECTOR_URL`.

## Architecture

```text
libation / libationd  →  POST /submit  →  Worker
                              ↓
                         Backblaze B2
                              ↓
GitHub ingest (DIAGNOSTICS_COLLECTOR_BASE_URL)  →  Copilot CLI  →  Issues
```

## GitHub Actions

| Workflow | Purpose |
|----------|---------|
| `diagnostics-collector-deploy` | Deploy Worker; set `DIAGNOSTICS_COLLECTOR_BASE_URL` from `deployment-url` |
| `diagnostics-ingest` | Daily `/report` pull + Copilot triage |
| `ci` | Release builds with `LIBATION_DIAGNOSTICS_COLLECTOR_URL` from repo variable |

### Secrets

`CLOUDFLARE_API_TOKEN`, `CLOUDFLARE_ACCOUNT_ID`, `DIAGNOSTICS_REPORT_API_KEY`, `DIAGNOSTICS_B2_*`, optional `COPILOT_GITHUB_TOKEN`.

## Redaction & privacy

Exact-value redaction, upload abort, Worker heuristics, optional `CLIENT_IP_HASH_SALT`.
