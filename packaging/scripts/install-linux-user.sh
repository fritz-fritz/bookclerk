#!/usr/bin/env bash
# Install Bookclerk in *session mode* for a single interactive user:
#   - plain (non-setuid) bookclerkd binary
#   - systemd --user unit running as the login user (tray-friendly)
#   - ~/Audiobooks + XDG files dir owned by the user (natural ownership)
#
# This does NOT use a setuid-root helper. Session mode runs as you; secrets
# live in your session. For hardened isolation (dedicated bookclerk uid +
# ambient CAP_CHOWN), use the system unit — see docs/operations.md.
#
# Usage (from a built tree, as the installing user; sudo only for /usr/local):
#   ./packaging/scripts/install-linux-user.sh [/path/to/bookclerkd]
set -euo pipefail

BIN_SRC="${1:-}"
if [[ -z "${BIN_SRC}" ]]; then
  if [[ -x ./target/release/bookclerkd ]]; then
    BIN_SRC=./target/release/bookclerkd
  elif [[ -x ./target/debug/bookclerkd ]]; then
    BIN_SRC=./target/debug/bookclerkd
  else
    echo "usage: $0 /path/to/bookclerkd" >&2
    exit 1
  fi
fi

INSTALL_USER="${SUDO_USER:-${USER}}"
if [[ "${INSTALL_USER}" == "root" ]]; then
  echo "refuse to install for root; run as the desktop user" >&2
  exit 1
fi
INSTALL_HOME="$(getent passwd "${INSTALL_USER}" | cut -d: -f6)"
if [[ -z "${INSTALL_HOME}" || ! -d "${INSTALL_HOME}" ]]; then
  echo "cannot resolve home for ${INSTALL_USER}" >&2
  exit 1
fi

FILES_DIR="${INSTALL_HOME}/.local/share/bookclerk"
AUDIOBOOKS="${INSTALL_HOME}/Audiobooks"
UNIT_DIR="${INSTALL_HOME}/.config/systemd/user"
BIN_DST=/usr/local/bin/bookclerkd

echo "==> installing bookclerkd (session mode — not setuid)"
# Mode 755 root:root — no setuid bit. User unit runs as ${INSTALL_USER}.
sudo install -o root -g root -m 755 "${BIN_SRC}" "${BIN_DST}"

echo "==> preparing files dir + Audiobooks for ${INSTALL_USER}"
mkdir -p "${FILES_DIR}" "${AUDIOBOOKS}" "${UNIT_DIR}"
# Session mode: the login user owns everything; no bookclerk ACL required.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
UNIT_SRC="${SCRIPT_DIR}/../systemd/bookclerkd.user.service"
if [[ ! -f "${UNIT_SRC}" ]]; then
  echo "missing ${UNIT_SRC}" >&2
  exit 1
fi
install -m 644 "${UNIT_SRC}" "${UNIT_DIR}/bookclerkd.service"

CFG="${FILES_DIR}/config.toml"
if [[ ! -f "${CFG}" ]]; then
  cat >"${CFG}" <<EOF
# Session mode: daemon runs as the login user (systemd --user).
# For dedicated bookclerk uid + ambient CAP_CHOWN, see docs/operations.md
# and packaging/systemd/bookclerkd.service.
[daemon.identity]
service_user = "bookclerk"
service_group = "bookclerk"
drop_privileges = true
allow_interactive_user = true

[output.local]
enabled = true
root = "@user/Audiobooks"
owner_user = "${INSTALL_USER}"
EOF
fi

echo "==> enabling user service"
systemctl --user daemon-reload
systemctl --user enable --now bookclerkd.service

echo "installed (session mode)."
echo "  daemon: ${BIN_DST} (mode 755, runs as ${INSTALL_USER})"
echo "  files:  ${FILES_DIR}"
echo "  media:  ${AUDIOBOOKS} (owned by ${INSTALL_USER})"
echo "  unit:   systemctl --user status bookclerkd"
echo "Set BOOKCLERK_AUTH_PASSWORD in the user unit Environment= for production."
echo "Hardened system install: see packaging/systemd/bookclerkd.service"
