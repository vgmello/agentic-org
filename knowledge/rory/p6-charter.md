# P6 charter: org-process write checks and approval (Rory)

**Scope.** docs/design/processes.md §5 (every write checks…, Approval, Version), §2.1-2.2,
decisions 3, 8, 10, 11 and 15 as they bind writes, and Appendix C's "Approval" line. These are pure
functions in crates/org-process: no SQL, git, network or clock, and no new dependencies. The
office gathers the facts (the files at the base, the branch, the caller and its role) and acts on
the answer.

**Tier and size.** Tier A, with each slice at most 250 non-test code lines.

## Checks

1. **`check_write(&WriteInput) -> Result<ProcessFile, WriteRefusal>`** runs on each `doc.new` or
   `doc.save` of kind process and each `process.save`. `WriteInput` carries:
   - `repo` and `path`;
   - the new `text`;
   - the caller (member id, whether it's the owner, whether it's an agent);
   - the resolution inputs at the base (the `Inputs` that `resolve` takes, for the project or team
     context the office supplies);
   - the base version of this file, if it exists;
   - the branch facts, if `branch?` was given: office-made, merged, the file it is for, and the
     member who opened it.

   It checks these in a fixed order, so the first problem found is the one reported:
   1. **Location.** The path is a process location (`location`) whose file name isn't refused.
      Otherwise `invalid_request`.
   2. **Front matter.** The front matter is valid and matches the path (`Location::read`).
      Otherwise `invalid_request` naming the field (PROC-16).
   3. **Own private folder.** An agent writes only its own private folder: an agent-level place
      must be the caller's own. Otherwise `not_authorized`.
   4. **Organization seal.** `overridable: false` at the organization level is only written by the
      owner (decision 3). Otherwise `not_authorized`.
   5. **Private only adds.** A private file has no `overrides`, no `status: off`, and no
      organization key, meaning a key an organization file has in the base inputs. Otherwise
      `invalid_request` with reason `private_only_adds`. Team and project keys are left to
      resolution.
   6. **Unique key.** No other file at that (level, scope) in the base inputs has the key (the file
      being saved is excluded by its path). Otherwise `already_exists`.
   7. **Sealed.** The file doesn't override or turn off a sealed key. **This check reuses
      `resolve` instead of restating rule 3.** It resolves the base inputs with the written file in
      place of its old version. A refused-as-sealed result, a can't-tell included (fail closed),
      gives `invalid_request` with reason `sealed`, naming the sealer or the unreadable path. The
      write path and resolution can't drift apart.
   8. **Version.** The version is the base version + 1, or 1 for a new file. Otherwise
      `invalid_request` naming `version` (§5 Version).
   9. **Branch.** If `branch?` is given, it is office-made, unmerged, for this file and opened by
      the caller. Otherwise `invalid_request`, or `not_authorized` for another member's branch.

   `WriteRefusal { code, reason?, field?, message }` maps 1:1 onto org-proto's `ApiError` codes,
   with the reasons on `ApiError.reason`. org-process doesn't depend on org-proto. The mapping is
   the office's, and the codes are the spec's.

2. **`check_approval(&ApprovalInput) -> Result<(), ApprovalRefusal>`** covers `process.approve`
   and `process.reject`. `ApprovalInput` carries:
   - the caller, and whether it's the owner or the level's approver;
   - the authors of the change (everyone who saved on the branch);
   - the commit the approver saw, and the branch's head now;
   - whether a save came after an earlier approval.

   The checks:
   - **Approver.** The caller must be the level's approver and none of the authors, except the
     owner (decision 11). Otherwise `not_authorized`.
   - **Seen commit.** The seen commit must equal the head. Otherwise `conflict`.
   - **Later save.** A later save voids an approval.

   As in P5, bindings and `forwarded_to_owner` (decision 15) are decided by the office before this
   call.

## Slices

- **P6a** is item 1, checks 1-6 and 8-9.
- **P6b** is item 1, check 7 (the resolve-backed seal check), plus item 2.

Each slice gets one request to Tomas, with evidence and a guard-to-test table (mutants on the
changed lines). Ambiguities are read fail-closed, listed in the request and pinned by tests.

## Open for ada

- **(a) Re-saving a sealed organization file.** Does check 4 also refuse a non-owner's save of an
  organization file that is already sealed and stays sealed (an edit that keeps
  `overridable: false`)? I'd say yes, fail closed: §5 says "sealing … only by the owner", and the
  save writes the seal again.
- **(b) Seals at write time.** Should check 7 judge seals against the integration or default
  branch's inputs, the same source resolution uses (A53)? Or should it include other open docs
  branches? I'd say base only: an unmerged seal doesn't bind yet, and the merge re-resolves.
- **(c) Branch refusal code.** For check 9, should a branch that isn't office-made, or is merged,
  or is for another file, give `invalid_request`, with `not_authorized` only when another member
  opened it? The spec names no code.
