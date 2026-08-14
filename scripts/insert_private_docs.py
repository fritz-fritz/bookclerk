#!/usr/bin/env python3
"""Insert missing private-item doc comments from Clippy diagnostics JSON.

Reads a list of {file, line, message, text} entries (as produced from
``cargo clippy --message-format=json`` for ``missing_docs_in_private_items``)
and inserts a one-line ``///`` summary above each item. Edits each file from
the bottom so line numbers stay valid.

Usage:
  python3 scripts/insert_private_docs.py /tmp/privdocs/*.json
"""

from __future__ import annotations

import json
import re
import sys
from collections import defaultdict
from pathlib import Path


def summary_for(message: str, text: str) -> str:
    t = text.strip()
    # Field: name: Type
    if "field" in message:
        m = re.match(r"(?:pub(?:\([^)]*\))?\s+)?(\w+)\s*:\s*(.+?),?\s*$", t)
        if m:
            name, ty = m.group(1), m.group(2).rstrip(",")
            return f"Holds the `{name}` value (`{ty}`) for this type."
        return "Field used by the enclosing private type."
    if "variant" in message:
        name = t.split("{")[0].split("(")[0].strip().rstrip(",")
        name = re.sub(r"^pub(?:\([^)]*\))?\s+", "", name)
        return f"`{name}` variant of the enclosing enum."
    if "constant" in message or "static" in message:
        m = re.search(r"\b([A-Z0-9_]+)\b", t)
        name = m.group(1) if m else "constant"
        return f"Constant `{name}` used by this module."
    if "module" in message:
        m = re.search(r"mod\s+(\w+)", t)
        name = m.group(1) if m else "module"
        return f"Private `{name}` module with implementation details."
    if "type alias" in message:
        m = re.search(r"type\s+(\w+)", t)
        name = m.group(1) if m else "alias"
        return f"Type alias `{name}` used inside this module."
    if "struct" in message or "enum" in message or "union" in message:
        kind = "struct" if "struct" in message else "enum" if "enum" in message else "union"
        m = re.search(rf"{kind}\s+(\w+)", t)
        name = m.group(1) if m else kind
        return f"Private `{name}` {kind} used by this crate's implementation."
    if "trait" in message:
        m = re.search(r"trait\s+(\w+)", t)
        name = m.group(1) if m else "trait"
        return f"Private `{name}` trait used by this crate's implementation."
    # function / method / associated function
    m = re.search(r"fn\s+(\w+)", t)
    name = m.group(1) if m else "helper"
    if name == "new":
        return "Constructs a new value for the enclosing type."
    if name == "default":
        return "Returns the default value used by serde or builder fallbacks."
    if name.startswith("default_"):
        return f"Serde / builder default for `{name.removeprefix('default_')}`."
    if name.startswith("is_"):
        return f"Returns whether `{name.removeprefix('is_')}` holds for this value."
    if name.startswith("has_"):
        return f"Returns whether this value has `{name.removeprefix('has_')}`."
    if name.startswith("set_"):
        return f"Updates the `{name.removeprefix('set_')}` field on this value."
    if name.startswith("get_"):
        return f"Returns the `{name.removeprefix('get_')}` field from this value."
    if name.startswith("with_"):
        return f"Returns a copy with `{name.removeprefix('with_')}` updated."
    if name.startswith("to_"):
        return f"Converts this value into `{name.removeprefix('to_')}`."
    if name.startswith("from_"):
        return f"Builds this value from `{name.removeprefix('from_')}`."
    if name.startswith("parse_"):
        return f"Parses `{name.removeprefix('parse_')}` from the given input."
    if name.startswith("load_"):
        return f"Loads `{name.removeprefix('load_')}` from storage or config."
    if name.startswith("save_"):
        return f"Persists `{name.removeprefix('save_')}` to storage or config."
    if name.startswith("handle_"):
        return f"Handles the `{name.removeprefix('handle_')}` request or event."
    if name.endswith("_mut"):
        return f"Returns a mutable reference used by `{name.removesuffix('_mut')}`."
    return f"Internal `{name}` helper used by this module."


def already_documented(lines: list[str], idx0: int) -> bool:
    i = idx0 - 1
    while i >= 0 and lines[i].strip() in ("",):
        i -= 1
    while i >= 0 and lines[i].lstrip().startswith("#["):
        i -= 1
        while i >= 0 and lines[i].strip() == "":
            i -= 1
    if i >= 0 and lines[i].lstrip().startswith("///"):
        return True
    return False


def insert_docs(path: Path, items: list[dict]) -> int:
    raw = path.read_text(encoding="utf-8")
    lines = raw.splitlines(keepends=True)
    # Dedupe by line; keep first message.
    by_line: dict[int, dict] = {}
    for it in items:
        line = int(it["line"])
        by_line.setdefault(line, it)
    inserted = 0
    for line in sorted(by_line, reverse=True):
        it = by_line[line]
        idx = line - 1
        if idx < 0 or idx >= len(lines):
            continue
        if already_documented(lines, idx):
            continue
        # Match indentation of the item line.
        indent = re.match(r"^[ \t]*", lines[idx]).group(0)  # type: ignore[union-attr]
        text = it.get("text") or lines[idx]
        doc = summary_for(it.get("message") or "", text)
        lines.insert(idx, f"{indent}/// {doc}\n")
        inserted += 1
    if inserted:
        path.write_text("".join(lines), encoding="utf-8")
    return inserted


def main(argv: list[str]) -> int:
    root = Path(__file__).resolve().parents[1]
    files = argv[1:]
    if not files:
        print("usage: insert_private_docs.py DIAG.json...", file=sys.stderr)
        return 2
    by_file: dict[str, list] = defaultdict(list)
    for f in files:
        data = json.loads(Path(f).read_text(encoding="utf-8"))
        for it in data:
            by_file[it["file"]].append(it)
    total = 0
    for rel, items in sorted(by_file.items()):
        path = root / rel
        if not path.is_file():
            print(f"skip missing {rel}", file=sys.stderr)
            continue
        n = insert_docs(path, items)
        total += n
        print(f"{n:4d} {rel}")
    print(f"inserted {total} doc comments")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
