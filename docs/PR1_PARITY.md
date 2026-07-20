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
| `liberate` | ✅ | `library liberate` — `--pdf`, `--license`, `-o` overrides, positional ASINs |
| `set-status` | ✅ | `library set-status` — `--downloaded` / `--not-downloaded` / `--force` + ASINs |
| `get-license` | ✅ | `library get-license --json` (summary) / `--full` (raw API JSON) |
| `search` | ✅ | `library search` (+ `--filter` saved quick filters) |
| `export` | ✅ | `library export --csv|--json|--xlsx` |
| `convert` | ✅ | `library convert` (m4b/m4a → mp3) |
| `import-account` | ✅ | `auth import --mkb79` |
| `login-external` | ✅ | `auth login --external` |
| `list-accounts` | ✅ | `auth list` |
| `get-setting` | ✅ | `config get/show/paths` (classic key aliases + `--bare`) |
| `copydb` | ✅ | `copydb` (SQLite `library.db` → PostgreSQL) |
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
| `FolderTemplate` / `FileTemplate` | ✅ | conditionals, truncation, replacement chars |
| `DownloadCoverArt` | ✅ | `download.download_cover` |
| `CreateCueSheet` | ✅ | `download.create_cue` |
| `AllowLibationFixup` | ✅ | `download.fixup_metadata` |
| `SaveMetadataToFile` | ✅ | `download.save_metadata_json` |
| `AutoDownloadEpisodes` | ✅ | `library.auto_liberate` |
| `AutoScan` | ✅ | `library.scan_interval_minutes` |
| `OverwriteExisting` | ✅ | `download.overwrite_existing` |
| `InProgress` | ✅ | `download.in_progress` |
| `ImportEpisodes` | ✅ | `library.import_episodes` |
| `ImportPlusTitles` | ✅ | `library.import_plus_titles` |
| `DownloadEpisodes` | ✅ | `library.download_episodes` |
| `BadBook` | ✅ | `download.bad_book_action` |
| `SplitFilesByChapter` | ✅ | `download.split_files_by_chapter` |
| `ChapterFileTemplate` / `ChapterTitleTemplate` | ✅ | `download.chapter_file_template` / `chapter_title_template` |
| `MinimumFileDuration` | ✅ | `download.minimum_file_duration_minutes` |
| `CombineNestedChapterTitles` | ✅ | `download.combine_nested_chapter_titles` |
| `MergeOpeningAndEndCredits` | ✅ | `download.merge_opening_and_end_credits` |
| `StripUnabridged` / `StripAudibleBrandAudio` | ✅ | `download.strip_unabridged` / `strip_audible_brand_audio` |
| `DownloadClipsBookmarks` | ✅ | `download.download_clips_bookmarks` |
| `RetainAaxFile` | ✅ | `download.retain_aax_file` |
| `DownloadSpeedLimit` | ✅ | `download.download_speed_limit_kbps` |
| `Lame*` (6 keys) | ✅ | `download.lame.*` |
| `ReplacementCharacters` | ✅ | `download.replacement_characters` |
| `MaxSampleRate` | ✅ | `download.max_sample_rate` |
| `CreationTime` / `LastWriteTime` | ✅ | `download.creation_time` / `last_write_time` (local storage) |

---

## Library / user metadata

| Field | Status |
| --- | --- |
| Tags | ✅ DB + search index + migrate import |
| User ratings (overall/performance/story) | ✅ DB + migrate import |
| Finished flag | ✅ DB + migrate import |
| Separate PDF status | ✅ |
| Publisher, length, categories, subtitle, published_at, content_kind | ✅ scan + DB |
| Lucene/Tantivy search index | ✅ |
| Saved quick filters | ✅ `library filters` list/save/delete |

---

## Explicitly out of PR1

| Item | Reason |
| --- | --- |
| Chardonnay / Classic GUI | Deferred post-PR1 per project plan |
| Password + CAPTCHA WebView login | GUI-only; OAuth headless paths cover CLI |
| Classic EF `LibationContext.db` copydb target | `copydb` exports libation-rs schema to PostgreSQL |
