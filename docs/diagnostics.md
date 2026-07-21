# Diagnostics & crash reports

## Operator config

```toml
[diagnostics]
share_reports = true
collector_url = "https://your-worker.example/v1/report"
```

No GitHub or B2 credentials on the Libation client. Reports are redacted
locally, then POSTed to your **write-only** Cloudflare Worker, which stores
objects in a private Backblaze B2 bucket. A GitHub Action later opens Issues.

## Architecture

```text
libation / libationd
    │  POST redacted JSON
    ▼
Cloudflare Worker (write-only + validation)
    │  b2_upload_file
    ▼
Backblaze B2  (diagnostics/incoming/*.json)
    │  scheduled GitHub Action (read key)
    ▼
GitHub Issues  (+ optional Copilot assignee)
```

GitHub Pages remains documentation-only (static; cannot accept POSTs).

Reference Worker: [`tools/diagnostics-collector/`](../tools/diagnostics-collector/).  
Ingest workflow: [`.github/workflows/diagnostics-ingest.yml`](../.github/workflows/diagnostics-ingest.yml).

### Enable ingest

1. Deploy the Worker with a **write-only** B2 application key.
2. Add repository secrets for the Action (prefer a **separate read** key):
   `B2_KEY_ID`, `B2_APPLICATION_KEY`, `B2_BUCKET`, `B2_ENDPOINT`, `B2_REGION`.
3. Set repository variable `DIAGNOSTICS_INGEST_ENABLED=true`.
4. Optional: `DIAGNOSTICS_ASSIGN_COPILOT=true` to assign new Issues to Copilot
   (requires Copilot coding agent for the repo).

## Redaction hardening

1. **Exact values** registered from config/env/auth/AWS (also percent-encoded forms)
2. Sensitive **field-name** denylist
3. **Pattern** matching (Audible tokens, Bearer, AWS key ids, GitHub PATs, PEM, …)
4. Remote uploads strip titles/authors/paths/home dirs and truncate long fields
5. **Upload abort** if any registered secret is still present after redaction
6. Collector rejects payloads that still match obvious secret heuristics

## Privacy

Reports include recent structured log events (redacted), Libation version, OS,
and trigger (`crash` / `error_burst`). Titles and account paths are scrubbed
before upload.
