"""Trusted-root checks for GitHub Actions file-command env paths."""

from __future__ import annotations

import os
import re
from typing import IO, Any

# Absolute POSIX path with safe segments only (no ``..``, no empties). Opening
# ``m.group(0)`` after ``fullmatch`` is the CodeQL-recognized path sanitizer.
_SAFE_ABS = re.compile(r"^/(?:[A-Za-z0-9._-]+/)*[A-Za-z0-9._-]+$")


def _allowlisted_abs(path_s: str, *, label: str) -> str:
    """Return ``fullmatch(...).group(0)`` for a normalized absolute path."""
    if not path_s or "\0" in path_s:
        raise SystemExit(f"{label} must be a non-empty path without NUL")
    if path_s != os.path.normpath(path_s):
        raise SystemExit(f"{label} must be a normalized absolute path: {path_s!r}")
    m = _SAFE_ABS.fullmatch(path_s)
    if m is None:
        raise SystemExit(f"{label} failed absolute path allowlist: {path_s!r}")
    return m.group(0)


def _runner_roots() -> list[str]:
    roots: list[str] = []
    for key in ("RUNNER_TEMP", "GITHUB_WORKSPACE"):
        raw = os.environ.get(key)
        if not raw:
            continue
        try:
            roots.append(_allowlisted_abs(raw, label=key))
        except SystemExit:
            continue
    return roots


def github_actions_file_path(path_s: str, *, label: str) -> str:
    """Resolve a GitHub Actions file-command path under a trusted runner root.

    ``GITHUB_OUTPUT`` / ``GITHUB_STEP_SUMMARY`` are runner-owned channels under
    ``RUNNER_TEMP`` (or occasionally ``GITHUB_WORKSPACE``).

    Returns:
        Allowlisted absolute path string under a trusted runner root.
    """
    path = _allowlisted_abs(path_s, label=label)
    roots = _runner_roots()
    if not roots:
        raise SystemExit(
            f"{label} is set but neither RUNNER_TEMP nor GITHUB_WORKSPACE is a "
            "usable absolute path; refusing to open a runner file-command path "
            "outside Actions"
        )
    for root in roots:
        if path == root or path.startswith(root + "/"):
            return path
    raise SystemExit(
        f"refusing {label} outside RUNNER_TEMP/GITHUB_WORKSPACE: {path}"
    )


def open_github_actions_append(path_s: str, *, label: str) -> IO[Any]:
    """Open a runner file-command path for append after allowlist + containment.

    Performs ``re.fullmatch`` and ``open(m.group(0), ...)`` in this function so
    CodeQL's ``py/path-injection`` query treats the open sink as sanitized.
    Returning an allowlisted string to the CLI and opening there does not clear
    taint across the helper boundary.
    """
    if not path_s or "\0" in path_s:
        raise SystemExit(f"{label} must be a non-empty path without NUL")
    if path_s != os.path.normpath(path_s):
        raise SystemExit(f"{label} must be a normalized absolute path: {path_s!r}")
    m = _SAFE_ABS.fullmatch(path_s)
    if m is None:
        raise SystemExit(f"{label} failed absolute path allowlist: {path_s!r}")
    path = m.group(0)

    roots = _runner_roots()
    if not roots:
        raise SystemExit(
            f"{label} is set but neither RUNNER_TEMP nor GITHUB_WORKSPACE is a "
            "usable absolute path; refusing to open a runner file-command path "
            "outside Actions"
        )
    if not any(path == root or path.startswith(root + "/") for root in roots):
        raise SystemExit(
            f"refusing {label} outside RUNNER_TEMP/GITHUB_WORKSPACE: {path}"
        )

    # Matched group is the CodeQL-recognized sanitized path.
    return open(m.group(0), "a", encoding="utf-8")
