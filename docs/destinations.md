# Destinations (output)

Acquire writes finished audio (and sidecars) through **destination plugins**.
Each destination under `[output.<name>]` has an `enabled` flag. **Multiple may
be enabled at once** — Bookclerk writes to every enabled destination.

Built-in destinations today:

| Id | Config table | Credentials |
| --- | --- | --- |
| Local filesystem | `[output.local]` | none |
| S3 / MinIO | `[output.s3]` | `AWS_*` env override → `encrypted_secrets` → SDK chain |

External `kind = "output"` plugins are discovered; loading is not implemented
yet ([plugins.md](plugins.md)).

## Local filesystem

```toml
[output.local]
enabled = true
# Default: @user/Audiobooks → ~/Audiobooks for the interactive / configured owner.
# root = "@user/Audiobooks"
# owner_user / owner_group accept a name or decimal id (Unix uid/gid).
# On Windows: account name (`alice`, `DOMAIN\alice`) or `S-1-…` SID.
# owner_user = "alice"       # or "1000"
# owner_group = "alice"      # or "1000"
# Explicit override (absolute path wins):
# root = "/data/Audiobooks"
# prefix = "library/"
```

| Root value | Resolves to |
| --- | --- |
| `@user/Audiobooks` (default) | `{owner home}/Audiobooks` |
| Relative (`Audiobooks`) | `{BOOKCLERK_FILES_DIR}/Audiobooks` |
| Absolute | unchanged |

Owner resolution (**env overrides config**):

`BOOKCLERK_OUTPUT_OWNER` → `output.local.owner_user` → `SUDO_USER` →
interactive user (not `root` / `bookclerk`). Group:
`BOOKCLERK_OUTPUT_OWNER_GROUP` → `output.local.owner_group`.

The user systemd unit sets `BOOKCLERK_OUTPUT_OWNER=%u`. When both env and TOML
are unset, the daemon captures the installing / setuid real user before
`@user` expansion and privilege drop. When the daemon runs as `bookclerk`
with no owner resolved, `@user` falls back to `{BOOKCLERK_FILES_DIR}/Audiobooks`
and logs a warning.

Also: `BOOKCLERK_OUTPUT_LOCAL_ROOT` overrides `output.local.root`.

After each write, Bookclerk sets ownership to the resolved owner when allowed:

| Platform | Mechanism | Notes |
| --- | --- | --- |
| Linux | `chown` + retained `CAP_CHOWN` after `setuid` | User-unit install needs the **setuid-root** helper (see below) |
| macOS | `seteuid` drop (real uid stays 0) + brief elevate for `chown` | LaunchDaemon must start as root; see security note in [operations.md](operations.md) |
| Windows | `SetNamedSecurityInfo` | Grant `SeRestorePrivilege` / `SeTakeOwnershipPrivilege` on the service account |

CLI as your login user already creates files as you; the daemon path is what
needs ownership transfer so media lands in `~/Audiobooks` owned by you.

## S3 / MinIO

```toml
[output.s3]
enabled = true
bucket = "my-audiobooks"
prefix = "library/"
region = "us-east-1"
# endpoint = "http://minio:9000"
# force_path_style = true
```

Credentials resolve in this order:

1. `BOOKCLERK_AWS_ACCESS_KEY_ID` + `BOOKCLERK_AWS_SECRET_ACCESS_KEY`
   (optional `BOOKCLERK_AWS_SESSION_TOKEN`) — process env override. Empty string
   counts as set (intentional override). These are not written to the DB unless
   you run `bookclerk config s3-credentials set`.
2. `encrypted_secrets` row `kind=s3`, `account_type=operator`, `account_id=default`, `name=default`
   (save with `bookclerk config s3-credentials set`; secrets are never accepted
   on argv). **Fail closed** if the row is present but cannot be unsealed —
   Bookclerk does not fall through to the SDK chain in that case.
3. AWS SDK **default provider chain** — shared config files (`~/.aws/credentials`,
   `~/.aws/config`), AWS SSO, and cloud identity (EC2/ECS/EKS roles, …).
   Installing the AWS CLI is not required.

`bookclerk config s3-credentials show|clear` inspects or removes the DB row.

Bucket/region/endpoint also accept `BOOKCLERK_OUTPUT_S3_*`
(or familiar `BOOKCLERK_S3_*`) env vars. Host-only endpoints are accepted;
`https://` is prepended when the value looks like a bare hostname. Whitespace-
only endpoint values are ignored.

Object user-metadata and local `.bookclerk-meta.json` support
`--match-storage` without downloading bodies. S3 timestamp metadata follows
`output.creation_time` / `output.last_write_time`.

## Multi-destination policy

When some destinations already have a title and others do not:

```toml
[output]
# sync_missing | refetch_missing | refetch_all
multi_destination = "sync_missing"
```

| Policy | Behavior |
| --- | --- |
| `sync_missing` (default) | Copy from a present destination into missing ones (no re-fetch) |
| `refetch_missing` | Re-download/encode; write only to missing destinations |
| `refetch_all` | Re-download/encode into every destination |

## Naming

Templates come from Libation’s NamingTemplate engine (`bookclerk-naming`).

```toml
[output]
naming_profile = "audiobookshelf"   # or "classic"
# folder_template / file_template override the profile when set
# folder_template = "<author>/<title>"
# file_template = "<title> [<asin>]"
```

| Profile | Intent |
| --- | --- |
| `audiobookshelf` (default) | Author / series / year-oriented layout friendly to ABS |
| `classic` | Libation desktop defaults (`<title short> [<id>]`, …) |

Per-destination overrides are allowed on `[output.local]` / `[output.s3]`
(`naming_profile`, `folder_template`, `file_template`,
`chapter_file_template`) so object keys can differ from the local tree.

CLI helpers:

```bash
bookclerk config template profiles
bookclerk config template tags
bookclerk config template preview <asin>
```

Path sanitization: `output.path_sanitization` (`auto` / `windows` / `posix` /
`s3` / `none`) or explicit `output.replacement_characters`.

## Packaging format & sidecars

```toml
[output]
format = "enriched_m4b"   # single_mp3 | split_mp3_by_chapter | …
download_cover = true
download_pdf = true
create_cue = true
fixup_metadata = true
save_chapter_json = true
cover_size = "500"        # 500 | 1215 | native
chapter_layout = "tree"   # tree | flat
```

Artifact (sidecar) failures are logged and do not fail the audio acquire.

Audible-specific knobs (Widevine, xHE-AAC, brand trim) are documented under
[sources.md](sources.md#audible).
