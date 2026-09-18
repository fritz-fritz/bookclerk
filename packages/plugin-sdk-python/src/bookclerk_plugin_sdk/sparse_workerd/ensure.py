"""Download / refresh the pinned Cloudflare ``workerd`` binary (mirrors ensure.rs).

Resolves the platform asset from ``workerd-pin.json``, verifies sha256, and
caches under ``~/.cache/bookclerk/workerd`` (or ``BOOKCLERK_WORKERD_CACHE``).
"""

from __future__ import annotations

import gzip
import hashlib
import os
import platform
import shutil
import subprocess
import urllib.request
from pathlib import Path
from typing import Any

from ..path_guard import resolve_under

_PKG = Path(__file__).resolve().parent.parent  # bookclerk_plugin_sdk/


def package_root() -> Path:
    """Return the ``bookclerk_plugin_sdk`` package directory.

    Returns:
        Absolute path containing ``workerd.py``, ``bridge/``, and the pin file.
    """
    return _PKG


def load_pin(root: Path | None = None) -> dict[str, Any]:
    """Load the pinned workerd release metadata.

    Args:
        root: Package root containing ``workerd-pin.json`` (defaults to this SDK).

    Returns:
        Parsed pin dictionary (release tag, assets, version stamp).

    Raises:
        FileNotFoundError: If the pin file is missing.
        json.JSONDecodeError: If the pin file is not valid JSON.
    """
    pin_path = resolve_under(root or package_root(), "workerd-pin.json")
    import json

    return json.loads(pin_path.read_text(encoding="utf-8"))


def platform_key(
    system: str | None = None,
    machine: str | None = None,
) -> str | None:
    """Map OS/arch to a pin asset key such as ``linux-x86_64``.

    Args:
        system: OS name override (defaults to ``platform.system()``).
        machine: CPU arch override (defaults to ``platform.machine()``).

    Returns:
        Pin key string, or ``None`` when the platform is unsupported.
    """
    system = (system or platform.system()).lower()
    machine = (machine or platform.machine()).lower()
    os_map = {"linux": "linux", "darwin": "macos", "windows": "windows"}
    arch_map = {
        "x86_64": "x86_64",
        "amd64": "x86_64",
        "aarch64": "aarch64",
        "arm64": "aarch64",
    }
    os_name = os_map.get(system)
    arch = arch_map.get(machine)
    if not os_name or not arch:
        return None
    return f"{os_name}-{arch}"


def binary_name() -> str:
    """Return the on-disk workerd binary filename for this OS.

    Returns:
        ``workerd.exe`` on Windows, otherwise ``workerd``.
    """
    return "workerd.exe" if platform.system().lower() == "windows" else "workerd"


def download_url(pin: dict[str, Any], artifact: str) -> str:
    """Build the GitHub release download URL for a workerd artifact.

    Args:
        pin: Loaded pin metadata containing ``release_tag``.
        artifact: Asset filename from the pin ``assets`` table.

    Returns:
        Absolute HTTPS URL to the Cloudflare workerd release asset.
    """
    tag = pin["release_tag"]
    return f"https://github.com/cloudflare/workerd/releases/download/{tag}/{artifact}"


def default_cache_dir() -> Path:
    """Resolve the workerd binary cache directory.

    Honors ``BOOKCLERK_WORKERD_CACHE`` when set; otherwise uses
    ``~/.cache/bookclerk/workerd``.

    Returns:
        Path to the cache directory (may not exist yet).
    """
    if env := os.environ.get("BOOKCLERK_WORKERD_CACHE"):
        return Path(env)
    home = Path.home()
    return home / ".cache" / "bookclerk" / "workerd"


def validate_fetch_url(url: str) -> str:
    """Validate a URL before ``urlopen`` / download (request-forgery guard).

    Allows ``https:`` anywhere, or ``http:`` only to loopback hosts.

    Args:
        url: Absolute URL string.

    Returns:
        The same URL when it passes scheme/host checks.

    Raises:
        ValueError: When the URL is invalid or uses a disallowed scheme/host.
    """
    from urllib.parse import urlparse

    parsed = urlparse(url)
    if parsed.scheme == "https" and parsed.netloc:
        return url
    if parsed.scheme == "http":
        host = (parsed.hostname or "").lower()
        if host in {"127.0.0.1", "localhost", "::1"}:
            return url
    raise ValueError(
        f"refusing non-HTTPS (or non-loopback HTTP) URL: {parsed.scheme}://{parsed.netloc}"
    )


def validate_spawn_executable(
    bin_path: Path | str,
    trusted_root: Path | str | None = None,
) -> Path:
    """Validate a workerd (or helper) binary path before ``subprocess`` spawn.

    Requires an absolute path with no NUL bytes. When ``trusted_root`` is set,
    requires the binary to stay under that root via :func:`resolve_under`.

    Args:
        bin_path: Candidate executable path.
        trusted_root: Optional directory the binary must remain under.

    Returns:
        Absolute validated :class:`~pathlib.Path`.

    Raises:
        ValueError: When the path is relative, contains NUL, or escapes
            ``trusted_root``.
    """
    path = Path(bin_path)
    s = os.fspath(path)
    if not s or "\0" in s:
        raise ValueError("spawn executable path is empty or contains NUL")
    if not path.is_absolute():
        raise ValueError(f"spawn executable must be absolute: {path}")
    if trusted_root is not None:
        return resolve_under(trusted_root, path)
    return Path(os.path.abspath(s))


def _stamp_file_name(pin: dict[str, Any]) -> str:
    stamp = pin.get("version_stamp")
    if not isinstance(stamp, str) or not stamp:
        raise ValueError("invalid workerd version_stamp")
    if (
        "\0" in stamp
        or "/" in stamp
        or "\\" in stamp
        or stamp in {".", ".."}
        or ".." in stamp
    ):
        raise ValueError(f"invalid workerd version_stamp: {stamp}")
    return stamp


def _is_current(
    bin_path: Path,
    pin: dict[str, Any],
    *,
    probe_root: Path | None = None,
) -> bool:
    stamp = resolve_under(bin_path.parent, _stamp_file_name(pin))
    if stamp.is_file() and stamp.read_text(encoding="utf-8").strip() == pin["release_tag"]:
        return True
    # Never ``--version``-probe an env override; stamp mismatch ⇒ not current.
    if probe_root is None:
        return False
    try:
        validated = validate_spawn_executable(bin_path, probe_root)
        proc = subprocess.run(
            [os.fspath(validated), "--version"],
            capture_output=True,
            text=True,
            check=False,
            shell=False,
        )
    except (OSError, ValueError):
        return False
    if proc.returncode != 0:
        return False
    combined = f"{proc.stdout}{proc.stderr}"
    tag = pin["release_tag"]
    bare = tag.lstrip("v")
    return tag in combined or bare in combined


def ensure_workerd(
    cache_dir: Path | None = None,
    root: Path | None = None,
) -> Path:
    """Ensure a pinned ``workerd`` binary is available locally.

    Reuses ``BOOKCLERK_WORKERD_BIN`` or a cached binary when the version stamp
    matches the pin; otherwise downloads, verifies sha256, and installs.

    Args:
        cache_dir: Override cache directory (defaults to :func:`default_cache_dir`).
        root: SDK root for loading ``workerd-pin.json``.

    Returns:
        Path to an executable workerd binary matching the pin.

    Raises:
        RuntimeError: If no asset exists for this platform or the download hash
            mismatches.
        OSError: If the binary cannot be written or executed.
        ValueError: If a resolved binary path fails spawn validation.
    """
    pin = load_pin(root)
    override = os.environ.get("BOOKCLERK_WORKERD_BIN")
    if override:
        path = Path(override)
        if path.is_file() and _is_current(path, pin):
            # Env override: stamp-only currency check (no spawn of the env path).
            return validate_spawn_executable(path)

    cache_s = os.path.abspath(os.fspath(cache_dir or default_cache_dir()))
    cache = Path(cache_s)
    cache.mkdir(parents=True, exist_ok=True)
    dest = resolve_under(cache, binary_name())
    if dest.is_file() and _is_current(dest, pin, probe_root=cache):
        return validate_spawn_executable(dest, cache)

    key = platform_key()
    assets = pin.get("assets") or {}
    if not key or key not in assets:
        raise RuntimeError(
            f"no pinned workerd asset for {platform.system()}-{platform.machine()}"
        )
    asset = assets[key]
    url = validate_fetch_url(download_url(pin, asset["artifact"]))
    print(f"bookclerk-plugin: fetching {url}", flush=True)
    # HTTPS (or loopback HTTP) URL validated above.
    with urllib.request.urlopen(url) as resp:  # noqa: S310 — pinned GitHub release URL
        compressed = resp.read()
    got = hashlib.sha256(compressed).hexdigest()
    if got != asset["sha256_hex"]:
        raise RuntimeError(
            f"workerd download sha256 mismatch: got {got}, expected {asset['sha256_hex']}"
        )

    tmp = resolve_under(cache, f"{binary_name()}.tmp")
    with gzip.GzipFile(fileobj=__import__("io").BytesIO(compressed)) as gz, tmp.open(
        "wb"
    ) as out:
        shutil.copyfileobj(gz, out)
    if platform.system().lower() != "windows":
        tmp.chmod(0o755)
    tmp.replace(dest)
    stamp_path = resolve_under(cache, _stamp_file_name(pin))
    stamp_path.write_text(f"{pin['release_tag']}\n", encoding="utf-8")
    print(f"bookclerk-plugin: installed {pin['release_tag']} → {dest}", flush=True)
    return validate_spawn_executable(dest, cache)
