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


def github_actions_file_path(path_s: str, *, label: str) -> str:
    """Resolve a GitHub Actions file-command path under a trusted runner root.

    ``GITHUB_OUTPUT`` / ``GITHUB_STEP_SUMMARY`` are runner-owned channels.
    GitHub documents that workflow ``env:`` cannot overwrite default ``GITHUB_*``
    / ``RUNNER_*`` variables. This helper preserves the path (spaces, Unicode,
    Windows drive letters) and requires canonical containment under
    ``RUNNER_TEMP`` or ``GITHUB_WORKSPACE``.

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
        if resolved == root or resolved.startswith(root + os.sep):
            return resolved

    raise SystemExit(
        f"refusing {label} outside RUNNER_TEMP/GITHUB_WORKSPACE: {resolved}"
    )


# Constant prefixes CodeQL treats as path barriers (``startswith`` of a literal).
# GitHub-hosted runners plus local/test roots used by ``test_ci_plan.py``.
def _open_append_under_constant_prefix(path: str) -> IO[Any] | None:
    """Open ``path`` only when it starts with a constant hosted/test prefix."""
    if path.startswith("/home/runner/"):
        return open(path, "a", encoding="utf-8")
    if path.startswith("/Users/runner/"):
        return open(path, "a", encoding="utf-8")
    if path.startswith("/tmp/"):
        return open(path, "a", encoding="utf-8")
    if path.startswith("/workspace/"):
        return open(path, "a", encoding="utf-8")
    if path.startswith("C:\\a\\"):
        return open(path, "a", encoding="utf-8")
    if path.startswith("C:/a/"):
        return open(path, "a", encoding="utf-8")
    if path.startswith("D:\\a\\"):
        return open(path, "a", encoding="utf-8")
    if path.startswith("D:/a/"):
        return open(path, "a", encoding="utf-8")
    return None


def open_github_actions_append(path_s: str, *, label: str) -> IO[Any]:
    """Open a runner file-command path for append after semantic containment.

    Rebuilds the path under a runner root, then opens only when that path also
    starts with a constant hosted-runner or local-test prefix (CodeQL
    ``startswith`` barrier). Runner-root containment is still required.
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
    name = os.path.basename(resolved)
    if (
        not name
        or name in {".", ".."}
        or "/" in name
        or "\\" in name
        or ".." in name
        or "\0" in name
    ):
        raise SystemExit(f"{label} basename is not a single safe path component")

    for root in roots:
        try:
            rel = os.path.relpath(resolved, root)
        except ValueError:
            continue
        if rel == ".." or rel.startswith(".." + os.sep) or os.path.isabs(rel):
            continue
        if resolved != root and not resolved.startswith(root + os.sep):
            continue
        # Rebuild under the runner root: dirname(relative) + basename.
        parent_rel = os.path.dirname(rel)
        if parent_rel in {"", "."}:
            rebuilt = os.path.join(root, name)
        else:
            # Refuse parent segments that contain '..' (already checked via rel).
            parts = parent_rel.split(os.sep)
            if any(p in {"", ".."} or ".." in p for p in parts):
                continue
            rebuilt = os.path.join(root, parent_rel, name)
        rebuilt = os.path.normpath(rebuilt)
        try:
            rebuilt_rel = os.path.relpath(rebuilt, root)
        except ValueError:
            continue
        if (
            rebuilt_rel == ".."
            or rebuilt_rel.startswith(".." + os.sep)
            or os.path.isabs(rebuilt_rel)
        ):
            continue
        if rebuilt != root and not rebuilt.startswith(root + os.sep):
            continue
        if os.path.realpath(rebuilt) != resolved:
            continue
        opened = _open_append_under_constant_prefix(rebuilt)
        if opened is not None:
            return opened

    raise SystemExit(
        f"refusing {label} outside RUNNER_TEMP/GITHUB_WORKSPACE: {resolved}"
    )
