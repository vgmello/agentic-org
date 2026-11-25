#!/usr/bin/env python3
"""Verify that every Cargo workspace member inherits `[lints] workspace = true`.

Operates on this repository's trusted workspace checkout (like `cargo fmt`,
`cargo clippy`, and `crates/org-platform/tests/libc_only.rs`). Source-level
`allow(...)` reason and `write_volatile` confinement checks live in
`crates/org-platform/tests/rust_hygiene.rs` using `syn` and `proc-macro2`.
"""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys

try:
    import tomllib
except ModuleNotFoundError:  # Python 3.10 with pinned `tomli`
    try:
        import tomli as tomllib
    except ModuleNotFoundError:
        tomllib = None

REPO_ROOT = Path(__file__).resolve().parents[3]


def manifest_inherits_workspace_lints(manifest_text: str) -> bool:
    if tomllib is None:
        raise RuntimeError(
            "Python 3.11+ (stdlib tomllib) or `tomli` on Python 3.10 is required to parse Cargo.toml"
        )
    data = tomllib.loads(manifest_text)
    lints = data.get("lints", {})
    return isinstance(lints, dict) and lints.get("workspace") is True


def workspace_member_manifests(repo_root: Path = REPO_ROOT) -> list[Path]:
    manifest_path = repo_root / "Cargo.toml"
    proc = subprocess.run(
        [
            "cargo",
            "metadata",
            "--no-deps",
            "--offline",
            "--format-version",
            "1",
            "--manifest-path",
            str(manifest_path),
        ],
        cwd=repo_root,
        capture_output=True,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        detail = (proc.stderr or proc.stdout).strip().splitlines()
        summary = detail[-1] if detail else f"exit {proc.returncode}"
        raise RuntimeError(f"cargo metadata failed: {summary}")
    meta = json.loads(proc.stdout)
    member_ids = set(meta.get("workspace_members", []))
    return sorted(
        {
            Path(pkg["manifest_path"])
            for pkg in meta.get("packages", [])
            if pkg.get("id") in member_ids and pkg.get("manifest_path")
        }
    )


def check_workspace_lints(repo_root: Path = REPO_ROOT) -> list[str]:
    if tomllib is None:
        return [
            "Cargo.toml:1: unreadable-manifest: "
            "Python 3.11+ (stdlib tomllib) or `tomli` on Python 3.10 is required to parse Cargo.toml"
        ]
    try:
        manifests = workspace_member_manifests(repo_root)
    except (OSError, ValueError, RuntimeError) as exc:
        return [f"Cargo.toml:1: unreadable-manifest: {exc}"]
    if not manifests:
        return ["Cargo.toml:1: empty-workspace: cargo metadata returned no workspace members"]

    repo_resolved = repo_root.resolve()
    findings: list[str] = []
    for manifest in manifests:
        try:
            rel = str(manifest.resolve().relative_to(repo_resolved))
        except (OSError, ValueError):
            findings.append(
                f"{manifest}:1: unsafe-path: member manifest resolves outside repository root"
            )
            continue
        try:
            inherited = manifest_inherits_workspace_lints(manifest.read_text(encoding="utf-8"))
        except (OSError, UnicodeDecodeError, ValueError, RuntimeError) as exc:
            findings.append(f"{rel}:1: unreadable-manifest: {exc}")
            continue
        if not inherited:
            findings.append(
                f"{rel}:1: missing-workspace-lints: "
                "workspace member must set `[lints] workspace = true`"
            )
    return findings


def main() -> int:
    findings = check_workspace_lints()
    if findings:
        for item in findings:
            print(item, file=sys.stderr)
        print(f"Rust workspace lint check failed ({len(findings)} finding(s)).", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
