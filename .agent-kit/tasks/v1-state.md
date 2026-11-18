# v1: current task record

The one current task record for v1. Update it when an increment starts, is blocked or
is merged.

## What v1 is built from

v1 is built from the [implementation plan](../../docs/implementation-plan.md). Its order
and split into increments are a proposal the engineer who takes an increment may
change; the behaviour is what the linked requirement documents say. Every increment
follows "For every increment" in that plan and the workflow in
[AGENTS.md](../../AGENTS.md).

- **Milestone one: one office on a Mac.** People and agents on board and talking, all
  in a single office on macOS:
  1. Getting in: install, register from the link setup prints, create the organization
     (A2, A3, A4, then A5; A1 alongside).
  2. The organization: the office, projects, teams and people; a second person signs
     in (B1, B2, B3, O1, A6, U1).
  3. Agents: hired through HR, started by the office, with a view of what they did
     (C1 to C5).
  4. Messages: chat and asks that wait for an answer (B6, B7, C6).
  5. Tasks: in the browser and with `agent-os`; an agent works in a repository (B4, B5,
     C7).
- **Milestone two: Linux,** including a container, runs now in parallel as Track X (X1a to
  X6; [design](../../docs/design/linux-host.md)).
- **Milestone three: a second office** on another machine (S1).
- **Later, still v1:** the increments listed under "Later, in no fixed order".

## State

Work done before the plan was written is not carried over as a task; what the code
does today is in [executable migration](../../docs/design/executable-migration.md)
and in the "Code today" line of each increment.

| Increment | Lead | Branch | State |
| --- | --- | --- | --- |
| X1a to X6 (Track X, Linux) | Track X lead agent | `claude/hopeful-noether-lsmbac` | Plan and per-increment state: [v1-track-x.md](v1-track-x.md) |

## Known facts that affect the first increments

- `crates/org-platform` exists (A2 B0), and from X1a no file outside it gains a platform predicate:
  `crates/org-platform/tests/platform_conditionals.rs` fails on one, and CI fails a
  change that adds a file to `.github/platform-conditionals.allow` or raises a count. A new operating-system need goes
  into that crate, with its macOS adapter and a refusing one, and a note for Track X.
- `crates/org-host` compiles on Linux but refuses to start an office worker there: the
  check that a started worker is the intended program exists only for macOS, and the
  version for every other system refuses (`supervisor/channels.rs`). CI lints the host
  on Linux and excludes its tests from the Linux workspace run. Increment X2 brings
  the host to Linux.
