# ADR-0019 · Task numbers are minted by the creating office

Status: **accepted by the owner 2026-11-17** (conflict plan entry C-12), and applied to
the data contract ([document 06](../design/organization-data/06-identity-and-numbering.md)).
The owner accepted three things: the office part of a task reference is a letter
(`T-B37`); the scheme applies to tasks only; and an office counts all its tasks in one
sequence ("let's just do it by office and keep it simple"). The details of the grammar
(which letters, how many) and decisions 2 and 4 to 7 are the lead's design for that
choice. One point is still open: whether creating a task may use the fast
acknowledgement.

The owner's requirement, in his words: tasks "happen inline and offline" and "have to
happen incredibly fast"; going to HQ to create or manage a task is "too long of a round
trip". He proposed putting an office index in front of the task number and asked for an
analysis. It follows from this, as a design aim, that a task's reference should be
final the moment it is created.

## Context

- The selected contract has HQ allocate every readable number. An office that creates
  a task offline saves it with an empty number, shown as "Pending number", until HQ
  answers ([document 06](../design/organization-data/06-identity-and-numbering.md)).
- Tasks are the records agents create all day. Teams, projects, releases, milestones
  and features are created less often and may wait for HQ.
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
| **A letter code per office and a per-office counter, `T-B37`** | **Chosen by the owner.** |

## Decision

1. **Grammar (letter form accepted by the owner; details are the lead's).** `T-<CODE><N>` for tasks. `CODE` is one
   or two uppercase letters from A to Z without I and O. `N` is a positive integer with
   no leading zeros. Examples: HQ is `A`, so its first task is `T-A1`; the second
   office is `B`, giving `T-B37`; the twelfth is `M`; the twenty-fifth is `AA`. Input
   is case-insensitive and output is uppercase.
2. **The boundary between letters and digits makes it unambiguous** at any size, and a
   reference with the letter dropped does not parse, so a typo never names another task.
3. **One sequence per office (accepted).** An office counts all its tasks in one
   sequence, whatever project or team they belong to. A reference is therefore unique in
   the organization and complete on its own: `T-B37` never needs a project or team in
   front, and moving a task to another project does not change it. The owner first
   chose a sequence per project and then settled on one per office as the simpler rule.
4. **HQ assigns office codes** when it creates or enrols an office, which is already a
   rare HQ operation. A code is never changed and never reissued. Two letters give 600
   offices.
5. **The creating office mints the number** in the same local transaction as the task.
   No network call is made. "Pending number" disappears for tasks.
6. **The letter means "minted at", not "lives at".** Routing never uses it. A task or
   agent that moves to another office keeps its reference.
7. **Never reused.** An office keeps a durable high-water mark and never shows a number
   above the last flushed mark. After a crash the counter jumps to the mark; skipped
   numbers are allowed. An office that cannot prove its counter is current, for example
   after restoring an older backup, must get a new code from HQ before it creates
   anything; the old code is retired and its references still resolve.
8. **Unchanged.** `TEAM-`, `P-`, `F-`, `R-` and `M-` stay HQ-allocated plain numbers,
   and feature slugs keep their existing rule (owner, 2026-11-17: the office pattern is
   for tasks only). A feature created while HQ is unreachable keeps the pending state
   the contract already describes.

## Default project for commands

Finding a task by its reference never needs a project. Commands that do need one, such
as creating or listing tasks, take it from the agent's context (owner, 2026-11-17):
`agent-os` knows the project of the task the agent is working on, or the project the
agent is assigned to. When the agent has no project, or means another one, the project
is a required parameter.

## Still open

**Whether creating a task may use the fast acknowledgement** of
[ADR-0014](0014-acknowledgement-classes-and-agent-os-latency.md). Today creation waits
for the drive, which is about 0.3 ms on the Linux machine measured and several
milliseconds on a Mac. With office-minted numbers the fast class is safe for numbering,
because the durable mark of decision 7 already prevents a number from being reused
after a power cut. Recommended: yes.

## Where it is applied

- [Document 06](../design/organization-data/06-identity-and-numbering.md): the reference
  rule, the multiple-offices section, files and recovery.
- The draft schema: `tasks.office_code`, a required `tasks.number`, uniqueness of code
  and number in the organization, `office_codes` (written by HQ) and
  `office_task_counters` (each office's own), with tests.
- The identity table of the [data index](../design/organization-data/README.md),
  [document 03](../design/organization-data/03-features-and-work.md), the UX specs and
  the [Git design](../design/projects-git-github.md), whose examples now read `T-A42`.

Still to do in code (FC-142): office codes and counters, the reference parser in
`agent-os`, the default project context, and `NumberScope` in `crates/org-work`.
