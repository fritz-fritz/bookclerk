"""Path containment helpers for authoring tools and sparse workerd launchers."""

from __future__ import annotations

import os
from pathlib import Path


def _is_under(root_s: str, resolved_s: str) -> bool:
    """True when ``resolved_s`` is ``root_s`` or a descendant (component-aware).

    Uses ``os.path.commonpath`` so children of the filesystem root (``/``)
    are accepted; a redundant ``root + sep`` prefix check would reject them.
    Also requires an explicit ``startswith`` prefix match so Default Setup
    path-injection queries see a barrier guard on the resolved path.
    """
    try:
        if os.path.commonpath([root_s, resolved_s]) != root_s:
            return False
    except ValueError:
        return False
    if resolved_s == root_s:
        return True
    prefix = root_s if root_s.endswith(os.sep) else root_s + os.sep
    return resolved_s.startswith(prefix)


def resolve_under(root: Path | str, *parts: str | Path) -> Path:
    """Join ``parts`` under ``root`` and require the result stay inside ``root``.

    ``root`` is a trusted operation root (plugin directory, output directory,
    cache). It may be absolute or relative and may lexically contain ``..``
    before normalization — we abspath/normpath it first. Only ``parts`` are
    treated as untrusted relative suffixes (``..`` components rejected).

    This helper is lexical (no symlink resolution). Callers that write or
    embed under a plugin tree must also use :func:`refuse_symlink_path` so a
    ``.bookclerk -> /outside`` link cannot redirect generated output.

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
    if not _is_under(root_s, resolved_s):
        raise ValueError(f"path {resolved_s} escapes root {root_s}")
    return Path(resolved_s)


def refuse_symlink_path(trusted_root: Path | str, path: Path | str) -> Path:
    """Require ``path`` under ``trusted_root`` with no symlink suffix components.

    Resolves the trusted root identity once (a symlinked operator/plugin root is
    allowed). Only components *below* that identity are inspected for symlinks.
    Missing final components are allowed (for create); intermediate missing
    parents raise.

    Args:
        trusted_root: Original operator/plugin root (not a promoted child).
        path: Candidate path previously produced by :func:`resolve_under`.

    Returns:
        The validated ``path`` as a :class:`~pathlib.Path`.

    Raises:
        ValueError: When a suffix component is a symlink or escapes ``trusted_root``.
        FileNotFoundError: When an intermediate parent is missing.
    """
    root_lex = Path(os.path.abspath(os.path.normpath(os.fspath(trusted_root))))
    candidate = Path(os.path.abspath(os.path.normpath(os.fspath(path))))
    if not _is_under(os.fspath(root_lex), os.fspath(candidate)):
        raise ValueError(f"path {candidate} escapes root {root_lex}")

    try:
        rel = candidate.relative_to(root_lex)
    except ValueError as err:
        raise ValueError(f"path {candidate} escapes root {root_lex}") from err

    # Allow the operator-selected root itself to be a symlink; constrain children.
    try:
        root = Path(os.path.realpath(root_lex))
    except OSError as err:
        raise ValueError(f"cannot resolve trusted root {root_lex}: {err}") from err

    cur = root
    parts = rel.parts
    for i, part in enumerate(parts):
        cur = cur / part
        if cur.is_symlink():
            raise ValueError(f"refusing symlink in path: {cur}")
        try:
            cur.lstat()
        except FileNotFoundError:
            if i < len(parts) - 1:
                raise FileNotFoundError(f"missing path component: {cur}") from None
            break
    return candidate


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


def copy_tree_no_symlinks(src: Path, dst: Path) -> None:
    """Copy a directory tree, refusing any symlink entries.

    Unlike ``shutil.copytree`` (which follows links by default), this never
    embeds outside bytes through ``modules/leak -> /outside``.

    Args:
        src: Source directory (must not itself be a symlink).
        dst: Destination directory to create.

    Raises:
        ValueError: When a symlink or unsupported file type is encountered.
        OSError: On filesystem failures.
    """
    import shutil

    if src.is_symlink():
        raise ValueError(f"refusing symlink package source: {src}")
    if not src.is_dir():
        raise ValueError(f"package source is not a directory: {src}")
    dst.mkdir(parents=True, exist_ok=True)
    for entry in sorted(src.iterdir(), key=lambda p: p.name):
        target = dst / entry.name
        if entry.is_symlink():
            raise ValueError(f"refusing symlink in package source: {entry}")
        if entry.is_dir():
            copy_tree_no_symlinks(entry, target)
        elif entry.is_file():
            shutil.copy2(entry, target, follow_symlinks=False)
        else:
            raise ValueError(f"refusing unsupported package source type: {entry}")
