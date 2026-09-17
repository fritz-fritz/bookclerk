"""Trusted-root checks for GitHub Actions file-command env paths."""

from __future__ import annotations

import os
from pathlib import Path


def github_actions_file_path(path_s: str, *, label: str) -> Path:
    """Resolve a GitHub Actions file-command path under a trusted runner root.

    ``GITHUB_OUTPUT`` / ``GITHUB_STEP_SUMMARY`` are runner-owned channels. Their
    values are absolute paths under ``RUNNER_TEMP`` (or occasionally
    ``GITHUB_WORKSPACE``). Containment under those roots is the real guard —
    not a cwd ``startswith`` check (``Path.resolve()`` is always absolute, so
    ``under_cwd or is_absolute()`` is a tautology).

    Args:
        path_s: Raw env value for the file command.
        label: Env var name for error messages.

    Returns:
        Path rebuilt under the matching trusted root.

    Raises:
        SystemExit: When the path is empty/NUL, runner roots are unset, or the
            resolved path escapes every trusted root.
    """
    if not path_s or "\0" in path_s:
        raise SystemExit(f"{label} must be a non-empty path without NUL")

    roots: list[Path] = []
    for key in ("RUNNER_TEMP", "GITHUB_WORKSPACE"):
        raw = os.environ.get(key)
        if not raw or "\0" in raw:
            continue
        roots.append(Path(raw).expanduser().resolve())
    if not roots:
        raise SystemExit(
            f"{label} is set but neither RUNNER_TEMP nor GITHUB_WORKSPACE is; "
            "refusing to open a runner file-command path outside Actions"
        )

    resolved = Path(path_s).expanduser().resolve()
    for root in roots:
        try:
            rel = resolved.relative_to(root)
        except ValueError:
            continue
        # Rebuild under the trusted root so the open sink is not a raw join of
        # the env string.
        return root.joinpath(*rel.parts)

    raise SystemExit(
        f"refusing {label} outside RUNNER_TEMP/GITHUB_WORKSPACE: {resolved}"
    )
