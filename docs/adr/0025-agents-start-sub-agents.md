# ADR-0025 · An agent can start sub-agents inside its turn

Status: **accepted by the owner** (decisions 1 and 2, 2026-11-18). Implementation
pending. Decisions 3 to 11 are its design. The
[session design](../design/agent-sessions.md), the
[run modes spec](../specs/agent-run-modes-ux.md), the
[supervision design](../design/employee-supervision.md) and the
[agent instructions](../design/agent-instructions.md) follow it. It is built with C3.

## Context

Every agent harness the product drives lets an agent start sub-agents: a second model
conversation, inside the first, that does a bounded piece of work and returns its
result. It is how an agent reads a lot without filling its own context, and how it does
two things at once.

The session design turned this off. It listed sub-agent work among the tools that "act
outside the office or the sandbox" and left it out of the allowlist, and the run modes
spec marked internal delegates as not in v1. That was this product's rule, not a limit
of a vendor's headless mode.

## Decision

The owner's rule:

1. **An agent can start sub-agents.** It is a primitive of every harness, and this
   product is a harness of harnesses: it does not take away what they give, except
   what would break isolation.
2. **If a way of running a vendor's tool cannot do it, that way is not used.** The
   adapter runs the tool in a mode that can.

The design:

3. **A sub-agent is part of its parent's turn.** It is not an employee. It is not
   hired, has no inbox, cannot be addressed, and does not appear in the organization.
   The product's records know the employee and the turn.
4. **It has what the parent has, and never more:** the same sandbox and writable
   places, the same private home, the same tool allowlist, the same permission policy
   and the same approval bridge. A sub-agent cannot be given a tool or a path the
   parent lacks.
5. **Its commands are the employee's.** `agent-os` run by a sub-agent binds with the
   run's binding, because the sub-agent descends from the process the office started.
   What it does is recorded under the employee
   ([chat spec](../specs/chat-ux.md): "anything an agent's own subprocesses post is
   attributed to that agent").
6. **It lives inside the turn.** The turn is not over while a sub-agent runs. When the
   turn ends, nothing of it is left running; the office stops what remains, as it does
   for any process the agent started, and records it. Sub-agents that keep running in
   the background after the turn's result stay off, because work after a result would
   be a turn nobody started.
7. **Its cost is the employee's.** Every model call of a sub-agent is usage of the
   employee in that run, and is in the turn's cost.
8. **Its model is the agent's model.** A sub-agent runs on the model of the agent's
   execution configuration. A cheaper model for helpers is the model policy for each
   slot, which stays a later feature (FC-105). Until then the cheap path for reading a
   channel is `channel summarize` ([ADR-0024](0024-a-channel-question-is-watched-for-its-answer.md)).
9. **The office writes the sub-agent's definition.** Where a runtime takes sub-agent
   definitions, the office supplies one general-purpose definition, written before
   every start like its other runtime files: a short built-in text that says it works
   for the main session and reports to it, and the organization's must-follow
   documents ([processes design](../design/processes.md), section 3.2). An agent
   cannot add or change a definition.
10. **What each adapter must show.** For each vendor tool, the sign-in spike (C1) also
    records how sub-agents are started under the isolation switches, that they inherit
    the sandbox, the tools and the environment, and where their conversation is
    stored. The adapter enables the tool's own sub-agent facility and checks at every
    start that a sub-agent's tools are within the allowlist. A mode of a vendor's tool
    in which sub-agents cannot run inside the isolation is not used for agents the
    office runs.
11. **Still off.** The other tools the session design keeps out stay out, because they
    do act outside the office or the sandbox: messaging between sessions, scheduling
    and remote triggers, background work after a result, the runtime's own worktree
    tool, and web access. Private specialists, generated copies of the employee, and
    delegation chosen by a process stay later features (FC-107).

## Acceptance criteria

| ID | Passing evidence |
| --- | --- |
| SUB-1 | An agent the office runs starts a sub-agent with its runtime's own tool; the sub-agent finishes and its result returns to the main session. This passes for every adapter the office offers |
| SUB-2 | A sub-agent's tools are within the parent's allowlist; a write outside the parent's writable places is refused to it as to the parent; an action needing approval reaches the same approval bridge |
| SUB-3 | `agent-os` run by a sub-agent is bound as the employee and its change is recorded under the employee |
| SUB-4 | The model calls of a sub-agent are in the employee's usage for that run, and the turn's cost includes them |
| SUB-5 | After a turn's result nothing the turn started is running; a sub-agent still alive at the end is stopped and recorded |
| SUB-6 | The session view shows a sub-agent's conversation under the turn that started it |
| SUB-7 | An agent's edit to a sub-agent definition in its home does not survive the next start, and an undeclared definition is not loaded |

## Consequences

- An agent keeps its own context small by handing reading and searching to a
  sub-agent, and can do independent pieces of work at once.
- A turn can cost more than its main conversation shows. The turn's cost includes its
  sub-agents, and budgets, once built, hold the whole turn.
- The isolation has one more thing to prove for each vendor tool: that a sub-agent is
  confined exactly as its parent is.

## Rejected

- **Keeping them off.** It makes an agent load into its own context everything it has
  to look at, and it removes something every harness has.
- **Sub-agents as employees.** A hire is for someone the organization knows and
  addresses. A sub-agent is a tool of one turn.
- **A product command that starts a sub-agent.** The runtime already has the tool, and
  a second mechanism would have to rebuild what it does.
