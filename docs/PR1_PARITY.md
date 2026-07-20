# PR1 parity: libation-rs vs Libation Chardonnay (headless)

**Goal:** Full feature parity with [Libation Chardonnay](https://github.com/rmcrackan/Libation) for headless/CLI/daemon use. **GUI is deferred** until after PR1.

Chardonnay and Classic share the same backend; this matrix uses upstream `master` / LibationCli + `Configuration.PersistentSettings.cs` as the reference.

## Status legend

| Symbol | Meaning |
| --- | --- |
| ✅ | Implemented |
| ⚠️ | Partial / subset |
| ❌ | Not yet implemented |
| 🚫 | Out of PR1 scope (GUI-only) |

---

## CLI verbs (LibationCli)

| Verb | Status | libation-rs |
| --- | --- | --- |
| `scan` | ✅ | `library scan` |
| `liberate` | ⚠️ | `library liberate` — has `--pdf`, `--license`, positional ASINs; missing `-o` overrides |
| `set-status` | ✅ | `library set-status` — `--downloaded` / `--not-downloaded` / `--force` + ASINs |
| `get-license` | ✅ | `library get-license --json` (summary) / `--full` (raw API JSON) |
| `search` | ✅ | `library search` |
| `export` | ✅ | `library export --csv|--json|--xlsx` |
| `convert` | ✅ | `library convert` (m4b/m4a → mp3) |
| `import-account` | ✅ | `auth import --mkb79` |
| `login-external` | ✅ | `auth login --external` |
| `list-accounts` | ✅ | `auth list` |
| `get-setting` | ⚠️ | `config get/show/paths` |
| `copydb` | ❌ | — |
| `version` | ✅ | `libation version` |
| `help` | ✅ | clap |

---

## Settings.json → config.toml

| Classic key | Status | libation-rs key |
| --- | --- | --- |
| `Books` | ✅ | `storage.local.root` |
| `FileDownloadQuality` | ✅ | `download.quality` |
| `DecryptToLossy` | ✅ | `download.format` |
| `UseWidevine` | ✅ | `download.widevine` |
| `Request_xHE_AAC` | ✅ | `download.xhe_aac` |
| `FolderTemplate` / `FileTemplate` | ⚠️ | subset of tags only |
| `DownloadCoverArt` | ✅ | `download.download_cover` |
| `CreateCueSheet` | ✅ | `download.create_cue` |
| `AllowLibationFixup` | ✅ | `download.fixup_metadata` |
| `SaveMetadataToFile` | ⚠️ | `download.save_chapter_json` (chapters JSON, not full catalog metadata.json) |
| `AutoDownloadEpisodes` | ✅ | `library.auto_liberate` |
| `AutoScan` | ✅ | `library.scan_interval_minutes` |
| `OverwriteExisting` | ✅ | `download.overwrite_existing` |
| `InProgress` | ✅ | `download.in_progress` |
| `ImportEpisodes` | ✅ | `library.import_episodes` |
| `ImportPlusTitles` | ✅ | `library.import_plus_titles` |
| `DownloadEpisodes` | ❌ | — |
| `BadBook` | ✅ | `download.bad_book_action` |
| `SplitFilesByChapter` | ❌ | — |
| `ChapterFileTemplate` / `ChapterTitleTemplate` | ❌ | — |
| `MinimumFileDuration` | ❌ | — |
| `CombineNestedChapterTitles` | ❌ | — |
| `MergeOpeningAndEndCredits` | ❌ | — |
| `StripUnabridged` / `StripAudibleBrandAudio` | ❌ | — |
| `DownloadClipsBookmarks` | ❌ | — |
| `RetainAaxFile` | ❌ | — |
| `DownloadSpeedLimit` | ❌ | — |
| `Lame*` (6 keys) | ❌ | — |
| `ReplacementCharacters` | ❌ | — |
| `MaxSampleRate` | ❌ | — |
| `CreationTime` / `LastWriteTime` | ❌ | — |

---

## Library / user metadata

| Field | Status |
| --- | --- |
| Tags | ✅ DB + search index + migrate import |
| User ratings (overall/performance/story) | ✅ DB + migrate import |
| Finished flag | ✅ DB + migrate import |
| Separate PDF status | ✅ |
| Publisher, length, categories | ⚠️ scan stores subset |
| Lucene/Tantivy search index | ✅ |
| Saved quick filters | ❌ |

---

## Implementation order (remaining PR1 work)

1. **Liberate advanced** — split-by-chapter, clips/bookmarks, full metadata.json, `-o` runtime overrides
2. **Templates** — conditionals, replacement chars, full tag set
3. **Ops** — copydb, download speed limit, file timestamps, `DownloadEpisodes`

---

## Explicitly out of PR1

| Item | Reason |
| --- | --- |
| Chardonnay / Classic GUI | Deferred post-PR1 per project plan |
| Password + CAPTCHA WebView login | GUI-only; OAuth headless paths cover CLI |
