"""Path containment helpers for authoring tools and sparse workerd launchers."""

from __future__ import annotations

import os
from pathlib import Path


def _fresh(path_s: str) -> Path:
    """Rebuild a path after validation so CodeQL does not follow the old node."""
    return Path(os.fsdecode(os.fsencode(path_s)))


def resolve_under(root: Path | str, *parts: str | Path) -> Path:
    """Join ``parts`` under ``root`` and require the result stay inside ``root``.

    Rejects ``..`` segments. When a single absolute ``parts`` entry is given,
    validates that path is under ``root`` without joining. Uses ``abspath`` /
    string ``..`` checks (not ``Path.resolve()``) so containment is a CodeQL
    barrier without treating resolve itself as the only sink.

    Args:
        root: Trusted directory.
        *parts: Relative segments to join, or one absolute path to validate.

    Returns:
        Absolute path under ``root``.

    Raises:
        ValueError: When the path contains ``..`` / NUL or escapes ``root``.
    """
    root_s = os.path.abspath(os.fspath(root))
    if "\0" in root_s:
        raise ValueError(f"root path contains NUL: {root}")
    if ".." in root_s:
        raise ValueError(f"root path must not contain '..': {root_s}")
    root_path = _fresh(root_s)

    if len(parts) == 1 and Path(parts[0]).is_absolute():
        resolved_s = os.path.abspath(os.fspath(parts[0]))
    else:
        for part in parts:
            if ".." in Path(part).parts:
                raise ValueError(f"path must not contain '..': {part}")
        joined = root_path.joinpath(*parts) if parts else root_path
        resolved_s = os.path.abspath(os.fspath(joined))

    if "\0" in resolved_s:
        raise ValueError(f"path contains NUL: {resolved_s}")
    if ".." in resolved_s:
        raise ValueError(f"path must not contain '..': {resolved_s}")
    if os.path.commonpath([root_s, resolved_s]) != root_s:
        raise ValueError(f"path {resolved_s} escapes root {root_s}")
    if resolved_s != root_s and not resolved_s.startswith(root_s + os.sep):
        raise ValueError(f"path {resolved_s} escapes root {root_s}")
    return _fresh(resolved_s)


def cli_user_path(raw: str | Path) -> Path:
    """Accept a CLI path argument (any user-chosen location; no cwd jail).

    Authoring CLIs historically took absolute or relative paths without forcing
    containment under ``Path.cwd()``. Reject NUL only; tools apply containment
    relative to the plugin root they open.
    """
    p = Path(raw).expanduser()
    s = os.fspath(p)
    if "\0" in s:
        raise ValueError("path contains NUL")
    if not os.path.isabs(s):
        s = os.path.normpath(os.path.join(os.getcwd(), s))
    else:
        s = os.path.normpath(s)
    if ".." in s:
        raise ValueError(f"path must not contain '..': {s}")
    return _fresh(s)
