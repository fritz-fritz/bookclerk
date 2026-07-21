# Diagnostics & crash reports

## Operator config

```toml
[diagnostics]
share_reports = true
# Worker origin — Libation POSTs to {url}/submit
collector_url = "https://your-worker.example"
```

No GitHub or B2 credentials on the Libation client. Reports are redacted
locally, then POSTed to `/submit` on your Cloudflare Worker, which validates,
enriches, and stores objects in a private Backblaze B2 bucket. A scheduled
GitHub Action calls Worker `/report` (shared secret) and uses Copilot CLI to
open Issues.

## Architecture

```text
libation / libationd
    │  POST /submit  (redacted JSON)
    ▼
Cloudflare Worker  (validate + enrich)
    │  b2_upload_file
    ▼
Backblaze B2  (diagnostics/incoming/*.json)
    │
    │  GET /report?since=…  (REPORT_API_KEY)
    ▼
GitHub Action (daily)
    │  Copilot CLI (prompt-injection guarded)
    ▼
GitHub Issues
```

Worker lives in-repo and is published via **Cloudflare Builds** (root:
`tools/diagnostics-collector`).  
Ingest workflow: [`.github/workflows/diagnostics-ingest.yml`](../.github/workflows/diagnostics-ingest.yml).

### Enable ingest

1. Connect the repo in Cloudflare Builds; root `tools/diagnostics-collector`.
2. Set Worker secrets: `B2_*`, `REPORT_API_KEY` (see Worker README).
3. Set GitHub repository secrets:
   - `DIAGNOSTICS_COLLECTOR_BASE_URL`
   - `DIAGNOSTICS_REPORT_API_KEY` (same value as Worker `REPORT_API_KEY`)
   - `DIAGNOSTICS_COPILOT_GITHUB_TOKEN`
4. Run **diagnostics-ingest** manually once, or wait for the daily schedule.

### Prompt-injection guarding

Report bodies are untrusted. The Action script
[`analyze-with-copilot.sh`](../tools/diagnostics-collector/scripts/analyze-with-copilot.sh):

- strips C0 control characters and caps size
- wraps JSON in `UNTRUSTED_DATA` fences with explicit SECURITY instructions
- tells Copilot to ignore instructions embedded in the data
- falls back to a single `gh issue create` if Copilot CLI fails

## Redaction hardening

1. **Exact values** registered from config/env/auth/AWS (also percent-encoded forms)
2. Sensitive **field-name** denylist
3. **Pattern** matching (Audible tokens, Bearer, AWS key ids, GitHub PATs, PEM, …)
4. Remote uploads strip titles/authors/paths/home dirs and truncate long fields
5. **Upload abort** if any registered secret is still present after redaction
6. Worker rejects payloads that still match obvious secret heuristics

## Privacy

Reports include recent structured log events (redacted), Libation version, OS,
and trigger (`crash` / `error_burst`). Titles and account paths are scrubbed
before upload. The Worker may attach a salted hash of the client IP when
`CLIENT_IP_HASH_SALT` is set (never the raw IP).
