#!/usr/bin/env python3
"""Sync ABI projections from crates/bookclerk-plugin-abi/schema/abi.json.

- Ensures packages/plugin-sdk/src/generated.ts lists every schema method
- Ensures packages/plugin-sdk-python/.../abi.py ``METHOD_NAMES`` match schema
  (preserves Google-style docs / TypedDicts; ``--write`` rewrites only that
  tuple, or scaffolds a stub file when missing)
- Ensures plugin-toml.json copies match between abi and manifest crates
- Ensures methods.rs METHOD_NAMES match abi.json methods keys (--check)
- Ensures fixtures/wire/*.json object keys are camelCase (no `_` in keys)

Exit 1 on drift when --check (default).
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
ABI = ROOT / "crates/bookclerk-plugin-abi/schema/abi.json"
METHODS_RS = ROOT / "crates/bookclerk-plugin-abi/src/methods.rs"
ABI_LIB_RS = ROOT / "crates/bookclerk-plugin-abi/src/lib.rs"
DB_EXECUTE_RS = ROOT / "crates/bookclerk-plugin-abi/src/db_execute.rs"
CAPNP_SCHEMAS = (
    ROOT / "crates/bookclerk-plugin-abi/schema/plugin_v2.capnp",
    ROOT / "crates/bookclerk-plugin-abi/schema/plugin_v2_host.capnp",
)
TS_GENERATED = ROOT / "packages/plugin-sdk/src/generated.ts"
TS_DB_EXECUTE = ROOT / "packages/plugin-sdk/src/db-execute.ts"
PY_ABI = ROOT / "packages/plugin-sdk-python/src/bookclerk_plugin_sdk/abi.py"
PY_DB_VALUE = ROOT / "packages/plugin-sdk-python/src/bookclerk_plugin_sdk/db_value.py"
PLUGIN_TOML_SCHEMA = ROOT / "crates/bookclerk-plugin-abi/schema/plugin-toml.json"
MANIFEST_SCHEMA = ROOT / "crates/bookclerk-plugin-manifest/schema/plugin-toml.json"
WIRE_FIXTURES = ROOT / "crates/bookclerk-plugin-abi/fixtures/wire"

REQUIRED_WIRE_FIXTURES = (
    "login.request.json",
    "login.result.json",
    "scan.request.json",
    "scan.result.json",
    "fetchTitle.request.json",
    "put.s3.request.json",
)

# Matches `METHOD_NAMES: tuple[str, ...] = ( ... )` including a trailing comma.
METHOD_NAMES_RE = re.compile(
    r"METHOD_NAMES:\s*tuple\[str,\s*\.\.\.\]\s*=\s*\((.*?)\)\s*\n",
    re.DOTALL,
)


def method_names_from_schema() -> list[str]:
    schema = json.loads(ABI.read_text(encoding="utf-8"))
    methods = schema["properties"]["methods"]["properties"]
    return list(methods.keys())


def method_names_from_methods_rs() -> list[str]:
    """Parse `pub const NAME: &str = "..."` entries in methods.rs modules."""
    text = METHODS_RS.read_text(encoding="utf-8")
    # Prefer METHOD_NAMES array expansion via individual NAME constants.
    names = re.findall(r'pub const NAME: &str = "([^"]+)";', text)
    if not names:
        raise SystemExit(f"no NAME constants found in {METHODS_RS}")
    return names


def method_names_tuple_body(names: list[str]) -> str:
    body = ",\n".join(f'    "{n}"' for n in names)
    return f"(\n{body},\n)"


def method_names_from_py(text: str) -> list[str] | None:
    """Return METHOD_NAMES entries from abi.py, or None if the binding is absent."""
    match = METHOD_NAMES_RE.search(text)
    if not match:
        return None
    return re.findall(r'"([^"]+)"', match.group(1))


def render_py_stub(names: list[str]) -> str:
    """Minimal abi.py used only when the file is missing."""
    return f'''"""ABI constants — keep aligned with crates/bookclerk-plugin-abi/schema/abi.json."""

from __future__ import annotations

API_VERSION: int = 1

METHOD_NAMES: tuple[str, ...] = {method_names_tuple_body(names)}
'''


def sync_py_method_names(text: str, names: list[str]) -> str:
    """Replace the METHOD_NAMES tuple in place; preserve surrounding docs/types."""
    replacement = f"METHOD_NAMES: tuple[str, ...] = {method_names_tuple_body(names)}\n"
    if not METHOD_NAMES_RE.search(text):
        raise SystemExit(f"METHOD_NAMES binding not found in {PY_ABI}")
    return METHOD_NAMES_RE.sub(replacement, text, count=1)


def collect_snake_keys(value: Any, path: str = "$") -> list[str]:
    """Return paths of object keys that contain `_` (not camelCase)."""
    bad: list[str] = []
    if isinstance(value, dict):
        for key, child in value.items():
            here = f"{path}.{key}"
            if "_" in key:
                bad.append(here)
            bad.extend(collect_snake_keys(child, here))
    elif isinstance(value, list):
        for i, child in enumerate(value):
            bad.extend(collect_snake_keys(child, f"{path}[{i}]"))
    return bad


def check_wire_fixtures() -> list[str]:
    """Validate golden wire fixtures exist and use camelCase object keys."""
    errors: list[str] = []
    if not WIRE_FIXTURES.is_dir():
        return [f"missing wire fixtures dir: {WIRE_FIXTURES}"]
    present = {p.name for p in WIRE_FIXTURES.glob("*.json")}
    for name in REQUIRED_WIRE_FIXTURES:
        if name not in present:
            errors.append(f"missing required wire fixture: {name}")
    for path in sorted(WIRE_FIXTURES.glob("*.json")):
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
        except json.JSONDecodeError as exc:
            errors.append(f"{path.name}: invalid JSON ({exc})")
            continue
        for bad in collect_snake_keys(data):
            errors.append(f"{path.name}: non-camelCase key at {bad}")
        # Spot-check a few multi-word fields against abi.json $defs naming.
        if path.name == "login.request.json" and "pluginDataDir" not in data:
            errors.append(f"{path.name}: expected pluginDataDir")
        if path.name == "put.s3.request.json" and "forcePathStyle" not in data:
            errors.append(f"{path.name}: expected forcePathStyle")
    return errors


# Legacy host-IR types must not be re-exported from the public plugin ABI crate root.
FORBIDDEN_ABI_LIB_EXPORTS = (
    "DbAtomicRequest",
    "DbAtomicPlan",
    "DbPlanExecResult",
    "DbPlanStatement",
    "StatementDto",
    "QueryResultDto",
    "DB_ATOMIC_SENTINEL",
    "DB_CAPABILITIES_SENTINEL",
    "GuestReceiptPersist",
    "HostExecuteEnvelope",
    "sea_null",
)

FORBIDDEN_ABI_SCHEMA_DEFS = (
    "DbAtomicRequest",
    "DbAtomicPlan",
    "DbPlanExecResult",
    "DbPlanStatement",
    "DbPlanStmtExecResult",
    "StatementDto",
    "QueryResultDto",
)


def check_abi_schema_defs() -> list[str]:
    """Fail when host planner IR reappears in abi.json `$defs`."""
    data = json.loads(ABI.read_text(encoding="utf-8"))
    defs = data.get("$defs", {})
    errors: list[str] = []
    for name in FORBIDDEN_ABI_SCHEMA_DEFS:
        if name in defs:
            errors.append(f"abi.json must not define legacy host IR `{name}`")
    return errors


def statement_kinds_rust() -> list[str]:
    """Wire names of `DbPlanStatementKind` variants (camelCase serde)."""
    text = DB_EXECUTE_RS.read_text(encoding="utf-8")
    match = re.search(r"pub enum DbPlanStatementKind \{(.*?)\n\}", text, re.DOTALL)
    if not match:
        raise SystemExit(f"DbPlanStatementKind not found in {DB_EXECUTE_RS}")
    variants = re.findall(r"^\s{4}([A-Z][A-Za-z0-9]*),", match.group(1), re.MULTILINE)
    return [v[0].lower() + v[1:] for v in variants]


def statement_kinds_capnp() -> list[str]:
    """Ordinal-ordered enumerants of the Cap'n statement-kind enum."""
    text = CAPNP_SCHEMAS[0].read_text(encoding="utf-8")
    match = re.search(r"enum Db(?:Plan)?StatementKind \{(.*?)\}", text, re.DOTALL)
    if not match:
        raise SystemExit(f"DbStatementKind enum not found in {CAPNP_SCHEMAS[0]}")
    entries = re.findall(r"(\w+)\s*@(\d+);", match.group(1))
    entries.sort(key=lambda e: int(e[1]))
    return [name for name, _ in entries]


def statement_kinds_ts() -> list[str]:
    """`KIND_FROM` ordinal decode table in the TS SDK."""
    text = TS_DB_EXECUTE.read_text(encoding="utf-8")
    match = re.search(r"const KIND_FROM = \[(.*?)\]", text, re.DOTALL)
    if not match:
        raise SystemExit(f"KIND_FROM not found in {TS_DB_EXECUTE}")
    return re.findall(r'"([^"]+)"', match.group(1))


def statement_kinds_py() -> list[str]:
    """`_KIND_FROM` ordinal decode table in the Python SDK."""
    text = PY_DB_VALUE.read_text(encoding="utf-8")
    match = re.search(r"_KIND_FROM = \((.*?)\)", text, re.DOTALL)
    if not match:
        raise SystemExit(f"_KIND_FROM not found in {PY_DB_VALUE}")
    return re.findall(r'"([^"]+)"', match.group(1))


def check_statement_kinds() -> list[str]:
    """Statement kinds must agree, in order, across Rust / Cap'n / TS / Python."""
    rust = statement_kinds_rust()
    capnp = statement_kinds_capnp()
    ts = statement_kinds_ts()
    py = statement_kinds_py()
    errors: list[str] = []
    for label, kinds in (("capnp", capnp), ("ts", ts), ("python", py)):
        if kinds != rust:
            errors.append(f"statement kinds drift rust={rust} {label}={kinds}")
    for label, kinds in (("rust", rust), ("capnp", capnp), ("ts", ts), ("python", py)):
        if "query" in kinds:
            errors.append(f'legacy "query" statement kind must not reappear in {label}')
    return errors


# JSON-era database RPC names and DTOs deleted from the public ABI; none may
# reappear in schemas or generated SDK sources.
LEGACY_DB_TOKENS = (
    "StatementDto",
    "QueryResultDto",
    "valuesJson",
    "rowsJson",
    "dbConnect",
    "dbQuery",
    "dbExecute",
    "dbBegin",
    "dbCommit",
    "dbRollback",
    "dbAtomic",
    "DbConnectResult",
    "DbConnectParams",
)


def check_legacy_db_tokens() -> list[str]:
    """Fail when deleted JSON-era database names reappear in public artifacts."""
    targets = (
        *CAPNP_SCHEMAS,
        ABI,
        TS_GENERATED,
        TS_DB_EXECUTE,
        PY_ABI,
        PY_DB_VALUE,
    )
    errors: list[str] = []
    for path in targets:
        if not path.is_file():
            errors.append(f"missing legacy-token scan target: {path}")
            continue
        text = path.read_text(encoding="utf-8")
        for token in LEGACY_DB_TOKENS:
            if re.search(rf"\b{token}\b", text):
                errors.append(
                    f"{path.relative_to(ROOT)}: legacy `{token}` must not reappear"
                )
    return errors


def check_abi_lib_exports() -> list[str]:
    """Fail when removed legacy database DTOs reappear in lib.rs `pub use`."""
    text = ABI_LIB_RS.read_text(encoding="utf-8")
    # `#[cfg(feature = "host")]`-gated re-exports are host-private surface,
    # invisible to plugin authors on default features — not public ABI.
    text = re.sub(
        r'#\[cfg\(feature = "host"\)\]\s*\npub use [^;]*;', "", text
    )
    errors: list[str] = []
    for name in FORBIDDEN_ABI_LIB_EXPORTS:
        if re.search(rf"\bpub use\b[^;]*\b{name}\b", text):
            errors.append(f"lib.rs must not publicly export legacy `{name}`")
        if re.search(rf"^\s*pub use db::\{{[^}}]*\b{name}\b", text, re.MULTILINE):
            errors.append(f"lib.rs must not publicly export legacy `{name}` from db::")
    if "from_atomic" in text or "into_plan_exec" in text:
        errors.append("lib.rs must not document legacy ExecuteRequest bridges")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    if not args.write:
        args.check = True

    names = method_names_from_schema()
    rs_names = method_names_from_methods_rs()
    drift = False

    if names != rs_names:
        print(
            "abi.json methods keys ≠ methods.rs NAME constants:\n"
            f"  schema only: {sorted(set(names) - set(rs_names))}\n"
            f"  methods.rs only: {sorted(set(rs_names) - set(names))}\n"
            f"  order mismatch: {names != rs_names}",
            file=sys.stderr,
        )
        if set(names) != set(rs_names) or names != rs_names:
            drift = True

    if MANIFEST_SCHEMA.read_text(encoding="utf-8") != PLUGIN_TOML_SCHEMA.read_text(
        encoding="utf-8"
    ):
        print("plugin-toml.json differs between abi and manifest crates", file=sys.stderr)
        drift = True
        if args.write:
            PLUGIN_TOML_SCHEMA.write_text(
                MANIFEST_SCHEMA.read_text(encoding="utf-8"), encoding="utf-8"
            )
            print(f"synced {PLUGIN_TOML_SCHEMA}")
            drift = False

    ts = TS_GENERATED.read_text(encoding="utf-8")
    missing = [n for n in names if f'"{n}"' not in ts]
    if missing:
        print(f"generated.ts missing methods: {missing}", file=sys.stderr)
        drift = True

    py_current = PY_ABI.read_text(encoding="utf-8") if PY_ABI.is_file() else ""
    py_names = method_names_from_py(py_current) if py_current else None
    if py_names != names:
        if args.write:
            if not py_current:
                PY_ABI.write_text(render_py_stub(names), encoding="utf-8")
            else:
                PY_ABI.write_text(
                    sync_py_method_names(py_current, names), encoding="utf-8"
                )
            print(f"wrote METHOD_NAMES in {PY_ABI}")
        else:
            detail = (
                "missing METHOD_NAMES binding"
                if py_names is None
                else (
                    f"schema only: {sorted(set(names) - set(py_names))}; "
                    f"abi.py only: {sorted(set(py_names) - set(names))}; "
                    f"order mismatch: {py_names != names}"
                )
            )
            print(f"python abi.py METHOD_NAMES drift ({detail})", file=sys.stderr)
            drift = True

    # Wire fixtures are not auto-fixable; fail even under `--write`.
    wire_errors = check_wire_fixtures()
    if wire_errors:
        for err in wire_errors:
            print(f"wire fixtures: {err}", file=sys.stderr)

    export_errors = check_abi_lib_exports()
    if export_errors:
        for err in export_errors:
            print(f"abi exports: {err}", file=sys.stderr)

    schema_errors = check_abi_schema_defs()
    if schema_errors:
        for err in schema_errors:
            print(f"abi schema: {err}", file=sys.stderr)

    kind_errors = check_statement_kinds()
    if kind_errors:
        for err in kind_errors:
            print(f"statement kinds: {err}", file=sys.stderr)

    legacy_errors = check_legacy_db_tokens()
    if legacy_errors:
        for err in legacy_errors:
            print(f"legacy names: {err}", file=sys.stderr)

    if wire_errors or export_errors or schema_errors or kind_errors or legacy_errors:
        return 1
    if drift and args.check and not args.write:
        return 1
    print(
        f"ok methods={len(names)} wire_fixtures={len(REQUIRED_WIRE_FIXTURES)} "
        f"abi_export_guard={len(FORBIDDEN_ABI_LIB_EXPORTS)} "
        f"abi_schema_guard={len(FORBIDDEN_ABI_SCHEMA_DEFS)} "
        f"statement_kinds={len(statement_kinds_rust())} "
        f"legacy_name_guard={len(LEGACY_DB_TOKENS)}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
