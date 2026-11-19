#!/usr/bin/env python3
"""Create a local review package for one repository and its current check receipt."""

import argparse
import json
import os
from pathlib import Path
import sys
import uuid

from kit_evidence import git, inspect_receipt, same_sources, snapshot, timestamp
from kit_hooks import protected_name


def package(repo, base, receipt_path):
    before = snapshot(repo)
    if not before["bound"]:
        raise ValueError("A review package requires a readable Git checkout")
    root = Path(before["root"])
    report = inspect_receipt(receipt_path)
    if not report["current"]:
        raise ValueError("Verification evidence is stale/failed: " + "; ".join(report["reasons"]))
    receipt = json.loads(receipt_path.read_text())
    if not any(source.get("root") == str(root) for source in receipt["sources_after"]):
        raise ValueError("Receipt does not cover this checkout")
    resolved = git(root, "rev-parse", "--verify", "--end-of-options", f"{base}^{{commit}}").decode().strip()
    scope = report["scope"]
    if scope != "all" and not scope.get("configuration_changed"):
        # A scoped receipt judged only the files it saw change; every file this review changes must be one.
        wanted_from = git(root, "merge-base", "HEAD", resolved).decode().strip()
        raw = git(root, "diff", "--name-only", "--no-renames", "-z", wanted_from, "--")
        raw += git(root, "ls-files", "--others", "--exclude-standard", "-z")
        wanted = {str(root / os.fsdecode(p)) for p in raw.split(b"\0") if p}
        if not wanted <= set(scope.get("changed", [])):
            raise ValueError(f"Verification was scoped to {scope.get('base')}, which covers less than {base}; "
                             f"rerun kit_hooks.py check --base {base} or --all")
    paths = git(root, "diff", "--no-ext-diff", "--no-textconv", "--no-renames", "--name-only", "-z", resolved, "--")
    changed = [os.fsdecode(p) for p in paths.split(b"\0") if p]
    if any(protected_name(Path(p).name) for p in changed):
        raise ValueError("A changed tracked path matches the secret-file guard; review it separately before packaging")
    diff = git(root, "diff", "--no-ext-diff", "--no-textconv", "--binary", "--no-renames", resolved, "--")
    untracked = [os.fsdecode(p) for p in git(root, "ls-files", "--others", "--exclude-standard", "-z").split(b"\0") if p]
    if not same_sources([before], [snapshot(root)]):
        raise ValueError("Source changed while building the review package; rerun verification")
    # Revalidate all repositories/config in a multi-repository receipt.
    if not inspect_receipt(receipt_path)["current"]:
        raise ValueError("Verification evidence changed while building the review package")
    directory = root / ".agent-kit/reviews" / uuid.uuid4().hex
    for candidate in (directory, *directory.parents):
        if candidate.is_symlink():
            raise ValueError("Review output must not traverse a symlink")
        if candidate == root:
            break
    directory.mkdir(parents=True, mode=0o700)
    metadata = {"version": 1, "created_at": timestamp(), "root": str(root), "base_ref": base,
                "base_commit": resolved, "source": before, "receipt": str(receipt_path.resolve()),
                "verification_current_at_creation": True, "review_verdict": "PENDING",
                "changed_tracked_paths": changed, "untracked_paths": untracked,
                "coverage": "tracked.diff covers committed, staged and unstaged tracked changes against base. "
                            "Read relevant untracked files separately; their contents are not copied."}
    files = {"review.json": (json.dumps(metadata, indent=2) + "\n").encode(),
             "tracked.diff": diff, "verification.json": (json.dumps(receipt, indent=2) + "\n").encode()}
    for name, data in files.items():
        path = directory / name
        with path.open("xb") as output:
            output.write(data)
        path.chmod(0o600)
    return directory


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=Path.cwd())
    parser.add_argument("--base", required=True, help="Agreed task base ref or commit")
    parser.add_argument("--receipt", required=True, type=Path)
    args = parser.parse_args()
    try:
        print(package(args.repo.resolve(), args.base, args.receipt.resolve()))
        return 0
    except (OSError, ValueError, KeyError, TypeError) as exc:
        print(f"Review package failed: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
