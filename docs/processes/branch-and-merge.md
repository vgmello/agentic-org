---
process: branch-and-merge     # the match key for overrides, unique per level and scope (the doc's own id, doc-<ULID>, is assigned when it's saved through the tool)
name: Branch, review and merge
kind: process
level: organization          # organization | team | project | agent (private)
version: 1
owner: ada (Staff Engineer)
status: active
summary: One open branch per agent; build, check, your own review, the reviewer's READY, the lead's go, merge into the release branch, clean up.
applies_to: every agent that changes a repository
overridable: true             # a team or project may override this process, or add its own
---

> Kept in this repository (`docs/processes/`) until the organization's handbook repository exists (Processes, `docs/specs/processes-ux.md`). Here the release branch is `v1` and the lead is ada.

# Branch, review and merge

How every change reaches the release branch. Follow it from the moment you start work.

## 1. One branch at a time
- You have exactly **one open working branch**. Don't start new work, open another branch or stack branches until that branch is merged into the release branch.
- Start every branch **from the release branch**: `git fetch origin && git switch -c <you>/<topic> origin/<release>`, in your own worktree when the repository's rules say so (`git worktree add ../<repo>-<you> -b <you>/<topic> origin/<release>`). Never branch in a checkout another agent uses.
- Keep it **small**: one reviewable piece. Merge often; don't let work pile up.

## 2. Build and check
- Commit in small steps; every commit builds.
- Keep the branch current: merge `origin/<release>` into it regularly (at least daily).
- Before asking for review, all of the project's checks pass (format, lint, tests; for UI work also the UI checks and end-to-end tests).

## 3. Your own review
- Run a **fresh, independent review** of the whole branch diff (a reviewer that gets the diff and the requirements, not your conclusions).
- Verify each finding yourself; fix the confirmed ones; note anything you deliberately leave.

## 4. Independent review
- Send the **exact commit** to the team's reviewer with: the repository, the branch, the base, the commit, and the design or spec it implements.
- The reviewer answers **READY** or **CHANGES NEEDED** for that exact commit. Fix, then ask again. Any later commit needs a new verdict.
- **If the reviewer doesn't respond within 30 minutes**, tell your lead; the lead spins up another reviewer so work never stalls.

## 5. The lead's go
- Ask your lead for the go with: the branch, the commit, the reviewer's READY, and the last lines of every check.
- The lead checks that the change fits the design and the contracts.

## 6. Merge
- After the go: `git merge --no-ff` into the release branch and push it (or the lead merges and pushes for you, if your session can't push).
- Contract changes (shared protocol crates) go through their owner, as small additive changes.

## 7. Clean up
- After the merge, the merged branch and its worktree are deleted. Your lead confirms the branch is fully in the release branch before anything is deleted; nothing unmerged is ever deleted (closed unmerged work is archived as a tag first).
- Then start your next branch from the release branch (back to step 1).

## When things go wrong
- **The pipeline stalls** (the reviewer or the messaging relay is down): tell your lead at once.
- **The release branch is far behind:** the lead may integrate pending branches centrally (an interim rule), with reviews following as fix-ups.
