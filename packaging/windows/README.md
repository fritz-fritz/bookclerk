# Windows service account for bookclerkd

Bookclerk expects a dedicated local account — not the interactive desktop user.
The tray companion runs in the interactive session; the daemon Log On As
`bookclerk` for isolation. Set `BOOKCLERK_OUTPUT_OWNER` to the installing user
so `@user/Audiobooks` expands under their profile.

## Create the account

```powershell
# Run elevated
net user bookclerk * /add /passwordchg:yes /expires:never
# Prefer a long random password stored in LAPS / a secret manager.
# Deny interactive logon via local security policy if desired.
mkdir C:\ProgramData\Bookclerk -Force
icacls C:\ProgramData\Bookclerk /inheritance:r `
  /grant:r "bookclerk:(OI)(CI)F" `
  /grant:r "SYSTEM:(OI)(CI)F" `
  /grant:r "Administrators:(OI)(CI)F"
```

Grant the service account write access to the owner's Audiobooks folder (default
`%USERPROFILE%\Audiobooks`), e.g.:

```powershell
icacls "$env:USERPROFILE\Audiobooks" /grant "bookclerk:(OI)(CI)M"
```

So acquired files end up owned by the installing user (same usage model as
Unix `chown`), grant the service account **SeRestorePrivilege** and/or
**SeTakeOwnershipPrivilege** (Local Security Policy → User Rights Assignment →
“Restore files and directories” / “Take ownership of files or other objects”,
or an equivalent group policy). Without those rights, Bookclerk still writes
the files but ownership transfer is skipped (debug log only).

Optional config (name or `S-1-…` SID):

```toml
[output.local]
root = "@user/Audiobooks"
owner_user = "alice"
# owner_group = "Users"
```

## Register as a service (NSSM example)

```powershell
nssm install bookclerkd C:\Program Files\Bookclerk\bookclerkd.exe
nssm set bookclerkd AppDirectory C:\ProgramData\Bookclerk
nssm set bookclerkd AppEnvironmentExtra `
  BOOKCLERK_FILES_DIR=C:\ProgramData\Bookclerk `
  BOOKCLERK_OUTPUT_OWNER=alice
nssm set bookclerkd ObjectName ".\bookclerk" "the-password"
nssm set bookclerkd Start SERVICE_AUTO_START
nssm start bookclerkd
```

Or use Windows Service Control Manager / sc.exe with `obj=` / `password=` set to
the `bookclerk` account. Do **not** run as `LocalSystem` for production — the
daemon will refuse SYSTEM when `daemon.identity.drop_privileges=true` and point
you here.

## Config knobs

```toml
[daemon.identity]
service_user = "bookclerk"
drop_privileges = true
allow_interactive_user = false
```

Env overrides: `BOOKCLERK_SERVICE_USER`, `BOOKCLERK_ALLOW_USER_RUN=1` (dev only),
`BOOKCLERK_OUTPUT_OWNER`.

## Plugin sandbox

External plugins are launched inside a **Windows AppContainer** (fail-closed)
with filesystem ACLs limited to that plugin's install dir, data dir, and
per-plugin cache (`cache\plugins\<id>`). A kill-on-close Job Object reaps the
process tree with the host. Disable only for debugging with
`BOOKCLERK_PLUGIN_SANDBOX=off`.
