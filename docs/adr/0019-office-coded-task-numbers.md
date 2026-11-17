# ADR-0019 · Task and feature numbers are minted by the creating office

Status: **proposed 2026-11-17**, awaiting the owner's confirmation of the reference
grammar (conflict plan entry C-12). The owner decided the requirement: creating a task
must never wait for HQ, in any office, online or offline, and an agent must never hold
a reference that later changes. He proposed putting an office index in front of the
task number and asked for an analysis. This ADR is that analysis's recommendation. It
is not yet applied to the data contract.

## Context

- The selected contract has HQ allocate every readable number. An office that creates
  a task offline saves it with an empty number, shown as "Pending number", until HQ
  answers ([document 06](../design/organization-data/06-identity-and-numbering.md)).
- Tasks and features are the records agents create all day. Teams, projects, releases
  and milestones are created rarely and may wait for HQ.
- The owner's literal proposal concatenates digits: office 1's first task is `T-11`,
  office 2's is `T-21`. That is ambiguous as soon as either part has two digits:
  `T-111` is office 1 task 11, or office 11 task 1.

## Options considered

| Option | Verdict |
| --- | --- |
| Digits concatenated, `T-11` | Rejected: not decodable beyond nine offices or nine tasks. |
| Fixed-width digits, `T-01000042` | Rejected: the width caps offices or tasks forever, and leading zeros get dropped. |
| A separator, `T-1.1` or `T-1-1` | Rejected: `1.10` reads as `1.1`, and `task/T-1-1-2` collides with slug and branch suffixes. |
| HQ unprefixed, satellites prefixed | Rejected: a dropped prefix silently names a real HQ task. |
| Number blocks leased by HQ, plain `T-n` | Rejected: fails when an office stays offline past its block, and the first task in a new scope still needs HQ. |
| A local reference now and an HQ number later | Rejected: two names per task, and HQ registration returns. |
| **A letter code per office and a per-office counter, `T-B37`** | **Recommended.** |

## Recommended decision

1. **Grammar.** `T-<CODE><N>` for tasks and `F-<CODE><N>` for features. `CODE` is one
   or two uppercase letters from A to Z without I and O. `N` is a positive integer with
   no leading zeros. Examples: HQ is `A`, so its first task is `T-A1`; the second
   office is `B`, giving `T-B37`; the twelfth is `M`; the twenty-fifth is `AA`. Input
   is case-insensitive and output is uppercase.
2. **The boundary between letters and digits makes it unambiguous** at any size, and a
   reference with the letter dropped does not parse, so a typo never names another task.
3. **One counter per office per kind,** for the whole organization, not per project or
   team. A reference is unique in the organization and needs no qualifier: `T-B37`
   resolves anywhere. Moving a task to another project or team never changes it.
4. **HQ assigns office codes** when it creates or enrols an office, which is already a
   rare HQ operation. A code is never changed and never reissued. Two letters give 600
   offices.
5. **The creating office mints the number** in the same local transaction as the task.
   No network call is made. "Pending number" disappears for tasks and features.
6. **The letter means "minted at", not "lives at".** Routing never uses it. A task or
   agent that moves to another office keeps its reference.
7. **Never reused.** An office keeps a durable high-water mark and never shows a number
   above the last flushed mark. After a crash the counter jumps to the mark; skipped
   numbers are allowed. An office that cannot prove its counter is current, for example
   after restoring an older backup, must get a new code from HQ before it creates
   anything; the old code is retired and its references still resolve.
8. **Feature slugs.** `F-<CODE><N>` is the only final reference. A slug is a label that
   is provisional until HQ registers it; the first registration in a project wins, and
   a later duplicate keeps its `F-` reference and is flagged for renaming.
9. **Unchanged.** `TEAM-`, `P-`, `R-` and `M-` stay HQ-allocated plain numbers.

## What the owner is asked to confirm

- Letters for the office code (`T-B37`) instead of digits (`T-237`).
- One organization-wide counter per office, so numbers grow larger than per-project
  numbers and the project qualifier (`P-8/T-42`) goes away.
- Whether `work.create` may also use the committed acknowledgement class of ADR-0014,
  which this scheme makes safe for numbers and which the 1 to 2 ms target needs.

## If accepted, what changes

Document 06 (scopes, allocation, recovery), the identity table and storage paragraph of
the organization-data index, document 03, the draft schema (`tasks`, `features`,
`reference_counters`, `reference_allocations`, a new office-code registry), the UX specs
that show "Pending number", and `NumberScope` in `crates/org-work`. The detailed list is
kept with conflict plan entry C-12.
