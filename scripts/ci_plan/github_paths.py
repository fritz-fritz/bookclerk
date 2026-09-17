"""Trusted-root checks for GitHub Actions file-command env paths."""

from __future__ import annotations

import os
from typing import IO, Any


def _runner_roots() -> list[str]:
    """Real paths of runner-owned roots (``RUNNER_TEMP``, ``GITHUB_WORKSPACE``)."""
    roots: list[str] = []
    for key in ("RUNNER_TEMP", "GITHUB_WORKSPACE"):
        raw = os.environ.get(key)
        if not raw or "\0" in raw:
            continue
        roots.append(os.path.realpath(raw))
    return roots


def _is_under(path: str, root: str) -> bool:
    """True when ``path`` is ``root`` or a descendant (same-drive ``commonpath``)."""
    try:
        return os.path.commonpath([path, root]) == root
    except ValueError:
        return False


def github_actions_file_path(path_s: str, *, label: str) -> str:
    """Resolve a GitHub Actions file-command path under a trusted runner root.

    ``GITHUB_OUTPUT`` / ``GITHUB_STEP_SUMMARY`` are runner-owned channels.
    GitHub documents that workflow ``env:`` cannot overwrite default ``GITHUB_*``
    / ``RUNNER_*`` variables. This helper preserves the path (spaces, Unicode,
    Windows drive letters) and requires canonical containment under
    ``RUNNER_TEMP`` or ``GITHUB_WORKSPACE`` via ``realpath`` + ``commonpath``.

    Static analysis: the return value is a ``path-injection`` barrier in
    ``.github/codeql/extensions/bookclerk-python`` (see that model pack).

    Args:
        path_s: Raw env value for the file command.
        label: Env var name for error messages.

    Returns:
        Canonical absolute path under a trusted runner root.

    Raises:
        SystemExit: When the path is empty/NUL, runner roots are unset, or the
            resolved path escapes every trusted root.
    """
    if not path_s or "\0" in path_s:
        raise SystemExit(f"{label} must be a non-empty path without NUL")

    roots = _runner_roots()
    if not roots:
        raise SystemExit(
            f"{label} is set but neither RUNNER_TEMP nor GITHUB_WORKSPACE is a "
            "usable path; refusing to open a runner file-command path outside "
            "Actions"
        )

    resolved = os.path.realpath(path_s)
    for root in roots:
        if _is_under(resolved, root):
            return resolved

    raise SystemExit(
        f"refusing {label} outside RUNNER_TEMP/GITHUB_WORKSPACE: {resolved}"
    )


def open_github_actions_append(path_s: str, *, label: str) -> IO[Any]:
    """Open a runner file-command path for append after semantic containment.

    Validates with :func:`github_actions_file_path` (realpath under
    ``RUNNER_TEMP`` / ``GITHUB_WORKSPACE``), then opens the canonical path.
    Modeled as a ``path-injection`` barrier in the bookclerk-python CodeQL
    extension pack so callers and this open sink are not treated as tainted.
    """
    path = github_actions_file_path(path_s, label=label)
    return open(path, "a", encoding="utf-8")
