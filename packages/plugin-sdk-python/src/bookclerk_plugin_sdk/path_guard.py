"""Path containment helpers for authoring tools and sparse workerd launchers."""

from __future__ import annotations

import os
from pathlib import Path


def resolve_under(root: Path | str, *parts: str | Path) -> Path:
    """Join ``parts`` under ``root`` and require the result stay inside ``root``.

    Rejects ``..`` segments. When a single absolute ``parts`` entry is given,
    validates that path is under ``root`` without joining.

    Args:
        root: Trusted directory.
        *parts: Relative segments to join, or one absolute path to validate.

    Returns:
        Absolute resolved path under ``root``.

    Raises:
        ValueError: When the path contains ``..`` or escapes ``root``.
    """
    root_path = Path(root).resolve()
    if len(parts) == 1 and Path(parts[0]).is_absolute():
        resolved = Path(parts[0]).resolve()
    else:
        for part in parts:
            if ".." in Path(part).parts:
                raise ValueError(f"path must not contain '..': {part}")
        resolved = root_path.joinpath(*parts).resolve() if parts else root_path
    try:
        resolved.relative_to(root_path)
    except ValueError as err:
        raise ValueError(f"path {resolved} escapes root {root_path}") from err
    root_s = os.fspath(root_path)
    resolved_s = os.fspath(resolved)
    if resolved_s != root_s and not resolved_s.startswith(root_s + os.sep):
        raise ValueError(f"path {resolved} escapes root {root_path}")
    return resolved
