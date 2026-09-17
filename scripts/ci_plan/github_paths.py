"""Trusted-root checks for GitHub Actions file-command env paths."""

from __future__ import annotations

import os
import re

# Absolute POSIX path with safe segments only (no ``..``, no empties). Matched
# groups are the CodeQL-recognized sanitized form used for open sinks.
_SAFE_ABS = re.compile(r"^/(?:[A-Za-z0-9._-]+/)*[A-Za-z0-9._-]+$")


def _allowlisted_abs(path_s: str, *, label: str) -> str:
    """Return an absolute path string after a regex allowlist sanitizer.

    Rejects NUL, relative paths, and any spelling that ``os.path.normpath``
    would rewrite (so ``..`` / ``.`` / duplicate slashes cannot slip through).
    Does not call ``Path.resolve()`` — that is itself a CodeQL path-expression
    sink on env-derived strings.
    """
    if not path_s or "\0" in path_s:
        raise SystemExit(f"{label} must be a non-empty path without NUL")
    if path_s != os.path.normpath(path_s):
        raise SystemExit(f"{label} must be a normalized absolute path: {path_s!r}")
    m = _SAFE_ABS.fullmatch(path_s)
    if m is None:
        raise SystemExit(f"{label} failed absolute path allowlist: {path_s!r}")
    return m.group(0)


def github_actions_file_path(path_s: str, *, label: str) -> str:
    """Resolve a GitHub Actions file-command path under a trusted runner root.

    ``GITHUB_OUTPUT`` / ``GITHUB_STEP_SUMMARY`` are runner-owned channels under
    ``RUNNER_TEMP`` (or occasionally ``GITHUB_WORKSPACE``). Containment under
    those roots is the real guard — not a cwd check (``Path.resolve()`` is
    always absolute, so ``under_cwd or is_absolute()`` is a tautology).

    Args:
        path_s: Raw env value for the file command.
        label: Env var name for error messages.

    Returns:
        Allowlisted absolute path string under a trusted runner root.

    Raises:
        SystemExit: When the path fails the allowlist, runner roots are unset,
            or the path escapes every trusted root.
    """
    path = _allowlisted_abs(path_s, label=label)

    roots: list[str] = []
    for key in ("RUNNER_TEMP", "GITHUB_WORKSPACE"):
        raw = os.environ.get(key)
        if not raw:
            continue
        try:
            roots.append(_allowlisted_abs(raw, label=key))
        except SystemExit:
            continue
    if not roots:
        raise SystemExit(
            f"{label} is set but neither RUNNER_TEMP nor GITHUB_WORKSPACE is a "
            "usable absolute path; refusing to open a runner file-command path "
            "outside Actions"
        )

    for root in roots:
        if path == root or path.startswith(root + "/"):
            # path and root are both allowlisted matched groups.
            return path

    raise SystemExit(
        f"refusing {label} outside RUNNER_TEMP/GITHUB_WORKSPACE: {path}"
    )
