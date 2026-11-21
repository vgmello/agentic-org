---
id: doc-22
title: v1 Implementation Kickoff Prompt (Historical)
type: guide
created_date: '2026-11-21T00:16:00Z'
updated_date: '2026-11-21T00:16:00Z'
tags:
  - processes
  - historical
  - onboarding
---
# Kickoff prompt for v1 implementation

> [!NOTE] Review note (2026-11-21) — Historical / Superseded
> Written for the initial multi-track v1 kickoff before Backlog.md adoption (suggesting starting from A1/A2/B1a/X1a/K1, which are now built or merged). New sessions should start from `backlog instructions overview`, `doc-1` (Vision), and active Backlog tasks (`TASK-1`–`TASK-75`). Retained for historical reference.

Send the prompt below to each engineer's agent, with the track and the first increment
filled in. The tracks and increments are defined in the
[implementation plan](../implementation-plan.md). Update this file when the plan changes.

```text
You are working on Agentic Org in the repository vgmello/agentic-org. Start from the
latest origin/v1.

Read first, in this order:
1. docs/START-HERE.md: what the product is, where each kind of requirement is written,
   and how to go from a feature to working code.
2. docs/architecture-non-negotiables.md: what never varies, and how you make and record
   a decision.
3. AGENTS.md: how to work in this repository.
4. docs/implementation-plan.md: the milestones and increments. Read your increment and
   every document it lists under "Read".

Your track is <A, B, C or X> and your first increment is <for example A2, A1 or X1a>.

Rules that always apply:
- Nothing has shipped. Write no migration, import, compatibility shim, version
  negotiation for old clients or legacy handling. Replace, do not adapt.
- The requirement documents decide behaviour. Code that disagrees with them is what you
  change.
- You make architectural decisions yourself. Where the documents are silent or disagree,
  decide inside the non-negotiables, write the decision into the owning document, and
  carry on. A non-negotiable varies only if no design can meet it; then choose the best
  option, record it as an exception, and report it.
- Build the smallest path a person or agent can use, not a layer.

Before writing code, reply with a short plan for your increment: what a person can do
when it is finished, the files you will own, what you will delete, the acceptance
criteria you will make pass, and the decisions you are taking where the documents are
silent.

For every increment: branch from origin/v1, keep the checks green, update
the row in docs/FEATURES.md with evidence, get an independent review, then merge origin/v1 into
your branch, tell the other lead sessions, git merge --no-ff into v1 and push it. Never push to main. Tests never touch a real organization's data.
```

Suggested start: one engineer takes Track A from A2 (setup's registration link), then
A3 and A4, so that installing, registering and creating the organization work first.
The other takes A1, the clean-up the store rebuild depends on, and then Track B from
B1a. B1a to B1c, the store foundation, are built from the
[office store design](../design/office-store.md).
A third agent can take Track X, Linux, from X1a at the same time. X1a is the shared
platform crate's skeleton, its CI check and the service contract agreed with the
agent on A2; read it with [the Linux host design](../design/linux-host.md).
K1, the command contract, needs nothing and suits any free agent: it is the one
grammar and answer format that every `agent-os` and `agentic-org` command is written
against; read it with the [agent command line design](../design/agent-os-command-line.md).
