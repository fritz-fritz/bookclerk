# Source endpoint probes

Live, API-first smoke checks for candidate audiobook sources (no cookie
scraping). Mirrors the intent of `scripts/librofm-apk-probe/`: prove the
reverse-engineered surface is reachable before wiring a `ContentSource`.

## Run

```bash
# Public / contract probes only (no accounts required)
python3 scripts/source-probes/probe_all.py

# Optional: Storytel password encryption for auth probes
pip install pycryptodome

# Subset
python3 scripts/source-probes/probe_all.py --sources graphicaudio,chirp

# Auth probes when you have test accounts
export TEST_GA_EMAIL=... TEST_GA_PASSWORD=...
export TEST_CHIRP_EMAIL=... TEST_CHIRP_PASSWORD=...
export TEST_STORYTEL_EMAIL=... TEST_STORYTEL_PASSWORD=...
export TEST_ABC_EMAIL=... TEST_ABC_PASSWORD=...
python3 scripts/source-probes/probe_all.py
```

Writes `artifacts/source-probes/report.{md,json}` (gitignored by default;
commit snapshots only when intentionally documenting a probe pass).

## Credentials checklist

| Source | Env vars | Account notes |
| --- | --- | --- |
| GraphicAudio | `TEST_GA_EMAIL`, `TEST_GA_PASSWORD` | Owned/purchased library preferred; login sends `client_id` device id |
| Chirp | `TEST_CHIRP_EMAIL`, `TEST_CHIRP_PASSWORD` | GraphQL `signIn` (password API, not cookies) |
| Storytel | `TEST_STORYTEL_EMAIL`, `TEST_STORYTEL_PASSWORD` | Subscription; password AES-CBC before POST (needs `pycryptodome`) |
| Audiobooks.com | `TEST_ABC_EMAIL`, `TEST_ABC_PASSWORD` | Storytel USA; app `apiKey` is embedded in the probe (from APK) |
| Kobo | *(none yet)* | Browser **ActivateOnWeb** after anonymous device auth — same UX class as Audible device login |
| Downpour | `TEST_DOWNPOUR_EMAIL`, `TEST_DOWNPOUR_PASSWORD` | Deferred until library REST is deobfuscated |
| Podimo | `TEST_PODIMO_EMAIL`, `TEST_PODIMO_PASSWORD` | Deferred until Cloudflare egress is workable |
| LibriVox | — | Public catalog; no auth |

Do **not** put passwords on argv. Prefer cheap/owned libraries for GA/Chirp;
Storytel/ABC are subscription models — use throwaway accounts and do not
bulk-download.

## Preference: API over cookies

Probes target mobile/store APIs discovered from APKs and community clients:

- GraphicAudio Retrofit (`/access/activation/login`, `/access/api/…`)
- Chirp GraphQL (`signIn`, library queries) — not session cookies
- Storytel Android `login.action` + JWT bookshelf
- Audiobooks.com `/api/v2/` form posts + APK `apiKey`
- Kobo `storeapi.kobo.com` device auth + init resources

Web cookie scrapers (Chirp `ci_session`, ABC HTML stream pages) are fallbacks
only if an API path dies.
