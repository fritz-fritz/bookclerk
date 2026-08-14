#!/usr/bin/env python3
"""Regression: undocumented private items in a binary fail Clippy.

Creates a throwaway Cargo package (not a workspace member) with a private
function lacking docs, runs Clippy with ``missing_docs_in_private_items``
denied, and asserts the build fails. Then documents the item and asserts
Clippy succeeds.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


def run(cmd: list[str], *, cwd: Path, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        cwd=cwd,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )


def main() -> int:
    root = Path(__file__).resolve().parents[2]
    # Use a directory outside the workspace so Cargo does not treat the fixture
    # as a workspace member (TMPDIR may point at <repo>/.tmp).
    with tempfile.TemporaryDirectory(prefix="bookclerk-privdocs-", dir="/tmp") as tmp:
        pkg = Path(tmp) / "privdocs_fixture"
        pkg.mkdir()
        (pkg / "Cargo.toml").write_text(
            """[package]
name = "privdocs_fixture"
version = "0.0.0"
edition = "2021"
publish = false

# Standalone package — must not join the Bookclerk workspace.
[workspace]

[lints.clippy]
missing_docs_in_private_items = "warn"
""",
            encoding="utf-8",
        )
        src = pkg / "src"
        src.mkdir()
        (src / "main.rs").write_text(
            """//! Fixture binary for private-docs lint regression.

fn main() {
    helper();
}

fn helper() {}
""",
            encoding="utf-8",
        )

        env = os.environ.copy()
        # Isolate from the workspace target / RUSTFLAGS=-D warnings noise.
        env["CARGO_TARGET_DIR"] = str(Path(tmp) / "target")
        env.pop("RUSTFLAGS", None)
        env.pop("CARGO_ENCODED_RUSTFLAGS", None)

        deny = run(
            [
                "cargo",
                "clippy",
                "--quiet",
                "--",
                "-D",
                "clippy::missing_docs_in_private_items",
            ],
            cwd=pkg,
            env=env,
        )
        if deny.returncode == 0:
            sys.stderr.write(
                "expected Clippy to reject undocumented private fn; got success\n"
            )
            sys.stderr.write(deny.stdout)
            sys.stderr.write(deny.stderr)
            return 1
        blob = deny.stdout + deny.stderr
        if "missing_docs_in_private_items" not in blob and "missing documentation" not in blob:
            sys.stderr.write("Clippy failed but not for missing private docs:\n")
            sys.stderr.write(blob)
            return 1

        (src / "main.rs").write_text(
            """//! Fixture binary for private-docs lint regression.

fn main() {
    helper();
}

/// Runs the fixture no-op used to prove the lint accepts documented items.
fn helper() {}
""",
            encoding="utf-8",
        )
        ok = run(
            [
                "cargo",
                "clippy",
                "--quiet",
                "--",
                "-D",
                "clippy::missing_docs_in_private_items",
            ],
            cwd=pkg,
            env=env,
        )
        if ok.returncode != 0:
            sys.stderr.write("expected Clippy to accept documented private fn\n")
            sys.stderr.write(ok.stdout)
            sys.stderr.write(ok.stderr)
            return 1

    print("ok: private-docs lint rejects undocumented binary items")
    # Keep workspace root reference so the test is tied to the repo layout.
    _ = root
    _ = shutil
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
