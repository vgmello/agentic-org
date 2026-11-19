#!/usr/bin/env python3
"""The platform-conditionals list only shrinks (ADR-0017 LNX-5).

Compares .github/platform-conditionals.allow at a base commit with the working tree's
and fails when a listed path's count is higher than at the base, or when the total is
higher. A new path is accepted only while the total does not rise: that is a listed
file renamed or split, its predicates moved and none added. The check in crates/org-platform/tests/platform_conditionals.rs keeps the list
equal to the sources; this keeps a change from adding a platform predicate outside
crates/org-platform by adding it to the list as well.

Usage: platform_allowlist_shrinks.py <base commit>
A base commit without the list passes: the change that creates the list has no earlier
one to compare with.
"""
import re
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
    if not re.fullmatch(r"[0-9a-f]{7,64}", base):
        sys.exit(f"the base must be a commit id in hexadecimal, not {base!r}")
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
    added = []
    for path, count in sorted(after.items()):
        if path not in before:
            added.append(f"{path}: added to the list ({count})")
        elif count > before[path]:
            problems.append(f"{path}: count raised from {before[path]} to {count}")
    # A listed file that is renamed or split appears as new paths. That is accepted
    # only while the total does not rise: predicates moved, none were added.
    if sum(after.values()) > sum(before.values()):
        problems.extend(added)
        problems.append(
            f"total raised from {sum(before.values())} to {sum(after.values())} predicates"
        )
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
