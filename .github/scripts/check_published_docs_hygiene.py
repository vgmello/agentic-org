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
    r"(?<![A-Za-z0-9_])(?:(?:(?i:task|adr|lnx|doc)|FC)-\d+[A-Za-z]?)(?![A-Za-z0-9_])"
)
INTERNAL_PATH_RE = re.compile(r"(?i)backlog/(?:tasks|docs|decisions)/")
MARKDOWN_LINK_RE = re.compile(r"!?\[[^\]]*\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")

EXPLICIT_TARGETS = (
    ".github/workflows/release.yml",
    "deploy/container/Dockerfile",
    "deploy/container/README.md",
    "docs/cli-reference.md",
    "docs/remote-office-setup.md",
)


def collect_script_files(repo_root: Path) -> list[Path]:
    scripts_dir = repo_root / "scripts"
    if not scripts_dir.is_dir():
        raise SystemExit("missing expected scripts/ directory")
    files = [
        p
        for p in sorted(scripts_dir.rglob("*"))
        if "__pycache__" not in p.parts and p.is_file() and p.suffix in (".sh", ".py")
    ]
    if not any(p.suffix == ".sh" for p in files):
        raise SystemExit("no .sh scripts found under scripts/")
    return files


def collect_target_files(repo_root: Path) -> list[Path]:
    files = collect_script_files(repo_root)
    for rel in EXPLICIT_TARGETS:
        path = repo_root / rel
        if not path.is_file():
            raise SystemExit(f"missing expected published file: {rel}")
        files.append(path)
    return files


def check_markdown_link(
    repo_resolved: Path, path: Path, rel: str, lineno: int, raw_target: str
) -> str | None:
    if raw_target.startswith(("http://", "https://", "mailto:", "#")):
        return None
    target_path = raw_target.split("#", 1)[0].split("?", 1)[0]
    if not target_path:
        return None
    resolved = (path.parent / target_path).resolve()
    try:
        resolved.relative_to(repo_resolved)
    except ValueError:
        return f"{rel}:{lineno}: relative markdown link escapes repository `{raw_target}`"
    if not resolved.exists():
        return f"{rel}:{lineno}: broken relative markdown link `{raw_target}`"
    return None


def check_markdown_file_links(
    repo_resolved: Path, path: Path, rel: str, lines: list[str]
) -> list[str]:
    errors: list[str] = []
    in_fence = False
    for lineno, line in enumerate(lines, start=1):
        if line.lstrip().startswith("```"):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        for match in MARKDOWN_LINK_RE.finditer(line):
            err = check_markdown_link(repo_resolved, path, rel, lineno, match.group(1))
            if err:
                errors.append(err)
    return errors


def check_text_hygiene(repo_root: Path, files: list[Path]) -> list[str]:
    errors: list[str] = []
    repo_resolved = repo_root.resolve()
    for path in files:
        rel = path.relative_to(repo_root).as_posix()
        lines = path.read_text(encoding="utf-8").splitlines()
        for lineno, line in enumerate(lines, start=1):
            for match in INTERNAL_ID_RE.finditer(line):
                errors.append(
                    f"{rel}:{lineno}: internal tracking ID `{match.group(0)}` in published file"
                )
            for match in INTERNAL_PATH_RE.finditer(line):
                errors.append(
                    f"{rel}:{lineno}: internal backlog path `{match.group(0)}` in published file"
                )
        if path.suffix == ".md":
            errors.extend(check_markdown_file_links(repo_resolved, path, rel, lines))
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
