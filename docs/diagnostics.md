# Diagnostics & crash reports

## Operator switch (boolean only)

```toml
[diagnostics]
share_reports = true
```

No GitHub token is required on the Libation client. When enabled, crash and
ERROR-burst reports POST a **pre-redacted** JSON payload to the project
collector.

Optional override:

```toml
collector_url = "https://your-collector.example/v1/report"
```

Empty `collector_url` uses the built-in project default Worker URL.

## Why not GitHub Pages for collection?

[GitHub Pages](https://docs.github.com/en/pages) serves **static** files only —
it cannot accept `POST` bodies or open Issues. We still use Pages (or `/docs`)
for **privacy documentation**. Issue filing is done by a small
[Cloudflare Worker](../tools/diagnostics-collector/) that holds a server-side
token and creates Issues in this repository.

## What is redacted?

1. **Exact values** from config/env/auth once they are in memory  
   (`LIBATION_AUTH_PASSWORD`, password-file contents, Audible access/refresh
   tokens, `AWS_*` keys, …)
2. Sensitive **field names** (`password`, `token`, `refresh_token`, …)
3. **Patterns** (Audible `Atna|` / `Atnr|`, Bearer, AWS key ids, GitHub PATs, PEM blocks, …)
4. For remote reports: titles/authors/paths/home dirs / `Accounts/*.auth`

## Privacy

Reports include only recent structured log events (already redacted), Libation
version, OS name, and a trigger (`crash` / `error_burst`). Library titles and
account identifiers are scrubbed before upload.
