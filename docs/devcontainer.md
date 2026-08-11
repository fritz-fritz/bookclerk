# Dev Container

Bookclerk’s Dev Container gives a controlled Linux build environment (Debian
Bookworm + Rust + OpenSSL headers + Node) so local hosts without `libssl-dev`
(or with mismatched toolchains) can still compile.

The same Dockerfile is shared with Cursor Cloud Agents
([`.devcontainer/Dockerfile`](../.devcontainer/Dockerfile), referenced from
[`.cursor/environment.json`](../.cursor/environment.json)).

The workspace is **bind-mounted**. Cargo writes into the repo’s `target/` and
`.cargo-home/` (registry/git), and Vite into `ui/dist/`. `target/debug` and
`target/release` are prepended to `PATH`, and `BOOKCLERK_FILES_DIR` defaults to
`<workspace>/BookclerkFiles`, so after a build you can exercise the CLI
in-container:

```bash
cargo build -p bookclerk-cli -p bookclerkd -p bookclerk-jail -p bookclerk-media-worker
bookclerk version
cargo stage-plugins   # or: cargo dev / cargo dev-cli -- …
```

On a **Linux desktop host**, reopen with the alternate config
[`.devcontainer/devcontainer.linux-desktop.json`](../.devcontainer/devcontainer.linux-desktop.json)
(Command Palette → **Dev Containers: Reopen in Container…** and pick that file, or
set `"name"` / open via the config picker). That variant bind-mounts the host
session runtime (`$XDG_RUNTIME_DIR` → `/run/host-user`) and `/tmp/.X11-unix`, and
sets `DBUS_SESSION_BUS_ADDRESS` so `bookclerkd`’s in-process tray (`ksni`) can
appear on the **host** StatusNotifier panel. It also relaxes AppArmor/SELinux
labels for Landlock/sandbox experiments — keep those `runArgs` out of the
default portable config.

Docker fails container create if a bind `source` is missing or
`${localEnv:XDG_RUNTIME_DIR}` is empty — it does not skip the mount. That is why
the default `devcontainer.json` omits desktop mounts so headless hosts can start
cleanly.

Fallback: run the bind-mounted binaries on the host itself (glibc ≥ Bookworm):

```bash
# inside the Dev Container
cargo build -p bookclerkd -p bookclerk-cli
cd ui && npm run build

# on the host (same checkout)
BOOKCLERK_FILES_DIR="$PWD/BookclerkFiles" \
BOOKCLERK_UI_DIST="$PWD/ui/dist" \
./target/debug/bookclerkd
```

Windows/macOS hosts can build here but cannot execute the resulting ELF binaries
natively.

## Open in Cursor / VS Code

1. Install the **Dev Containers** extension (Cursor: same Marketplace id as VS Code).
2. Command Palette → **Dev Containers: Reopen in Container**.
3. Wait for the image build and `postCreateCommand` (`cargo fetch`, `ui` `npm ci`).

Definition: [`.devcontainer/`](../.devcontainer/).

## What is installed

| Piece | Notes |
| --- | --- |
| `rust:1.85-bookworm` | Shared with Cloud Agents; `rust-toolchain.toml` selects `stable` + rustfmt/clippy |
| `pkg-config` + `libssl-dev` | Fixes the local OpenSSL / `openssl-sys` failure mode |
| `libdbus-1-dev` + `xdg-utils` | Linux tray (`ksni` / zbus) and `xdg-open` for “Open Bookclerk” |
| `openssh-client` | SSH commit signing in the Dev Container |
| Node.js 22 | `ui/` Vite build |
| Cargo registry/git | Workspace `.cargo-home/` (`CARGO_HOME` on the bind mount) |
| Linux desktop + tray | Optional [`devcontainer.linux-desktop.json`](../.devcontainer/devcontainer.linux-desktop.json) |
| `PATH` | `target/debug` then `target/release` (via `remoteEnv`) |

## SSH commit signing (Dev Container)

Cloud Agents use Cursor’s HSM-backed SSH agent (`/run/host-services/ssh-auth.sock`
+ `cursor-git-ssh-keygen`). That Cloud-VM HSM is **not** available inside a
local Dev Container opened in the Cursor IDE.

Locally we use the same *security boundary*: the private key stays on the host
(or in 1Password / a hardware agent); the container only sees an `SSH_AUTH_SOCK`
and signs via `ssh-keygen -Y` through that agent.
[`git-signing.sh`](../.devcontainer/git-signing.sh) runs on `postStartCommand`.

| Source | How the agent reaches the container |
| --- | --- |
| Default `devcontainer.json` | Cursor/VS Code SSH-agent forwarder (`SSH_AUTH_SOCK` under `/tmp/…`) when it works |
| Linux desktop config | Bind-mounts host `${SSH_AUTH_SOCK}` → `/run/host-services/ssh-auth.sock` |
| macOS Docker Desktop (optional) | Magic host socket at `/run/host-services/ssh-auth.sock` — add an explicit mount if the IDE forwarder is empty |

Host prep:

```bash
eval "$(ssh-agent -s)"
ssh-add ~/.ssh/id_ed25519          # signing key loaded into the agent
export GIT_AUTHOR_NAME="Your Name"
export GIT_AUTHOR_EMAIL="you@example.com"
# Optional: pin which agent key signs (public only — never the private key)
export GIT_SSH_SIGNING_PUBKEY="$(ssh-add -L | head -1)"
```

| Host env var | Purpose |
| --- | --- |
| `GIT_AUTHOR_NAME` / `GIT_AUTHOR_EMAIL` | Commit author (git-native overrides) |
| `GIT_COMMITTER_NAME` / `GIT_COMMITTER_EMAIL` | Committer (falls back to author) |
| `GIT_SSH_SIGNING_PUBKEY` | Optional public key line (`key::…` added if missing); else first `ssh-add -L` key |

If the IDE forwarder shows “agent has no identities”, bind the host socket
explicitly (Linux example) in `devcontainer.json`:

```json
"mounts": [
  "source=${localEnv:SSH_AUTH_SOCK},target=/run/host-services/ssh-auth.sock,type=bind"
],
"remoteEnv": {
  "SSH_AUTH_SOCK": "/run/host-services/ssh-auth.sock"
}
```

Do not copy private keys into the container or pass them as env vars.

## Common commands (in the container)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p bookclerk-cli -p bookclerkd -p bookclerk-jail -p bookclerk-media-worker
bookclerk version
cd ui && npm run build
cargo dev
```

Mirrors [AGENTS.md](../AGENTS.md) / CI. For packaging the release image, see
[packaging.md](packaging.md) (`packaging/docker/Dockerfile`) — that path is
separate from this Dev Container.
