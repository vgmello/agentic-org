# Rory: offline handover (2026-11-13)

On the owner's order (Claude tokens are low, about 3 days offline), Rory stops. Codex agents
continue on `v1-offline`. Everything is on origin; nothing is left only in Rory's container.

## Where each ticket stands

| Ticket | State | Where |
|---|---|---|
| P3 process resolve | merged into v1 (24ce7706) | v1 |
| docs-invisible-set | merged into v1 (32233b2b) | v1 |
| P4a fingerprint + checklist (+ fix) | merged (1d9505f1) | v1 |
| P4b render (+ fence/quote fix) | merged (c98f07df, ca538886) | v1 |
| P5a/P5b one_branch (+ rulings commit) | merged (3ddfc902) | v1 |
| **P6a write checks** | **built, not yet requested** | `rory/p6-write-checks` |
| P6b seal check + approval | not started | charter only |
| P8 gateway §9 org mutations | waits on Kai's P5 ApiBackend | not started |

## P6: org-process write checks (`crates/org-process/src/write.rs`)

The branch is `rory/p6-write-checks`. Its history, oldest first:
- v1 d85df1cf;
- the charter commit a2b7ee52 (`knowledge/rory/p6-charter.md`, approved by ada);
- **P6a**, the commit after the charter;
- this note.

Get the P6a SHA with `git log origin/rory/p6-write-checks`.

**Done in P6a.** `check_write(&WriteInput) -> Result<ProcessFile, WriteRefusal>` runs checks 1–6,
then 8 and 9, in the charter's order. The first failure is the one reported. The checks:
1. location / file name;
2. front matter (`Location::read`);
3. an agent writes only its own private folder;
4. an organization seal is set or kept only by the owner;
5. private files only add;
6. unique key at (level, scope);
8. version = base + 1, and agrees with the base file;
9. branch: office-made, unmerged, for this file in this repository, opened by the caller.

Check 7 is a marked empty slot (`// 7. Sealed keys: not checked yet.`).

**Types:**
- `Writer { Owner | Agent | Member }(MemberId)`. It is named Writer because one_branch exports
  `Caller`.
- `Branch { office_made, merged, project: Option<ProjectKey>, path, opened_by }`.
- `WriteRefusal` has private fields; `code()`, `reason()` and `field()` map 1:1 onto ApiError.
- `RefusalCode`, `RefusalReason`.
- There is no org-proto dependency.

**Refactor.** `Input::seals()` was added in resolve.rs, and resolve now uses it. The review
confirmed it is exactly equivalent.

**Tests:** all green. On the final tree, org-process has 57 unit and 10 doc tests passing; fmt
and org-process clippy exit 0. The full workspace gates (2383/0, sqlcipher 73, both clippy runs)
passed on the tree just before the last two review fixes. **Rerun all six gates before sending
the request.**

**Mutants.** 38 in write.rs, plus the AR-5 pub-field doctest mutant (WriteRefusal), plus R1
(repo ignored) and V1 (base file not compared). All are red. The table is in the P6a commit
body. Rulings (a) and (c) each have a mutant.

**Fresh review.** Done once. Its MEDIUM (cross-repository branch) and LOW (base_version
disagreement) are fixed in P6a. There are no other findings.

**Next for P6a:**
1. Rerun the gates.
2. Send ONE request to ada/Tomas: the branch, the full SHA, base a2b7ee52 (the diff is the P6a
   commit only), the evidence block, the guard table and "no macOS-sensitive tests".
3. List the open choices below.

**Open choices to list in the request (all fail closed and pinned by tests):**
- **Check 3:** only `Writer::Agent(m)` writes `agents/m/`. The owner and human members are
  refused.
- **Check 5:** an organization key in any status or validity is refused. An unreadable
  organization file refuses with `private_only_adds` ("can't tell: <path> can't be read"). P6b
  may prefer `sealed` for that case.
- **Check 6:** an unreadable file at the place doesn't block.
- **Check 8:** when the base file's version can't be read, it isn't compared.
- **Check 4 (ruling a, extended):** a non-owner is refused when the new text has
  `overridable: false` OR the base file at that path seals or can't be shown not to. This closes
  un-sealing. The §5 sentence was updated to say so.

**P6b, not started.** Per the charter: check 7 resolves the base inputs with the written file in
place and refuses `sealed` when it is refused as sealed, a can't-tell included. That reuses
resolve and doesn't restate rule 3. The slot for it is in check_write. Ruling (b): seals are
judged against the base (the integration/default branch, A53) only; add its §5 sentence. Then
`check_approval`:
- the caller is the level's approver and not an author, unless it is the owner (`not_authorized`);
- the seen commit must equal the head (`conflict`);
- a later save voids an approval.

Bindings and `forwarded_to_owner` stay in the office.

## Rulings in force (ada, 2026-11-13; all written into processes.md)

- **R1 / R2 / R2b:** the §3.2 rules for diamonds, unreadable keys and readable
  `overridable: true`.
- **P3 corner:** a conflict names the unreadable path and the outermost real seal below it.
- **§3.3:** place names are not in the fingerprint. The cache owner (River) drops sets on a
  rename. See `knowledge/rory/p4b-note.md` for River's inputs: `delivered()` is the only read mark,
  `texts[key]` is the caller's guarantee, a fresh context passes an empty `read`, Q1 (body only),
  Q2 (`render_within` for the composer slice).
- **§4.2:**
  - a closing record blocks;
  - a conflict on branch-and-merge keeps the rule on;
  - an invalid or clashed file of the key, or one whose key can't be read, keeps it on;
  - an exception is used up by any start that makes a record (a reuse uses none);
  - an Unrelated copy is own work;
  - a missing own_tip or remote head is not own work;
  - the owner may move over own work on their own task, and a lead who is the assignee counts as
    the assignee.
- **§5:** ruling (a) as extended above, and (c) the branch codes. (b) comes with P6b.

## Traps

- **Run tests as user rory.** Use `su rory -s /bin/bash -c 'cd <tree> && export HOME=/home/rory/gh
  RUSTUP_HOME=/root/.rustup CARGO_HOME=/home/rory/.cargo CARGO_TARGET_DIR=<own dir>
  PATH=/root/.cargo/bin:$PATH; ...'` after `chmod -R o+rX <tree>`. The sqlcipher_init policy test
  needs a HOME with a `.gitconfig` holding `[safe] directory = *`.
- **Target directories.** Use one target dir per worktree; a shared one can silently reuse
  binaries. Disk is tight, so delete `debug/incremental` or use `CARGO_INCREMENTAL=0`.
- **Doctests.** On stable, rustdoc ignores the E0xxx in `compile_fail`. Every private-field
  doctest needs a pub-field mutant in the guard table (AR-5 item 4).
- **Policy and conflicts.** `macro_rules!` is refused by the repo's source-policy test.
  `crates/org-process/src/lib.rs` conflicts on every parallel slice (mod list, pub use): resolve
  it as a union.
- **Commit signing** can fail with a transient 503. Retry the commit.
- **Size.** Slices are at most 250 non-test lines. Tomas accepted overruns with a reason: P4b's
  render.rs at 281.
