#!/usr/bin/env python3
"""Live-smoke Libro.fm API calls: current client constants vs APK-extracted surface.

Credentials (first match wins):
  email:    TEST_LIBRO_EMAIL | TEST_LIBRO_USERNAME | TEST_LIBRO_USER |
            LIBRO_FM_USERNAME
  password: TEST_LIBRO_PASSWORD | LIBATION_LIBRO_PASSWORD | LIBRO_FM_PASSWORD

Optional:
  TEST_LIBRO_ISBN — prefer this ISBN for download-manifest / packaged_m4b checks
                    (otherwise first library ISBN is used). Cap: one book only.

Exit codes:
  0 — selected profiles succeeded
  1 — one or more profile calls failed
  2 — missing credentials / bad args
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any


def first_env(*names: str) -> str | None:
    for name in names:
        value = os.environ.get(name)
        if value:
            return value
    return None


@dataclass
class Profile:
    name: str
    base_url: str
    oauth_path: str
    library_path: str
    download_manifest_path: str
    packaged_m4b_path: str
    app_ver: str
    user_agent: str
    extra_headers: dict[str, str]
    manifest_extra_query: dict[str, str]


def load_client_profile(repo: Path) -> Profile:
    # Import constants by parsing client.rs (avoid compiling Rust for a smoke).
    text = (repo / "crates/libation-libro/src/client.rs").read_text(encoding="utf-8")
    consts: dict[str, str] = {}
    for line in text.splitlines():
        line = line.strip()
        if not line.startswith("pub const "):
            continue
        # pub const FOO: &str = "bar";
        try:
            name = line.split()[2].rstrip(":")
            value = line.split("=", 1)[1].strip().rstrip(";").strip().strip('"')
            consts[name] = value
        except IndexError:
            continue
    # Mirror crates/libation-libro/src/client.rs AuthInterceptor-equivalent
    # headers and DownloadApi client_version query (not separate pub consts).
    return Profile(
        name="current-client",
        base_url=consts["DEFAULT_BASE_URL"],
        oauth_path=consts["OAUTH_TOKEN_PATH"],
        library_path=consts["LIBRARY_PATH"],
        download_manifest_path=consts["DOWNLOAD_MANIFEST_PATH"],
        packaged_m4b_path=consts["PACKAGED_M4B_PATH"],
        app_ver=consts["APP_VER"],
        user_agent=consts["USER_AGENT_VALUE"],
        extra_headers={
            "X-LibroFm-Device": "libation-rs",
            "X-LibroFm-OsVer": "Android 34",
        },
        manifest_extra_query={
            "client_version": consts["APP_VER"],
        },
    )


def load_apk_profile(report_path: Path) -> Profile:
    report = json.loads(report_path.read_text(encoding="utf-8"))
    apk = report["apk"]
    tracked = apk.get("absolute_tracked_paths") or {}
    # Match Android AuthInterceptor prod headers (no Api-Key on prod).
    return Profile(
        name="apk-extracted",
        base_url=apk["base_url"],
        oauth_path=apk.get("oauth_token_path") or "/oauth/token",
        library_path=tracked["library"],
        download_manifest_path=tracked["download-manifest"],
        packaged_m4b_path=tracked["audiobooks/{isbn}/packaged_m4b"],
        app_ver=apk["version_name"],
        user_agent=apk["okhttp_user_agent"],
        extra_headers={
            "X-LibroFm-Device": "libation-rs-smoke",
            "X-LibroFm-OsVer": "Android 34",
        },
        # Android DownloadApi: client_version=appVer, format=null for ZIP / "m4b" for M4B.
        manifest_extra_query={
            "client_version": apk["version_name"],
        },
    )


def http_json(
    method: str,
    url: str,
    *,
    headers: dict[str, str],
    body: dict[str, Any] | None = None,
    timeout: float = 60.0,
) -> tuple[int, Any, str]:
    data = None
    req_headers = dict(headers)
    if body is not None:
        data = json.dumps(body).encode("utf-8")
        req_headers.setdefault("Content-Type", "application/json")
    req = urllib.request.Request(url, data=data, headers=req_headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            raw = resp.read().decode("utf-8", errors="replace")
            status = resp.getcode()
    except urllib.error.HTTPError as exc:
        raw = exc.read().decode("utf-8", errors="replace")
        status = exc.code
    except Exception as exc:  # noqa: BLE001
        return 0, None, f"request error: {exc}"

    parsed: Any
    try:
        parsed = json.loads(raw) if raw.strip() else None
    except json.JSONDecodeError:
        parsed = None
    return status, parsed, raw[:500]


def profile_headers(profile: Profile, token: str | None = None) -> dict[str, str]:
    headers = {
        "Accept": "application/json",
        "User-Agent": profile.user_agent,
        "X-LibroFm-AppVer": profile.app_ver,
        **profile.extra_headers,
    }
    if token:
        headers["Authorization"] = f"Bearer {token}"
    return headers


def run_profile(
    profile: Profile,
    email: str,
    password: str,
    preferred_isbn: str | None,
) -> dict[str, Any]:
    result: dict[str, Any] = {"profile": profile.name, "ok": False, "steps": []}
    base = profile.base_url.rstrip("/")

    # 1) OAuth
    status, parsed, raw = http_json(
        "POST",
        f"{base}{profile.oauth_path}",
        headers=profile_headers(profile),
        body={
            "grant_type": "password",
            "username": email,
            "password": password,
        },
    )
    step = {"name": "oauth", "status": status}
    if status != 200 or not isinstance(parsed, dict) or not parsed.get("access_token"):
        step["error"] = raw
        result["steps"].append(step)
        return result
    token = parsed["access_token"]
    step["ok"] = True
    result["steps"].append(step)

    # 2) Library page 1
    status, parsed, raw = http_json(
        "GET",
        f"{base}{profile.library_path}?page=1",
        headers=profile_headers(profile, token),
    )
    step = {"name": "library", "status": status, "path": profile.library_path}
    if status != 200 or not isinstance(parsed, dict):
        step["error"] = raw
        result["steps"].append(step)
        return result
    books = parsed.get("audiobooks") or []
    step["ok"] = True
    step["book_count"] = len(books)
    step["total_pages"] = parsed.get("total_pages")
    result["steps"].append(step)

    isbn = preferred_isbn
    if not isbn and books:
        first = books[0]
        isbn = str(first.get("isbn") or "")
    if not isbn:
        result["ok"] = True
        result["note"] = "library empty; skipped download checks"
        return result

    # 3) Packaged M4B (404 / empty is OK)
    m4b_path = profile.packaged_m4b_path.replace("{isbn}", isbn)
    status, parsed, raw = http_json(
        "GET",
        f"{base}{m4b_path}",
        headers=profile_headers(profile, token),
    )
    step = {
        "name": "packaged_m4b",
        "status": status,
        "path": m4b_path,
        "isbn": isbn,
    }
    if status in (200, 404) or (400 <= status < 500):
        step["ok"] = True
        if isinstance(parsed, dict):
            step["has_m4b_url"] = bool(parsed.get("m4b_url"))
    else:
        step["error"] = raw
    result["steps"].append(step)
    if not step.get("ok"):
        return result

    # 4) Download manifest (metadata only — do not download audio)
    q = {"isbn": isbn, **profile.manifest_extra_query}
    url = f"{base}{profile.download_manifest_path}?{urllib.parse.urlencode(q)}"
    status, parsed, raw = http_json(
        "GET",
        url,
        headers=profile_headers(profile, token),
    )
    step = {
        "name": "download_manifest",
        "status": status,
        "path": profile.download_manifest_path,
        "isbn": isbn,
        "query": q,
    }
    if status != 200 or not isinstance(parsed, dict):
        step["error"] = raw
        result["steps"].append(step)
        return result
    parts = parsed.get("parts") or []
    tracks = parsed.get("tracks") or []
    step["ok"] = True
    step["parts"] = len(parts)
    step["tracks"] = len(tracks)
    result["steps"].append(step)

    result["ok"] = all(s.get("ok") for s in result["steps"])
    return result


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path(__file__).resolve().parents[2],
    )
    parser.add_argument(
        "--report",
        type=Path,
        default=None,
        help="APK probe report.json (enables apk-extracted profile)",
    )
    parser.add_argument(
        "--profiles",
        default="current,apk",
        help="Comma list: current, apk",
    )
    parser.add_argument(
        "--out",
        type=Path,
        default=None,
        help="Write JSON results here",
    )
    args = parser.parse_args(argv)

    email = first_env(
        "TEST_LIBRO_EMAIL",
        "TEST_LIBRO_USERNAME",
        "TEST_LIBRO_USER",
        "LIBRO_FM_USERNAME",
    )
    password = first_env(
        "TEST_LIBRO_PASSWORD", "LIBATION_LIBRO_PASSWORD", "LIBRO_FM_PASSWORD"
    )
    if not email or not password:
        print(
            "error: missing Libro credentials. Set TEST_LIBRO_EMAIL (or "
            "TEST_LIBRO_USERNAME / TEST_LIBRO_USER) and TEST_LIBRO_PASSWORD.",
            file=sys.stderr,
        )
        print(
            "hint: injected cloud secrets currently visible to this process:",
            file=sys.stderr,
        )
        print(
            f"  CLOUD_AGENT_INJECTED_SECRET_NAMES="
            f"{os.environ.get('CLOUD_AGENT_INJECTED_SECRET_NAMES', '')}",
            file=sys.stderr,
        )
        return 2

    isbn = first_env("TEST_LIBRO_ISBN")
    wanted = {p.strip() for p in args.profiles.split(",") if p.strip()}
    profiles: list[Profile] = []
    if "current" in wanted:
        profiles.append(load_client_profile(args.repo_root))
    if "apk" in wanted:
        report = args.report or (
            args.repo_root / "artifacts/librofm-apk-probe/report.json"
        )
        if not report.exists():
            print(f"error: APK report not found: {report}", file=sys.stderr)
            return 2
        profiles.append(load_apk_profile(report))

    results = [run_profile(p, email, password, isbn) for p in profiles]
    payload = {"results": results, "email_present": True, "isbn": isbn}
    text = json.dumps(payload, indent=2)
    print(text)
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(text + "\n", encoding="utf-8")

    if not results:
        return 2
    return 0 if all(r["ok"] for r in results) else 1


if __name__ == "__main__":
    sys.exit(main())
