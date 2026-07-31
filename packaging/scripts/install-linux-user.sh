#!/usr/bin/env bash
# Install Bookclerk for a single interactive user:
#   - system account `bookclerk` for the daemon process identity
#   - setuid-root wrapper so a user systemd unit can drop to bookclerk
#   - ~/Audiobooks ACL so bookclerk can write; CAP_CHOWN (kept across drop)
#     chowns files back to the installing user
#   - user unit + optional tray
#
# Usage (from a built tree, as the installing user with sudo):
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
  echo "refuse to install for root; run as the desktop user (with sudo)" >&2
  exit 1
fi
INSTALL_HOME="$(getent passwd "${INSTALL_USER}" | cut -d: -f6)"
if [[ -z "${INSTALL_HOME}" || ! -d "${INSTALL_HOME}" ]]; then
  echo "cannot resolve home for ${INSTALL_USER}" >&2
  exit 1
fi

echo "==> ensuring system user bookclerk"
if ! getent passwd bookclerk >/dev/null; then
  sudo useradd --system --home /var/lib/bookclerk --shell /usr/sbin/nologin bookclerk
fi
sudo mkdir -p /var/lib/bookclerk
sudo chown -R bookclerk:bookclerk /var/lib/bookclerk

FILES_DIR="${INSTALL_HOME}/.local/share/bookclerk"
AUDIOBOOKS="${INSTALL_HOME}/Audiobooks"
UNIT_DIR="${INSTALL_HOME}/.config/systemd/user"
BIN_DST=/usr/local/bin/bookclerkd

echo "==> installing bookclerkd (setuid-root → drops to bookclerk)"
sudo install -o root -g root -m 4755 "${BIN_SRC}" "${BIN_DST}"
# User unit starts this with euid root (setuid); bookclerkd then setuid+CAP_CHOWN
# to bookclerk. Without this helper, the user unit cannot drop or chown.

echo "==> preparing files dir + Audiobooks for ${INSTALL_USER}"
mkdir -p "${FILES_DIR}" "${AUDIOBOOKS}" "${UNIT_DIR}"
# bookclerk must write here after drop; CAP_CHOWN chowns results to owner.
if command -v setfacl >/dev/null 2>&1; then
  sudo setfacl -m "u:bookclerk:rwx" "${AUDIOBOOKS}"
  sudo setfacl -d -m "u:bookclerk:rwx" "${AUDIOBOOKS}"
  sudo setfacl -m "u:bookclerk:rwx" "${FILES_DIR}"
  sudo setfacl -d -m "u:bookclerk:rwx" "${FILES_DIR}"
else
  echo "warning: setfacl missing; grant bookclerk write on ${AUDIOBOOKS} manually" >&2
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
UNIT_SRC="${SCRIPT_DIR}/../systemd/bookclerkd.user.service"
if [[ ! -f "${UNIT_SRC}" ]]; then
  echo "missing ${UNIT_SRC}" >&2
  exit 1
fi
install -m 644 "${UNIT_SRC}" "${UNIT_DIR}/bookclerkd.service"

# Ensure config exists with owner + identity defaults if absent.
CFG="${FILES_DIR}/config.toml"
if [[ ! -f "${CFG}" ]]; then
  cat >"${CFG}" <<EOF
[daemon.identity]
service_user = "bookclerk"
service_group = "bookclerk"
drop_privileges = true
allow_interactive_user = false

[output.local]
enabled = true
root = "@user/Audiobooks"
owner_user = "${INSTALL_USER}"
EOF
fi

echo "==> enabling user service"
systemctl --user daemon-reload
systemctl --user enable --now bookclerkd.service

echo "installed."
echo "  daemon: ${BIN_DST} (setuid-root, drops to bookclerk)"
echo "  files:  ${FILES_DIR}"
echo "  media:  ${AUDIOBOOKS} (owner ${INSTALL_USER}; ACL for bookclerk)"
echo "  unit:   systemctl --user status bookclerkd"
echo "Set BOOKCLERK_AUTH_PASSWORD in the user unit Environment= for production."
