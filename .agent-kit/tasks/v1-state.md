# v1: current task record

The one current task record for v1. Update it when an increment starts, is blocked or
is merged.

## What v1 is built from

v1 is built from the [implementation plan](../../docs/implementation-plan.md). Its order
and split into increments are a proposal the engineer who takes an increment may
change; the behaviour is what the linked requirement documents say. Every increment
follows "For every increment" in that plan and the workflow in
[AGENTS.md](../../AGENTS.md).

- **Milestone one: one office.** People and agents on board and talking, all in a
  single office:
  1. Getting in: install, register from the link setup prints, create the organization
     (A1 to A5).
  2. The organization: the office, projects, teams and people; a second person signs
     in; Linux and containers (B1, B2, B3, O1, A6, U1, A7).
  3. Agents: hired through HR, started by the office, with a view of what they did
     (C1 to C5).
  4. Messages: chat and asks that wait for an answer (B6, B7, C6).
  5. Tasks: in the browser and with `agent-os`; an agent works in a repository (B4, B5,
     C7).
- **Milestone two: a second office** on another machine (S1). It starts only when
  milestone one works in one office.
- **Later, still v1:** the increments listed under "Later, in no fixed order".

## State

No increment has started. Work done before the plan was written is not carried over as
a task; what the code does today is in
[executable migration](../../docs/design/executable-migration.md) and in the "Code
today" line of each increment.

| Increment | Lead | Branch | State |
| --- | --- | --- | --- |
| (none started) | | | |

## Known facts that affect the first increments

- At commit 04041dc, `crates/org-host` does not compile on Linux:
  `supervisor/channels.rs` calls `org_socket::peer_process_allowed`, which does not
  exist. CI excludes `org-host` and `agentic-org` from its Linux workspace runs.
  Increment A7 brings the host to Linux.
