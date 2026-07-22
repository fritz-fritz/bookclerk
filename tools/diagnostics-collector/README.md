# Diagnostics collector (Cloudflare Worker → Backblaze B2)

Deployed by **GitHub Actions** ([wrangler-action](https://github.com/cloudflare/wrangler-action)):
[`.github/workflows/diagnostics-collector-deploy.yml`](../../.github/workflows/diagnostics-collector-deploy.yml).

After deploy, the workflow stores `deployment-url` in:

- **Repository variable** `DIAGNOSTICS_COLLECTOR_BASE_URL` (ingest workflow)
- **`config/diagnostics-collector.url`** (Libation compile-time default)

| Method | Path | Who | Auth |
|--------|------|-----|------|
| `POST` | `/submit` | Libation clients | none (validated + secret heuristics) |
| `GET` | `/report?since=<ms>` | GitHub Action | `Authorization: Bearer <REPORT_API_KEY>` |
| `GET` | `/health` | probes | none |

## GitHub secrets

| Secret | Purpose |
|--------|---------|
| `CLOUDFLARE_API_TOKEN` / `CLOUDFLARE_ACCOUNT_ID` | Deploy |
| `DIAGNOSTICS_REPORT_API_KEY` | Worker `REPORT_API_KEY` + ingest |
| `DIAGNOSTICS_B2_*` | B2 → Worker secrets |

## Libation client

With deploy CI, leave `collector_url` empty — the URL from
`config/diagnostics-collector.url` is baked in at compile time:

```toml
[diagnostics]
share_reports = true
```

Override anytime with explicit `collector_url` or `LIBATION_DIAGNOSTICS_COLLECTOR_URL`.

## Ingest

[`.github/workflows/diagnostics-ingest.yml`](../../.github/workflows/diagnostics-ingest.yml) reads
`DIAGNOSTICS_COLLECTOR_BASE_URL`.

See [`docs/diagnostics.md`](../../docs/diagnostics.md).
