"""Path containment helpers for authoring tools and sparse workerd launchers."""

from __future__ import annotations

import os
from pathlib import Path


def resolve_under(root: Path | str, *parts: str | Path) -> Path:
    """Join ``parts`` under ``root`` and require the result stay inside ``root``.

    ``root`` is a trusted operation root (plugin directory, output directory,
    cache). It may be absolute or relative and may lexically contain ``..``
    before normalization — we abspath/normpath it first. Only ``parts`` are
    treated as untrusted relative suffixes (``..`` components rejected).

    Args:
        root: Trusted directory (user-selected plugin/output root, or cache).
        *parts: Relative segments to join, or one absolute path to validate
            under ``root``.

    Returns:
        Absolute path under ``root``.

    Raises:
        ValueError: When a part contains ``..`` / NUL or the result escapes
            ``root``.
    """
    root_s = os.path.abspath(os.path.normpath(os.fspath(root)))
    if "\0" in root_s:
        raise ValueError(f"root path contains NUL: {root}")
    root_path = Path(root_s)

    if len(parts) == 1 and os.path.isabs(os.fspath(parts[0])):
        resolved_s = os.path.abspath(os.path.normpath(os.fspath(parts[0])))
    else:
        for part in parts:
            part_s = os.fspath(part)
            if "\0" in part_s:
                raise ValueError(f"path contains NUL: {part}")
            # Component-level check: allow names like ``edition..2``.
            for seg in Path(part_s).parts:
                if seg == "..":
                    raise ValueError(f"path must not contain '..': {part}")
        joined = root_path.joinpath(*parts) if parts else root_path
        resolved_s = os.path.abspath(os.path.normpath(os.fspath(joined)))

    if "\0" in resolved_s:
        raise ValueError(f"path contains NUL: {resolved_s}")
    if os.path.commonpath([root_s, resolved_s]) != root_s:
        raise ValueError(f"path {resolved_s} escapes root {root_s}")
    if resolved_s != root_s and not resolved_s.startswith(root_s + os.sep):
        raise ValueError(f"path {resolved_s} escapes root {root_s}")
    return Path(resolved_s)


def cli_user_path(raw: str | Path) -> Path:
    """Accept a CLI path argument (any user-chosen location; no cwd jail).

    Authoring CLIs take absolute or relative paths without forcing containment
    under ``Path.cwd()``. Reject NUL only; callers apply
    :func:`resolve_under` for manifest-derived children beneath the opened
    plugin/output root.

    Args:
        raw: Operator-selected path (absolute, relative, or ``~``-prefixed).

    Returns:
        Normalized absolute path.

    Raises:
        ValueError: When the path contains an interior NUL.
    """
    p = Path(raw).expanduser()
    s = os.fspath(p)
    if "\0" in s:
        raise ValueError("path contains NUL")
    if not os.path.isabs(s):
        s = os.path.normpath(os.path.join(os.getcwd(), s))
    else:
        s = os.path.normpath(s)
    return Path(s)
