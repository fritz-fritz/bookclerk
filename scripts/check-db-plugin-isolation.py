#!/usr/bin/env python3
"""Forbid database plugins from embedding Bookclerk domain SQL.

Database guests execute host-authored generic plans. Plugin sources must not
import `bookclerk-library` entities or mention application table identifiers.
See docs/adr/sql-database-contract.md.

Usage:
    python3 scripts/check-db-plugin-isolation.py
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PLUGIN_GLOBS = (
    "crates/bookclerk-plugins/platform/database-*/src/**/*.rs",
    "crates/bookclerk-plugins/optional/database-*/src/**/*.rs",
    "crates/bookclerk-db-guest/src/**/*.rs",
)

FORBIDDEN_IMPORTS = (
    re.compile(r"bookclerk_library::entities"),
    re.compile(r"use\s+bookclerk_library::entities"),
    re.compile(r"bookclerk_library::sql_plan"),
    re.compile(r"bookclerk_library::migrations"),
    re.compile(r"\bmigration_sql\b"),
    re.compile(r"\bmigration_sql_postgres\b"),
    re.compile(r"\bmigration_sql_d1\b"),
    re.compile(r"\binterpret_plan\b"),
    re.compile(r"\bDbAtomicParams\b"),
    re.compile(r"\batomic_status\b"),
)

# Application tables the host owns. Guests must not mention these identifiers.
FORBIDDEN_TABLES = (
    "domain_events",
    "event_deliveries",
    "event_outbox_stats",
    "event_subscriber_nodes",
    "users",
    "jobs",
    "job_temp_paths",
    "job_queue_control",
    "books",
    "accounts",
    "claim_tickets",
    "oidc_rp_states",
    "webauthn_challenges",
    "portal_identities",
    "portal_sessions",
    "operator_sessions",
    "encrypted_secrets",
    "db_atomic_receipts",
    "db_serialization_slots",
)

TABLE_RE = re.compile(
    r"(?:FROM|INTO|UPDATE|JOIN|TABLE|EXISTS)\s+(?:IF\s+NOT\s+EXISTS\s+)?"
    r"(" + "|".join(re.escape(t) for t in FORBIDDEN_TABLES) + r")\b",
    re.IGNORECASE,
)

JSON_EACH_RE = re.compile(r"\bjson_each\b", re.IGNORECASE)

# Production guests and the shared guest crate must not take a non-optional
# `bookclerk-library` dependency. Optional (`host-helpers`) and `[dev-dependencies]`
# are allowed for tests/CLI helpers.
LIBRARY_ISOLATION_PACKAGES = (
    "bookclerk-db-guest",
    "bookclerk-plugin-database-sqlite",
    "bookclerk-plugin-database-postgres",
    "bookclerk-plugin-database-d1",
)


def check_cargo_metadata() -> list[str]:
    """Refuse a non-optional production `bookclerk-library` dep on guest crates."""
    proc = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        return [f"cargo metadata failed: {proc.stderr.strip() or proc.stdout.strip()}"]
    meta = json.loads(proc.stdout)
    hits: list[str] = []
    for pkg in meta.get("packages", []):
        if pkg.get("name") not in LIBRARY_ISOLATION_PACKAGES:
            continue
        manifest = Path(pkg.get("manifest_path", ""))
        try:
            manifest.relative_to(ROOT)
        except ValueError:
            continue
        for dep in pkg.get("dependencies", []):
            if dep.get("name") != "bookclerk-library":
                continue
            kind = dep.get("kind")
            optional = bool(dep.get("optional"))
            if kind in (None, "normal") and not optional:
                hits.append(
                    f"{pkg['name']}: non-optional dependency on bookclerk-library "
                    "(use bookclerk-db-exec, optional host-helpers, or a dev-dependency)"
                )
    return hits


def iter_plugin_sources() -> list[Path]:
    files: list[Path] = []
    for glob in PLUGIN_GLOBS:
        files.extend(ROOT.glob(glob))
    return sorted({p.resolve() for p in files if p.is_file()})


def strip_cfg_test_regions(src: str) -> str:
    """Replace `#[cfg(test)]` modules and item blocks with spaces (keep newlines)."""
    out = list(src)
    i = 0
    marker = "#[cfg(test)]"
    while True:
        idx = src.find(marker, i)
        if idx < 0:
            break
        # Find the next `{` after the attribute (mod tests { ... } or fn ... {).
        brace = src.find("{", idx)
        if brace < 0:
            break
        depth = 0
        j = brace
        while j < len(src):
            ch = src[j]
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0:
                    j += 1
                    break
            j += 1
        for k in range(idx, min(j, len(src))):
            if out[k] != "\n":
                out[k] = " "
        i = j
    return "".join(out)


def strip_comments(src: str) -> str:
    """Blank line/block comments while preserving newlines."""
    out: list[str] = []
    i = 0
    n = len(src)
    while i < n:
        if src.startswith("//", i):
            while i < n and src[i] != "\n":
                out.append(" ")
                i += 1
            continue
        if src.startswith("/*", i):
            while i + 1 < n and not src.startswith("*/", i):
                out.append("\n" if src[i] == "\n" else " ")
                i += 1
            out.extend("  ")
            i = min(i + 2, n)
            continue
        out.append(src[i])
        i += 1
    return "".join(out)


def check_file(path: Path) -> list[str]:
    rel = path.relative_to(ROOT).as_posix()
    src = path.read_text(encoding="utf-8")
    scanned = strip_comments(strip_cfg_test_regions(src))
    hits: list[str] = []
    for rx in FORBIDDEN_IMPORTS:
        for m in rx.finditer(scanned):
            line = scanned.count("\n", 0, m.start()) + 1
            hits.append(
                f"{rel}:{line}: forbidden host/domain symbol `{m.group(0)}` in database plugin"
            )
    for m in TABLE_RE.finditer(scanned):
        line = scanned.count("\n", 0, m.start()) + 1
        hits.append(f"{rel}:{line}: application table identifier `{m.group(1)}`")
    for m in JSON_EACH_RE.finditer(scanned):
        line = scanned.count("\n", 0, m.start()) + 1
        hits.append(f"{rel}:{line}: guest SQL must not use json_each (host filters in Rust)")
    return hits


def main() -> int:
    files = iter_plugin_sources()
    if not files:
        print("check-db-plugin-isolation: no database plugin sources found", file=sys.stderr)
        return 2
    hits: list[str] = []
    for path in files:
        hits.extend(check_file(path))
    hits.extend(check_cargo_metadata())
    if hits:
        print("Database plugin isolation violations:", file=sys.stderr)
        for h in hits:
            print(f"  {h}", file=sys.stderr)
        print(
            "Guests must execute host-authored plans only; host selects schema versions. "
            "See docs/adr/sql-database-contract.md",
            file=sys.stderr,
        )
        return 1
    print(f"ok: {len(files)} database plugin sources + cargo metadata")
    return 0


if __name__ == "__main__":
    sys.exit(main())
