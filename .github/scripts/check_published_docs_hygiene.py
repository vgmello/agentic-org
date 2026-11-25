#!/usr/bin/env python3
"""CI hygiene guard for published user-facing scripts and operator docs.

Ensures published release/deploy scripts and operator documentation:
1. Do not leak internal tracking IDs (TASK-*, ADR-*, FC-*, LNX-*, doc-*).
2. Do not contain broken repo-relative links to internal ADR/design directories.
3. Pass `bash -n` syntax checks (and `shellcheck` when `--require-shellcheck` or present).
"""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
from pathlib import Path

INTERNAL_ID_RE = re.compile(
    r"(?<![A-Za-z0-9_])(?:(?:TASK|ADR|FC|LNX)-\d+|doc-\d+)(?![A-Za-z0-9_])"
)
BROKEN_INTERNAL_LINK_RE = re.compile(r"\.\./\.\./docs/(?:adr|design)/")

EXPLICIT_TARGETS = (
    ".github/workflows/release.yml",
    "deploy/container/Dockerfile",
    "deploy/container/README.md",
    "docs/cli-reference.md",
    "docs/remote-office-setup.md",
)


def collect_target_files(repo_root: Path) -> list[Path]:
    files: list[Path] = []
    scripts_dir = repo_root / "scripts"
    if scripts_dir.is_dir():
        for path in sorted(scripts_dir.iterdir()):
            if path.is_file() and path.suffix in (".sh", ".py"):
                files.append(path)
    for rel in EXPLICIT_TARGETS:
        path = repo_root / rel
        if path.is_file():
            files.append(path)
        else:
            raise SystemExit(f"missing expected published file: {rel}")
    return files


def check_text_hygiene(repo_root: Path, files: list[Path]) -> list[str]:
    errors: list[str] = []
    for path in files:
        rel = path.relative_to(repo_root).as_posix()
        for lineno, line in enumerate(
            path.read_text(encoding="utf-8").splitlines(), start=1
        ):
            for match in INTERNAL_ID_RE.finditer(line):
                errors.append(
                    f"{rel}:{lineno}: internal tracking ID `{match.group(0)}` in published file"
                )
            for match in BROKEN_INTERNAL_LINK_RE.finditer(line):
                errors.append(
                    f"{rel}:{lineno}: broken internal docs link `{match.group(0)}`"
                )
    return errors


def check_shell_scripts(
    repo_root: Path, files: list[Path], require_shellcheck: bool
) -> list[str]:
    errors: list[str] = []
    shell_files = [p for p in files if p.suffix == ".sh"]
    if not shell_files:
        return errors
    for path in shell_files:
        rel = path.relative_to(repo_root).as_posix()
        proc = subprocess.run(
            ["bash", "-n", str(path)],
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        if proc.returncode != 0:
            errors.append(f"{rel}: bash -n failed: {proc.stderr.strip()}")
    shellcheck = shutil.which("shellcheck")
    if shellcheck is None:
        if require_shellcheck:
            errors.append("shellcheck not found on PATH (--require-shellcheck)")
    else:
        proc = subprocess.run(
            [shellcheck, *(str(p) for p in shell_files)],
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        if proc.returncode != 0:
            output = (proc.stdout + "\n" + proc.stderr).strip()
            errors.append(f"shellcheck failed:\n{output}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Verify hygiene and shell lint on published scripts and operator docs."
    )
    parser.add_argument(
        "--require-shellcheck",
        action="store_true",
        help="Fail if shellcheck is not installed on PATH.",
    )
    args = parser.parse_args()
    repo_root = Path(__file__).resolve().parents[2]
    files = collect_target_files(repo_root)
    errors = check_text_hygiene(repo_root, files)
    errors.extend(check_shell_scripts(repo_root, files, args.require_shellcheck))
    if errors:
        for err in errors:
            print(err, file=sys.stderr)
        return 1
    print(f"Published hygiene OK ({len(files)} files checked).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
