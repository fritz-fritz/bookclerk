# Diagnostics collector (Cloudflare Worker → Backblaze B2)

In-repo Worker published with **Cloudflare Builds** (project root:
`tools/diagnostics-collector`).

| Method | Path | Who | Auth |
|--------|------|-----|------|
| `POST` | `/submit` | Libation clients | none (write-only; validated + heuristic secret reject) |
| `GET` | `/report?since=<ms>` | GitHub Action | `Authorization: Bearer <REPORT_API_KEY>` |
| `GET` | `/health` | probes | none |

Clients never see B2 credentials. The Action never talks to B2 directly —
it pulls assembled JSON from `/report`.

## Cloudflare Builds

1. Create a private B2 bucket (e.g. `libation-diagnostics`).
2. Create an application key with **write** (and separately **read** for the
   Worker `/report` path — same key is fine if the Worker holds it).
3. In Cloudflare: Workers → Create → Connect Git repo → set **Root directory**
   to `tools/diagnostics-collector` → deploy.
4. Set Worker secrets (dashboard or `wrangler secret put`):

```text
B2_KEY_ID
B2_APPLICATION_KEY
B2_BUCKET_ID
REPORT_API_KEY          # long random; shared with GitHub secret
CLIENT_IP_HASH_SALT     # optional
```

Local deploy (optional):

```bash
cd tools/diagnostics-collector
npm install
npx wrangler secret put B2_KEY_ID
npx wrangler secret put B2_APPLICATION_KEY
npx wrangler secret put B2_BUCKET_ID
npx wrangler secret put REPORT_API_KEY
npx wrangler deploy
```

## Libation client

```toml
[diagnostics]
share_reports = true
collector_url = "https://libation-diagnostics.<account>.workers.dev"
```

Libation POSTs to `{collector_url}/submit` (or the URL as-is if it already
ends with `/submit`).

## GitHub Action ingest

Workflow: [`.github/workflows/diagnostics-ingest.yml`](../../.github/workflows/diagnostics-ingest.yml)

Repository secrets:

- `DIAGNOSTICS_COLLECTOR_BASE_URL` — Worker origin (same as `collector_url`)
- `DIAGNOSTICS_REPORT_API_KEY` — must match Worker `REPORT_API_KEY`

**Copilot auth** (workflow picks automatically):

| Repo type | Secret | Billing |
|-----------|--------|---------|
| Personal (now) | `COPILOT_GITHUB_TOKEN` — fine-grained PAT with Copilot + Issues | Your Copilot seat |
| Organization (future) | *(none)* — uses workflow `GITHUB_TOKEN` | Org (needs [Copilot CLI policy](https://docs.github.com/en/copilot/how-tos/copilot-cli/use-copilot-cli-in-actions)) |

Legacy alias `DIAGNOSTICS_COPILOT_GITHUB_TOKEN` is still accepted.

Daily job: `GET /report?since=…` → Copilot CLI (prompt-injection guarded) →
GitHub Issues. See [`docs/diagnostics.md`](../../docs/diagnostics.md).
