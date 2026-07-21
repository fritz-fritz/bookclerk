# Diagnostics collector: Cloudflare → Backblaze B2 (write-only)

Libation clients POST redacted JSON to this Worker. The Worker validates input
and uploads an object to B2. Clients never receive B2 credentials and cannot
list or read the bucket through this endpoint.

Issue creation is **not** done here. A scheduled GitHub Action
(`.github/workflows/diagnostics-ingest.yml`) reads new objects with
repository secrets and opens Issues (optionally assigning Copilot).

## Deploy the write-only ingress

1. Create a private B2 bucket (e.g. `libation-diagnostics`).
2. Create an application key with **write-only** access to that bucket.
3. Deploy:

```bash
cd tools/diagnostics-collector
npx wrangler secret put B2_KEY_ID
npx wrangler secret put B2_APPLICATION_KEY
npx wrangler secret put B2_BUCKET_ID
npx wrangler deploy
```

4. Point Libation at the Worker:

```toml
[diagnostics]
share_reports = true
collector_url = "https://libation-diagnostics-b2-ingress.<account>.workers.dev/v1/report"
```

## Ingest / Issues

Configure repository secrets for the Action (separate **read** B2 key is fine):

- `B2_KEY_ID`, `B2_APPLICATION_KEY`, `B2_BUCKET` (bucket name)
- `B2_ENDPOINT` — S3-compatible endpoint, e.g. `https://s3.us-west-004.backblazeb2.com`
- `B2_REGION` — e.g. `us-west-004`

See [`docs/diagnostics.md`](../../docs/diagnostics.md).
