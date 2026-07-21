# Diagnostics collector (Cloudflare Worker)

Receives redacted Libation crash/ERROR reports and opens GitHub Issues.

Clients only set:

```toml
[diagnostics]
share_reports = true
```

GitHub Pages cannot accept POSTs; this Worker is the collection endpoint.
Privacy documentation: [`docs/diagnostics.md`](../../docs/diagnostics.md).

## Deploy

```bash
cd tools/diagnostics-collector
npx wrangler deploy
npx wrangler secret put GITHUB_TOKEN   # Issues: write on the target repo
```

Create a `diagnostics` label on the repo if `ISSUE_LABELS` includes it.

Point Libation at the Worker URL if it differs from the built-in default:

```toml
[diagnostics]
share_reports = true
collector_url = "https://libation-diagnostics.<account>.workers.dev/v1/report"
```
