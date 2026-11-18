#!/usr/bin/env python3
"""The platform-conditionals list only shrinks (ADR-0017 LNX-5).

Compares .github/platform-conditionals.allow at a base commit with the working tree's
and fails when the working tree's has a path the base's lacks, or a higher count for a
path. The check in crates/org-platform/tests/platform_conditionals.rs keeps the list
equal to the sources; this keeps a change from adding a platform predicate outside
crates/org-platform by adding it to the list as well.

Usage: platform_allowlist_shrinks.py <base commit>
A base commit without the list passes: the change that creates the list has no earlier
one to compare with.
"""
import subprocess
import sys

LIST = ".github/platform-conditionals.allow"


def entries(text):
    found = {}
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        path, _, count = line.rpartition(" ")
        found[path] = int(count)
    return found


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    base = sys.argv[1]
    shown = subprocess.run(
        ["git", "show", f"{base}:{LIST}"], capture_output=True, text=True, check=False
    )
    if shown.returncode != 0:
        exists = subprocess.run(
            ["git", "cat-file", "-e", f"{base}^{{commit}}"], capture_output=True, check=False
        )
        if exists.returncode != 0:
            sys.exit(f"base commit {base} is not available: {shown.stderr.strip()}")
        print(f"{LIST} does not exist at {base}; nothing to compare with")
        return
    before = entries(shown.stdout)
    with open(LIST, encoding="utf-8") as current:
        after = entries(current.read())
    problems = []
    for path, count in sorted(after.items()):
        if path not in before:
            problems.append(f"{path}: added to the list ({count})")
        elif count > before[path]:
            problems.append(f"{path}: count raised from {before[path]} to {count}")
    if problems:
        print(f"{LIST} only shrinks. Compared with {base}:")
        for problem in problems:
            print(f"  {problem}")
        print(
            "A new operating-system difference goes into crates/org-platform, behind a "
            "contract (docs/design/linux-host.md, section 1)."
        )
        sys.exit(1)
    files, predicates = len(after), sum(after.values())
    print(f"{LIST}: {files} files, {predicates} predicates; nothing added since {base}")


if __name__ == "__main__":
    main()
