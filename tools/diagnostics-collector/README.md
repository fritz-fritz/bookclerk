# Diagnostics collector (Cloudflare Worker → Backblaze B2)

Deployed by **GitHub Actions** ([wrangler-action](https://github.com/cloudflare/wrangler-action)):
[`.github/workflows/diagnostics-collector-deploy.yml`](../../.github/workflows/diagnostics-collector-deploy.yml).

After deploy, `deployment-url` is stored in repository variable
**`DIAGNOSTICS_COLLECTOR_BASE_URL`**.

| Method | Path | Who | Auth |
|--------|------|-----|------|
| `POST` | `/submit` | Libation clients | none (validated + secret heuristics) |
| `GET` | `/report?since=<ms>` | GitHub Action | `Authorization: Bearer <REPORT_API_KEY>` |
| `GET` | `/health` | probes | none |

Each `/submit` is assigned a **`report_id` UUID**; the B2 object is
`diagnostics/incoming/<report_id>.json`. Ingest/Copilot issues list these IDs
for manual review.

## GitHub secrets

| Secret | Purpose |
|--------|---------|
| `CLOUDFLARE_API_TOKEN` / `CLOUDFLARE_ACCOUNT_ID` | Deploy |
| `DIAGNOSTICS_REPORT_API_KEY` | Worker `REPORT_API_KEY` + ingest |
| `DIAGNOSTICS_B2_*` | B2 → Worker secrets |

## Libation client

Bake the collector URL at build time (CI does this automatically):

```bash
LIBATION_DIAGNOSTICS_COLLECTOR_URL="https://libation-diagnostics.fritztech.workers.dev" \
  cargo build --release -p libation-cli -p libationd
```

Then enable sharing without a config URL:

```toml
[diagnostics]
share_reports = true
```

Override anytime with `collector_url` or runtime `LIBATION_DIAGNOSTICS_COLLECTOR_URL`.

## Ingest

[`.github/workflows/diagnostics-ingest.yml`](../../.github/workflows/diagnostics-ingest.yml) reads
`DIAGNOSTICS_COLLECTOR_BASE_URL`.

See [`docs/diagnostics.md`](../../docs/diagnostics.md).
