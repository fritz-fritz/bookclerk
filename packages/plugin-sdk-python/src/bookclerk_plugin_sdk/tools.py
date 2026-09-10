"""check / fmt / package / sync-embed — mirrors Rust/TS author tools.

Validates ``plugin.toml``, formats manifests, vendors workerd embeds, and packs
release archives. Workerd authors:

- ``from bookclerk_plugin_sdk.workerd import BookclerkPlugin, js``
  (``bookclerk-workerd`` injects that module — no relative filepath embed)

See ``docs/plugins.md`` for manifest fields and runtime requirements.
"""

from __future__ import annotations

import hashlib
import os
import re
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any

# Required for local bookclerk-workerd / Pyodide without pywrangler.
PYTHON_WORKERD_FLAGS = ("python_workers", "disable_python_external_sdk")
"""Compatibility flags required for Python Workers under bookclerk-workerd."""

LOGO_EXTENSIONS = (".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".ico")
"""Allowed file extensions for embedded ``plugin.toml`` logo paths."""


def validate_logo(raw: str) -> tuple[str, str]:
    r"""Classify and validate a ``plugin.toml`` logo value.

    Mirrors Rust ``validate_logo``. Accepts absolute ``http``/``https`` URLs or
    relative image paths under the plugin root.

    Args:
        raw: Raw ``logo`` string from the manifest.

    Returns:
        A ``(\"remote\"|\"embedded\", value)`` pair with the validated URL or
        normalized relative path.

    Raises:
        ValueError: If the logo is empty, uses a bad scheme, includes userinfo,
            or is an unsafe / non-image embedded path.
    """
    from urllib.parse import urlparse

    trimmed = raw.strip()
    if not trimmed:
        raise ValueError("plugin.toml: `logo` must not be empty (omit the key instead)")
    if "\0" in trimmed:
        raise ValueError("plugin.toml: `logo` must not contain NUL")
    # Absolute URLs (any scheme) via urllib — only http/https allowed.
    # Relative image paths have no scheme and use path validation.
    parsed = urlparse(trimmed)
    if parsed.scheme:
        return _validate_parsed_url(parsed, trimmed)
    return _validate_embedded_path(trimmed)


def _validate_parsed_url(parsed: Any, original: str) -> tuple[str, str]:
    scheme = parsed.scheme.lower()
    if scheme not in {"http", "https"}:
        raise ValueError(
            f"plugin.toml: `logo` URL must use http:// or https:// (got scheme `{scheme}`)"
        )
    # Match Rust `url::Url`: reject non-empty username, or any password
    # (including empty). Empty username alone (`https://@host`) is allowed.
    username = parsed.username or ""
    if username or parsed.password is not None:
        raise ValueError(
            "plugin.toml: `logo` URL must not include userinfo (user:pass@host)"
        )
    host = (parsed.hostname or "").strip()
    if not host or host in {".", ".."}:
        raise ValueError("plugin.toml: `logo` URL is missing a host")
    return ("remote", original)


def _validate_embedded_path(trimmed: str) -> tuple[str, str]:
    path = trimmed.replace("\\", "/")
    if path.startswith("/") or path.startswith("~"):
        raise ValueError(
            "plugin.toml: embedded `logo` must be a relative path under the plugin root"
        )
    if len(path) >= 2 and path[1] == ":":
        raise ValueError(
            "plugin.toml: embedded `logo` must be a relative path (no drive letter)"
        )
    if path.startswith("//"):
        raise ValueError("plugin.toml: embedded `logo` must be a relative path (no UNC)")
    segments: list[str] = []
    for seg in path.split("/"):
        if not seg or seg == ".":
            continue
        if seg == "..":
            raise ValueError("plugin.toml: embedded `logo` must not contain `..` segments")
        segments.append(seg)
    if not segments:
        raise ValueError("plugin.toml: embedded `logo` path is empty after normalization")
    normalized = "/".join(segments)
    lower_path = normalized.lower()
    if not any(lower_path.endswith(ext) for ext in LOGO_EXTENSIONS):
        raise ValueError(
            "plugin.toml: embedded `logo` must end with one of " + ", ".join(LOGO_EXTENSIONS)
        )
    return ("embedded", normalized)


def validate_plugin_id(id: str) -> None:
    """Validate a plugin id against the strict ``[a-z0-9_]{2,32}`` grammar.

    Mirrors Rust ``validate_plugin_id``. Ids are globally unique across plugins.
    Invalid characters are rejected — never rewritten — so ``a/b`` and ``a_b``
    cannot collide. Leading/trailing whitespace is rejected (non-lossy), not
    stripped.

    Args:
        id: Candidate plugin id from ``plugin.toml``.

    Raises:
        ValueError: If the id fails length, charset, or underscore rules.
    """
    if id != id.strip():
        raise ValueError(
            f"plugin id `{id}` must not have leading or trailing whitespace"
        )
    if len(id) < 2 or len(id) > 32:
        raise ValueError(f"plugin id `{id}` must be 2–32 characters")
    if not id.isascii() or not all(
        c.islower() or c.isdigit() or c == "_" for c in id
    ):
        raise ValueError(
            f"plugin id `{id}` must be lowercase ascii letters, digits, or `_`"
        )
    if id.startswith("_") or id.endswith("_") or "__" in id:
        raise ValueError(
            f"plugin id `{id}` must not start/end with `_` or contain `__`"
        )


def validate_manifest(m: dict[str, Any]) -> None:
    """Validate a parsed ``plugin.toml`` mapping.

    Args:
        m: Manifest dictionary (typically from ``tomllib.loads``).

    Raises:
        ValueError: If required fields, runtime tables, or network capabilities
            are missing or inconsistent.
    """
    if not str(m.get("id", "")).strip():
        raise ValueError("plugin.toml: `id` is required")
    try:
        # Validate the raw id (non-lossy): do not strip before grammar checks.
        validate_plugin_id(str(m["id"]))
    except ValueError as exc:
        raise ValueError(f"plugin.toml: {exc}") from exc
    if m.get("api_version") != 3:
        raise ValueError("plugin.toml: `api_version` must be 3")
    if m.get("logo") is not None:
        validate_logo(str(m["logo"]))
    _validate_surface(m)
    runtime = m.get("runtime") or "native"
    net = (m.get("capabilities") or {}).get("network") or {}
    if runtime == "native":
        if not str(m.get("command") or "").strip():
            raise ValueError('plugin.toml: `command` is required when runtime = "native"')
        domains = net.get("domains") or []
        if domains:
            raise ValueError(
                'plugin.toml: capabilities.network.domains is only valid for runtime = "workerd" '
                "(native outbound is coarse jail networking with no hostname filter — omit domains)"
            )
    elif runtime == "workerd":
        w = m.get("workerd")
        if not isinstance(w, dict):
            raise ValueError('plugin.toml: `[workerd]` is required when runtime = "workerd"')
        if not str(w.get("compatibility_date") or "").strip():
            raise ValueError("plugin.toml: workerd.compatibility_date is required")
        if not str(w.get("main_module") or "").strip():
            raise ValueError("plugin.toml: workerd.main_module is required")
        if net.get("mode") == "outbound" and not net.get("domains"):
            raise ValueError(
                'plugin.toml: capabilities.network.domains is required when runtime = "workerd" '
                'and mode = "outbound"'
            )
    else:
        raise ValueError(f"plugin.toml: unknown runtime {runtime}")


ENTRYPOINT_NAMES: tuple[str, ...] = (
    "storefront",
    "storage",
    "databaseAdapter",
    "remoteLibrary",
    "cli",
    "oidc",
)
"""Entrypoint wire names accepted in ``entrypoints``."""

_DATABASE_BINDING_RE = re.compile(r"^[A-Z][A-Z0-9_]*$")


def _validate_surface(m: dict[str, Any]) -> None:
    """Validate ``entrypoints`` / triggers / bindings (mirrors Rust ``validate``)."""
    entrypoints = [str(e) for e in (m.get("entrypoints") or [])]
    seen: set[str] = set()
    for entrypoint in entrypoints:
        if entrypoint not in ENTRYPOINT_NAMES:
            raise ValueError(
                f"plugin.toml: unknown entrypoint `{entrypoint}` "
                f"(expected one of {', '.join(ENTRYPOINT_NAMES)})"
            )
        if entrypoint in seen:
            raise ValueError(f"plugin.toml: entrypoints entry `{entrypoint}` is duplicated")
        seen.add(entrypoint)
    events = m.get("events") or {}
    consumers = list(events.get("consumers") or [])
    jobs = list((m.get("triggers") or {}).get("jobs") or [])
    if not entrypoints and not consumers and not jobs:
        raise ValueError(
            "plugin.toml: declare at least one of `entrypoints`, `[[events.consumers]]`, "
            "or `[triggers].jobs`"
        )
    for consumer in consumers:
        if not str(consumer.get("type") or "").strip():
            raise ValueError("plugin.toml: [[events.consumers]] `type` is required")
    for producer in events.get("producers") or []:
        if not str(producer.get("type") or "").strip():
            raise ValueError("plugin.toml: [[events.producers]] `type` is required")
    if m.get("cli") is not None and "cli" not in seen:
        raise ValueError('plugin.toml: `[cli]` requires `"cli"` in `entrypoints`')
    if ((m.get("oidc") or {}).get("clients") or []) and "oidc" not in seen:
        raise ValueError('plugin.toml: `[[oidc.clients]]` requires `"oidc"` in `entrypoints`')
    bindings: set[str] = set()
    for db in m.get("databases") or []:
        name = str(db.get("binding") or "")
        if not _DATABASE_BINDING_RE.match(name) or len(name) > 32:
            raise ValueError(
                f"plugin.toml: [[databases]] binding `{name}` must be `[A-Z][A-Z0-9_]*` "
                "and at most 32 chars"
            )
        if name in bindings:
            raise ValueError(f"plugin.toml: [[databases]] binding `{name}` is duplicated")
        bindings.add(name)


def _workerd_modules_dir(plugin_dir: Path, m: dict[str, Any]) -> Path:
    w = m["workerd"]
    return plugin_dir / (w.get("modules_dir") or "modules")


def _is_python_workerd(m: dict[str, Any]) -> bool:
    w = m.get("workerd") or {}
    return str(w.get("main_module") or "").lower().endswith(".py")


def _ensure_python_flags(flags: list[Any] | None) -> list[str]:
    out = [str(f) for f in (flags or [])]
    for required in PYTHON_WORKERD_FLAGS:
        if required not in out:
            out.append(required)
    return out


def _sdk_workerd_embed_src() -> Path:
    return Path(__file__).resolve().parent / "workerd.py"


def check_plugin(plugin_dir: Path) -> str:
    """Validate a plugin directory and its ``plugin.toml``.

    Args:
        plugin_dir: Path to the plugin root containing ``plugin.toml``.

    Returns:
        A short ``ok id=... entrypoints=... runtime=...`` status string.

    Raises:
        ValueError: If the manifest or Python workerd sources are invalid.
        FileNotFoundError: If required logo, modules, or native binaries are missing.
        OSError: If ``plugin.toml`` cannot be read.

    Examples:
        >>> # print(check_plugin(Path("./my-plugin")))
        >>> # ok id=echo entrypoints=storefront runtime=workerd
    """
    text = (plugin_dir / "plugin.toml").read_text(encoding="utf-8")
    m = tomllib.loads(text)
    validate_manifest(m)
    if m.get("logo") is not None:
        kind, value = validate_logo(str(m["logo"]))
        if kind == "embedded":
            logo_path = plugin_dir / value
            if not logo_path.is_file():
                raise FileNotFoundError(f"embedded logo missing: {logo_path}")
    runtime = m.get("runtime") or "native"
    if runtime == "workerd":
        w = m["workerd"]
        modules_dir = _workerd_modules_dir(plugin_dir, m)
        if not modules_dir.is_dir():
            raise FileNotFoundError(f"workerd modules_dir missing: {modules_dir}")
        main = modules_dir / w["main_module"]
        if not main.is_file():
            raise FileNotFoundError(f"workerd main_module missing: {main}")
        if _is_python_workerd(m):
            src = main.read_text(encoding="utf-8")
            if "bookclerk_plugin_sdk.workerd" not in src and "BookclerkPlugin" not in src:
                raise ValueError(
                    f"{main.name}: import BookclerkPlugin from "
                    "bookclerk_plugin_sdk.workerd "
                    '(e.g. `from bookclerk_plugin_sdk.workerd import BookclerkPlugin, js`)'
                )
            if "WorkerEntrypoint" in src and "BookclerkPlugin" not in src:
                raise ValueError(
                    f"{main.name}: subclass BookclerkPlugin from "
                    "bookclerk_plugin_sdk.workerd, not bare WorkerEntrypoint"
                )
            flags = list(w.get("compatibility_flags") or [])
            missing = [f for f in PYTHON_WORKERD_FLAGS if f not in flags]
            if missing:
                raise ValueError(
                    "plugin.toml: workerd.compatibility_flags must include "
                    f"{', '.join(PYTHON_WORKERD_FLAGS)} for Python Workers "
                    f"(missing: {', '.join(missing)}). "
                    "Run: bookclerk-plugin fmt  # or sync-embed"
                )
    elif runtime == "native":
        cmd = Path(m["command"])
        resolved = cmd if cmd.is_absolute() else plugin_dir / cmd
        if not resolved.exists() and (plugin_dir / ".require-binary").exists():
            raise FileNotFoundError(f"native command not found: {resolved}")
    entrypoints = ",".join(str(e) for e in (m.get("entrypoints") or []))
    return f"ok id={m['id']} entrypoints={entrypoints} runtime={runtime}"


def sync_embed(plugin_dir: Path) -> str:
    """Vendor SDK sources under ``modules/`` for offline workerd archives.

    Prefer package imports — ``bookclerk-workerd`` injects
    ``bookclerk_plugin_sdk.workerd`` at runtime. This writes the same files so a
    staged tree is self-contained without host injection. Also ensures Python
    Workers compatibility flags in ``plugin.toml``.

    Args:
        plugin_dir: Path to a workerd Python plugin root.

    Returns:
        Status string describing the synced path (and flag updates, if any).

    Raises:
        ValueError: If the plugin is not a Python workerd guest.
        OSError: If files cannot be read or written.

    Examples:
        >>> # print(sync_embed(Path("./my-python-workerd-plugin")))
    """
    toml_path = plugin_dir / "plugin.toml"
    text = toml_path.read_text(encoding="utf-8")
    m = tomllib.loads(text)
    validate_manifest(m)
    if (m.get("runtime") or "native") != "workerd":
        raise ValueError("sync-embed requires runtime = \"workerd\"")
    if not _is_python_workerd(m):
        raise ValueError(
            "sync-embed (Python SDK): main_module must end with .py "
            f"(got {m['workerd'].get('main_module')!r})"
        )
    modules_dir = _workerd_modules_dir(plugin_dir, m)
    pkg = modules_dir / "bookclerk_plugin_sdk"
    pkg.mkdir(parents=True, exist_ok=True)
    init = pkg / "__init__.py"
    if not init.is_file():
        init.write_text(
            '"""Bookclerk plugin SDK (vendored for workerd). Prefer .workerd."""\n',
            encoding="utf-8",
        )
    dest = pkg / "workerd.py"
    shutil.copy2(_sdk_workerd_embed_src(), dest)

    new_text = _ensure_python_flags_in_toml_text(text, m)
    if new_text != text:
        toml_path.write_text(new_text, encoding="utf-8")
        return f"synced {dest} + python workerd flags in {toml_path}"
    return f"synced {dest}"


def _ensure_python_flags_in_toml_text(text: str, m: dict[str, Any]) -> str:
    """Insert or replace `compatibility_flags` for Python workerd without a full fmt."""
    w = m.get("workerd") or {}
    flags = list(w.get("compatibility_flags") or [])
    if all(f in flags for f in PYTHON_WORKERD_FLAGS):
        return text
    flag_block = (
        "compatibility_flags = [\n"
        + "".join(f'    "{f}",\n' for f in _ensure_python_flags(flags))
        + "]"
    )
    if re.search(r"(?m)^\s*compatibility_flags\s*=", text):
        return re.sub(
            r"(?ms)^\s*compatibility_flags\s*=\s*\[.*?\]",
            flag_block,
            text,
            count=1,
        )
    date = str(w.get("compatibility_date") or "")
    needle = f'compatibility_date = "{date}"'
    if needle in text:
        return text.replace(needle, f"{needle}\n{flag_block}", 1)
    return re.sub(
        r"(?m)^\[workerd\]\s*$",
        f"[workerd]\n{flag_block}",
        text,
        count=1,
    )


def _esc(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def _array(values: list[str]) -> str:
    """Emit a TOML array like ``toml::to_string_pretty``: inline for one element."""
    if not values:
        return "[]"
    if len(values) == 1:
        return f"[{values[0]}]"
    inner = ",\n".join(f"    {v}" for v in values)
    return f"[\n{inner},\n]"


def _string_array(values: list[str]) -> str:
    return _array([_esc(v) for v in values])


def _number_array(values: list[Any]) -> str:
    return _array([str(v) for v in values])


def _bool(value: Any) -> str:
    return "true" if value else "false"


def _value(value: Any) -> str | None:
    if isinstance(value, bool):
        return _bool(value)
    if isinstance(value, str):
        return _esc(value)
    if isinstance(value, (int, float)):
        return str(value)
    if isinstance(value, list) and all(isinstance(v, str) for v in value):
        return _string_array(list(value))
    return None


def _table_rows(lines: list[str], table: dict[str, Any]) -> None:
    for key in sorted(table):
        rendered = _value(table[key])
        if rendered is not None:
            lines.append(f"{key} = {rendered}")


def _named_binding(lines: list[str], header: str, binding: Any) -> None:
    if binding is None:
        return
    lines.append("")
    lines.append(header)
    name = str((binding or {}).get("binding") or "") if isinstance(binding, dict) else ""
    if name:
        lines.append(f"binding = {_esc(name)}")


def format_manifest(m: dict[str, Any]) -> str:
    """Emit canonical TOML matching Rust ``format_manifest`` gold fixtures.

    Args:
        m: Validated manifest dictionary.

    Returns:
        Canonical ``plugin.toml`` text ending with a newline.
    """
    if (m.get("runtime") or "native") == "workerd" and _is_python_workerd(m):
        w = dict(m.get("workerd") or {})
        w["compatibility_flags"] = _ensure_python_flags(w.get("compatibility_flags"))
        m = {**m, "workerd": w}

    lines: list[str] = []
    lines.append(f"api_version = {m['api_version']}")
    lines.append(f"id = {_esc(m['id'])}")
    if m.get("name") is not None:
        lines.append(f"name = {_esc(m['name'])}")
    if m.get("version") is not None:
        lines.append(f"version = {_esc(m['version'])}")
    if m.get("logo") is not None:
        lines.append(f"logo = {_esc(m['logo'])}")
    runtime = m.get("runtime") or "native"
    lines.append(f"runtime = {_esc(runtime)}")
    if m.get("command") is not None:
        lines.append(f"command = {_esc(m['command'])}")
    if m.get("args"):
        lines.append(f"args = {_string_array(list(m['args']))}")
    if m.get("entrypoints"):
        lines.append(f"entrypoints = {_string_array([str(e) for e in m['entrypoints']])}")

    if m.get("workerd"):
        w = m["workerd"]
        lines.append("")
        lines.append("[workerd]")
        lines.append(f"compatibility_date = {_esc(w['compatibility_date'])}")
        if w.get("compatibility_flags"):
            lines.append(f"compatibility_flags = {_string_array(list(w['compatibility_flags']))}")
        lines.append(f"main_module = {_esc(w['main_module'])}")
        lines.append(f"modules_dir = {_esc(w.get('modules_dir') or 'modules')}")
        lines.append(f"entrypoint = {_esc(w.get('entrypoint') or 'default')}")
        limits = w.get("limits") or {}
        if limits.get("cpu_ms") is not None or limits.get("subrequests") is not None:
            lines.append("")
            lines.append("[workerd.limits]")
            if limits.get("cpu_ms") is not None:
                lines.append(f"cpu_ms = {limits['cpu_ms']}")
            if limits.get("subrequests") is not None:
                lines.append(f"subrequests = {limits['subrequests']}")

    for mod in m.get("modules") or []:
        lines.append("")
        lines.append("[[modules]]")
        lines.append(f"name = {_esc(str(mod['name']))}")
        lines.append(f"path = {_esc(str(mod['path']))}")
        lines.append(f"type = {_esc(str(mod.get('type') or 'js'))}")

    jobs = list((m.get("triggers") or {}).get("jobs") or [])
    if jobs:
        lines.append("")
        lines.append("[triggers]")
        lines.append(f"jobs = {_string_array([str(j) for j in jobs])}")

    events = m.get("events") or {}
    for consumer in events.get("consumers") or []:
        lines.append("")
        lines.append("[[events.consumers]]")
        lines.append(f"type = {_esc(str(consumer['type']))}")
        lines.append(f"schema_versions = {_number_array(list(consumer.get('schema_versions') or [1]))}")
        lines.append(f"supports_suspend = {_bool(consumer.get('supports_suspend'))}")
        lines.append(f"resource_class = {_esc(str(consumer.get('resource_class') or 'network'))}")
        if consumer.get("max_retries") is not None:
            lines.append(f"max_retries = {consumer['max_retries']}")
        filt = consumer.get("filter")
        if isinstance(filt, dict) and filt:
            lines.append("")
            lines.append("[events.consumers.filter]")
            _table_rows(lines, filt)
    for producer in events.get("producers") or []:
        lines.append("")
        lines.append("[[events.producers]]")
        lines.append(f"type = {_esc(str(producer['type']))}")
        if producer.get("binding"):
            lines.append(f"binding = {_esc(str(producer['binding']))}")

    for db in m.get("databases") or []:
        lines.append("")
        lines.append("[[databases]]")
        lines.append(f"binding = {_esc(str(db['binding']))}")

    if m.get("vars") is not None:
        lines.append("")
        lines.append("[vars]")
        _table_rows(lines, dict(m["vars"]))
    _named_binding(lines, "[secrets]", m.get("secrets"))
    for kv in m.get("kv_namespaces") or []:
        _named_binding(lines, "[[kv_namespaces]]", kv)
    _named_binding(lines, "[work_fs]", m.get("work_fs"))
    _named_binding(lines, "[oauth]", m.get("oauth"))

    caps = m.get("capabilities") or {}
    net = caps.get("network") or {}
    lines.append("")
    lines.append("[capabilities.network]")
    lines.append(f"mode = {_esc(net.get('mode') or 'deny')}")
    if net.get("domains"):
        lines.append(f"domains = {_string_array(list(net['domains']))}")

    cli = m.get("cli") or {}
    for cmd in cli.get("commands") or []:
        lines.append("")
        lines.append("[[cli.commands]]")
        lines.append(f"name = {_esc(cmd['name'])}")
        if cmd.get("about") is not None:
            lines.append(f"about = {_esc(cmd['about'])}")
        if not cmd.get("args"):
            lines.append("args = []")
        for arg in cmd.get("args") or []:
            lines.append("")
            lines.append("[[cli.commands.args]]")
            lines.append(f"name = {_esc(str(arg['name']))}")
            if arg.get("long") is not None:
                lines.append(f"long = {_esc(str(arg['long']))}")
            if arg.get("short") is not None:
                lines.append(f"short = {_esc(str(arg['short']))}")
            lines.append(f"kind = {_esc(str(arg.get('kind') or 'string'))}")
            lines.append(f"required = {_bool(arg.get('required'))}")
            if arg.get("default") is not None:
                lines.append(f"default = {_esc(str(arg['default']))}")
            if arg.get("about") is not None:
                lines.append(f"about = {_esc(str(arg['about']))}")
            lines.append(f"positional = {_bool(arg.get('positional'))}")

    for client in (m.get("oidc") or {}).get("clients") or []:
        lines.append("")
        lines.append("[[oidc.clients]]")
        lines.append(f"client_id = {_esc(str(client['client_id']))}")
        if client.get("display_name"):
            lines.append(f"display_name = {_esc(str(client['display_name']))}")
        lines.append(f"callback_path = {_esc(str(client['callback_path']))}")
        lines.append(f"public_client = {_bool(client.get('public_client', True))}")
        scopes = list(client.get("default_scopes") or [])
        if scopes:
            lines.append(f"default_scopes = {_string_array([str(sc) for sc in scopes])}")
        lines.append(f"issue_refresh_token = {_bool(client.get('issue_refresh_token', True))}")
        lines.append(f"origin_config_key = {_esc(str(client['origin_config_key']))}")

    out = "\n".join(lines)
    if not out.endswith("\n"):
        out += "\n"
    return out


def fmt_plugin_toml(path: Path, *, check_only: bool) -> str:
    """Format ``plugin.toml`` in place, or check that it is already canonical.

    Args:
        path: Path to ``plugin.toml``.
        check_only: When ``True``, raise if reformatting would change the file.

    Returns:
        Status string (``ok …`` or ``wrote …``).

    Raises:
        ValueError: If validation fails or ``check_only`` finds a drift.
        OSError: If the file cannot be read or written.
    """
    text = path.read_text(encoding="utf-8")
    m = tomllib.loads(text)
    validate_manifest(m)
    formatted = format_manifest(m)

    def norm(s: str) -> str:
        s = s.replace("\r\n", "\n")
        return s if s.endswith("\n") else s + "\n"

    if check_only:
        if norm(text) != norm(formatted):
            raise ValueError(f"would reformat {path}")
        return f"ok {path}"
    path.write_text(formatted, encoding="utf-8")
    return f"wrote {path}"


def _host_target() -> str:
    import platform

    sysname = sys.platform
    machine = platform.machine().lower()
    if sysname.startswith("linux") and machine in {"x86_64", "amd64"}:
        return "linux-x64-gnu"
    if sysname.startswith("linux") and machine in {"aarch64", "arm64"}:
        return "linux-arm64"
    if sysname == "darwin" and machine in {"arm64", "aarch64"}:
        return "macos-arm64"
    if sysname == "darwin" and machine in {"x86_64", "amd64"}:
        return "macos-x64"
    if sysname.startswith("win") and machine in {"x86_64", "amd64"}:
        return "windows-x64"
    return f"{sysname}-{machine}"


def package_plugin(plugin_dir: Path, out_dir: Path) -> Path:
    """Pack a plugin into a ``.tar.gz`` archive and update ``SHA256SUMS``.

    Args:
        plugin_dir: Path to the plugin root.
        out_dir: Destination directory for the archive and checksums file.

    Returns:
        Path to the created ``.tar.gz`` archive.

    Raises:
        ValueError: If the manifest is invalid.
        FileNotFoundError: If a required native binary, modules tree, or logo is missing.
        subprocess.CalledProcessError: If ``tar`` fails.

    Examples:
        >>> # archive = package_plugin(Path("./my-plugin"), Path("./dist"))
        >>> # print(f"packed {archive}")
    """
    m = tomllib.loads((plugin_dir / "plugin.toml").read_text(encoding="utf-8"))
    validate_manifest(m)
    version = m.get("version") or "0.0.0"
    plugin_id = m["id"]
    out_dir.mkdir(parents=True, exist_ok=True)
    staging = out_dir / f".staging-{plugin_id}"
    if staging.exists():
        shutil.rmtree(staging)
    staging.mkdir(parents=True)
    runtime = m.get("runtime") or "native"
    if runtime == "native":
        shutil.copy2(plugin_dir / "plugin.toml", staging / "plugin.toml")
        cmd = Path(m["command"])
        src = cmd if cmd.is_absolute() else plugin_dir / cmd
        if not src.is_file():
            raise FileNotFoundError(f"native binary not found for package: {src}")
        dest = staging / src.name
        shutil.copy2(src, dest)
        os.chmod(dest, 0o755)
        stem = f"bookclerk-plugin-{plugin_id}-{version}-{_host_target()}"
    else:
        modules_dir = m["workerd"].get("modules_dir") or "modules"
        shutil.copytree(plugin_dir / modules_dir, staging / modules_dir)
        toml_text = (plugin_dir / "plugin.toml").read_text(encoding="utf-8")
        if _is_python_workerd(m):
            # Vendor package-shaped SDK so archives work even without host injection.
            pkg = staging / modules_dir / "bookclerk_plugin_sdk"
            pkg.mkdir(parents=True, exist_ok=True)
            (pkg / "__init__.py").write_text(
                '"""Bookclerk plugin SDK (vendored for workerd)."""\n',
                encoding="utf-8",
            )
            shutil.copy2(_sdk_workerd_embed_src(), pkg / "workerd.py")
            toml_text = _ensure_python_flags_in_toml_text(toml_text, m)
        (staging / "plugin.toml").write_text(toml_text, encoding="utf-8")
        stem = f"bookclerk-plugin-{plugin_id}-{version}-workerd"

    if m.get("logo") is not None:
        kind, value = validate_logo(str(m["logo"]))
        if kind == "embedded":
            src = plugin_dir / value
            if not src.is_file():
                raise FileNotFoundError(f"embedded logo missing for package: {src}")
            dest = staging / value
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(src, dest)

    archive_name = f"{stem}.tar.gz"
    archive_path = out_dir / archive_name
    subprocess.run(
        ["tar", "-C", str(staging), "-czf", str(archive_path), "."],
        check=True,
    )
    shutil.rmtree(staging)
    digest = hashlib.sha256(archive_path.read_bytes()).hexdigest()
    sums = out_dir / "SHA256SUMS"
    lines = []
    if sums.is_file():
        lines = [
            ln
            for ln in sums.read_text(encoding="utf-8").splitlines()
            if ln and not ln.endswith(archive_name)
        ]
    lines.append(f"{digest}  {archive_name}")
    sums.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return archive_path
