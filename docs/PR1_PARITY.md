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
| `liberate` | ⚠️ | `library liberate` — has `--pdf`, positional ASINs; missing `--license`, `-o` overrides |
| `set-status` | ⚠️ | `library set-status` — missing `--downloaded` / `--not-downloaded` / `--force` |
| `get-license` | ⚠️ | `library get-license` — missing full JSON output |
| `search` | 🔄 | `library search` (in progress — Tantivy) |
| `export` | 🔄 | `library export` (in progress) |
| `convert` | ❌ | — |
| `import-account` | ❌ | — |
| `login-external` | ✅ | `auth login --external` |
| `list-accounts` | ✅ | `auth list` |
| `get-setting` | ⚠️ | `config get/show/paths` |
| `copydb` | ❌ | — |
| `version` | 🔄 | `libation version` (in progress) |
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
| `OverwriteExisting` | 🔄 | `download.overwrite_existing` |
| `InProgress` | ❌ | — |
| `ImportEpisodes` | 🔄 | `library.import_episodes` |
| `ImportPlusTitles` | 🔄 | `library.import_plus_titles` |
| `DownloadEpisodes` | ❌ | — |
| `BadBook` | 🔄 | `download.bad_book_action` |
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
| Tags | 🔄 DB + search index |
| User ratings (overall/performance/story) | 🔄 |
| Finished flag | 🔄 |
| Separate PDF status | 🔄 |
| Publisher, length, categories | ⚠️ scan stores subset |
| Lucene/Tantivy search index | 🔄 |
| Saved quick filters | ❌ |

---

## Implementation order (remaining PR1 work)

1. **Search** — Tantivy index + `library search` + index rebuild on scan
2. **User metadata** — DB columns + migrate from `UserDefinedItem` + search fields
3. **CLI surface** — export, convert, import-account, version, liberate `--pdf`/`--license`, set-status force modes
4. **Settings parity** — overwrite, bad-book, scan filters, in-progress dir, Lame tuning
5. **Liberate advanced** — split-by-chapter, clips/bookmarks, full metadata.json
6. **Templates** — conditionals, replacement chars, full tag set
7. **Ops** — copydb, download speed limit, file timestamps

---

## Explicitly out of PR1

| Item | Reason |
| --- | --- |
| Chardonnay / Classic GUI | Deferred post-PR1 per project plan |
| Password + CAPTCHA WebView login | GUI-only; OAuth headless paths cover CLI |
