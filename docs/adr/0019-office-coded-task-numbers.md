# ADR-0019 · Task numbers are minted by the creating office

Status: **accepted in part by the owner 2026-11-17** (conflict plan entry C-12).
Accepted: the office part of a task reference is a letter code (`T-B37`), and the
scheme applies to tasks only. Features and every other numbered record keep their
HQ-allocated numbers. One point is still open: whether an office counts all its tasks
in one sequence or per project (see "Still open"). The data contract is rewritten when
that is settled; until then document 06 carries a note and must not be built as written
for tasks.

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
| **A letter code per office and a per-office counter, `T-B37`** | **Recommended.** |

## Decision

1. **Grammar (accepted).** `T-<CODE><N>` for tasks. `CODE` is one
   or two uppercase letters from A to Z without I and O. `N` is a positive integer with
   no leading zeros. Examples: HQ is `A`, so its first task is `T-A1`; the second
   office is `B`, giving `T-B37`; the twelfth is `M`; the twenty-fifth is `AA`. Input
   is case-insensitive and output is uppercase.
2. **The boundary between letters and digits makes it unambiguous** at any size, and a
   reference with the letter dropped does not parse, so a typo never names another task.
3. **How an office counts (recommended, still open).** One sequence per office for all
   its tasks, whatever project or team they belong to. See "Still open".
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

## Still open

**How an office counts its tasks.** Today every project counts its own tasks from 1, so
project 8 has a `T-42` and project 9 can have a `T-42` too. That is why a full
reference is written `P-8/T-42`: the number alone does not say which task is meant.
With an office letter there are two ways to count.

| | A. One sequence per office (recommended) | B. One sequence per office per project |
| --- | --- | --- |
| The second office's tasks | `T-B1`, `T-B2`, `T-B3`, in whatever projects they are | `P-8/T-B1`, `P-8/T-B2`, and also `P-9/T-B1` |
| Is `T-B37` enough to find the task? | Yes. There is only one in the organization. | No. The project must be written in front. |
| Moving a task to another project | The reference stays the same. | It needs a new number, or carries a number that does not fit its new project. |
| Size of the numbers | Larger: an office's thousandth task is `T-B1000`. | Smaller inside each project. |
| What each office must keep | One counter. | One counter per project, created when it first makes a task there. |

The owner's own example (`T-11` for the first office, `T-21` for the second) had no
project in front, which is option A. Option A is recommended: an agent can quote a task
in a message, a branch name or a commit with nothing else, and it always resolves.

**Whether creating a task may use the fast acknowledgement** of
[ADR-0014](0014-acknowledgement-classes-and-agent-os-latency.md). Today creation waits
for the drive, which is about 0.3 ms on the Linux machine measured and several
milliseconds on a Mac. With office-minted numbers the fast class is safe for numbering,
because the durable high-water mark of decision 7 already prevents a number from being
reused after a power cut. Recommended: yes.

## What changes when the open point is settled

- [Document 06](../design/organization-data/06-identity-and-numbering.md): the task rows
  of the counter table, the multiple-offices section (tasks leave "pending
  registration"), and recovery (the high-water mark and the rule for an office that
  cannot prove its counter).
- The identity table of the [data index](../design/organization-data/README.md) and the
  task references in [document 03](../design/organization-data/03-features-and-work.md).
- The draft schema: `tasks.number` and its key, a place for the office code on a task,
  an office-code registry written by HQ, and per-office counters.
- The UX specs that show "Pending number" for a task, and branch names in the
  [Git design](../design/projects-git-github.md), which then always have a reference.
- Code: `NumberScope` in `crates/org-work` and the task reference parser in `agent-os`.
