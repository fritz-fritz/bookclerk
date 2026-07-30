# Windows service account for bookclerkd

Bookclerk expects a dedicated local account — not the interactive desktop user.

## Create the account

```powershell
# Run elevated
net user bookclerk * /add /fullnamepasswordchg:yes /expires:never
# Prefer a long random password stored in LAPS / a secret manager.
# Deny interactive logon via local security policy if desired.
mkdir C:\ProgramData\Bookclerk -Force
icacls C:\ProgramData\Bookclerk /inheritance:r `
  /grant:r "bookclerk:(OI)(CI)F" `
  /grant:r "SYSTEM:(OI)(CI)F" `
  /grant:r "Administrators:(OI)(CI)F"
```

## Register as a service (NSSM example)

```powershell
nssm install bookclerkd C:\Program Files\Bookclerk\bookclerkd.exe
nssm set bookclerkd AppDirectory C:\ProgramData\Bookclerk
nssm set bookclerkd AppEnvironmentExtra BOOKCLERK_FILES_DIR=C:\ProgramData\Bookclerk
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

Env overrides: `BOOKCLERK_SERVICE_USER`, `BOOKCLERK_ALLOW_USER_RUN=1` (dev only).

Plugins are assigned to a Windows Job Object (kill-on-close). FS isolation for
`master.key` / `library.db` comes from ACLs on `BOOKCLERK_FILES_DIR` owned by
`bookclerk`.
