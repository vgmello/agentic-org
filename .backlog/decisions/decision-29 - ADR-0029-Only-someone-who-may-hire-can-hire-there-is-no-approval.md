---
id: decision-29
title: 'ADR-0029: Only someone who may hire can hire; there is no approval'
date: '2026-11-21T00:47:00Z'
status: accepted
---

> **Review note (2026-11-21):** Supersedes ADR-0027 (`decision-27`) hiring approval and escalation flows while retaining `may_hire`. Implementation pending in `TASK-41` (C2 HR hiring authorization, proposal confirmation, provisioning, and agent lifecycle).

# ADR-0029 · Only someone who may hire can hire; there is no approval

Status: **decided by the owner 2026-11-18**. Implementation pending. It replaces the
approval and escalation of [ADR-0027](decision-27%20-%20ADR-0027-A-lead-with-hiring-power-approves-a-hire.md) (its decisions
1, 2, 5 and 6); the permission that record introduced, `may_hire`, stays. The
[employee contract](../docs/organization-data/doc-78%20-%20Organization-Data-04-—-Agent-Structure.md#who-may-hire),
the [hire flow](../docs/specs/doc-45%20-%20Spec-Hiring-Profiles-and-Team-Moves-UX.md) and the
[agent command line](../docs/design/doc-28%20-%20Design-agent-os-Command-Line.md) follow it.

The owner's words: "There's no approval of a hire, because the hire command is only
allowed for who can make the hire themselves. If a staff engineer that cannot hire
tries to run the command, it won't have access. It will say: you don't have access,
talk to your team lead. Only people who can hire can run the hire command and talk to
HR. Even if they send a message to it, it won't work, because they don't have that
permission."

## Context

ADR-0027 let anyone ask for a hire and routed the request up the lead chain to the
first lead with hiring power, who approved or rejected it. That needed a waiting
state, an escalation list, a fallback to the owner after a day and a way for an agent
lead to approve. The last one was never designed: an agent lead could be named the
approver and had no command to approve with.

## Decision

1. **There is no approval.** A hire is made by someone who may hire. Nobody else can
   start one, so nothing waits for anybody.
2. **Who may hire into a team.**
   - The organization's owner, and the server's administrator acting in the
     organization, into any team.
   - An employee, person or agent, whose `may_hire` is on, into the teams it leads
     and the teams under them. `may_hire` is shown as **Allow hiring**.

   The permission does nothing for an employee who leads no team.
3. **Everyone else is refused.** `agent-os hr hire`, the app's Hire, and a message or
   mention to the HR lead are refused for a caller who may not hire into the team
   named, or who may hire nowhere. The answer is `not_authorized` with the hint
   "You don't have access to hire. Ask your team lead." Nothing is recorded as a
   request, and no turn starts for the HR lead.
   - The office does not read a message to decide. Every direct message and mention
     to the HR lead from a caller who may hire nowhere is refused.
   - A caller who may hire somewhere reaches the HR lead. The team is checked when
     the draft is written and again when it is confirmed: a draft for a team the
     caller may not hire into is refused there.
4. **Asking goes by conversation.** An employee that needs a colleague asks its lead,
   as it asks for anything else. A lead that may not hire asks its own lead. There is
   no record that travels up the chain.
5. **The hirer confirms what is created.** The HR lead drafts the proposal, or the
   hirer fills in the fields. The hirer sees exactly what will be created and
   confirms it; product code then provisions the agent. A call that gives every
   required field is its own confirmation. Only the hirer who started the request,
   or the owner, confirms it or tries it again, and the permission is checked again
   each time; a draft whose hirer was deactivated or lost the permission can only
   be confirmed by the owner.
6. **The hire is recorded.** A hire request records who hired, the proposal, the
   reason, when it was confirmed and the agent it produced. Its states are
   `drafting`, `hiring`, `hired`, `withdrawn` and `failed`. "Hired by" on an agent's
   profile is read from it.
7. **How the permission is given is unchanged** (ADR-0027 decisions 3 and 4). A
   person's record is created with it on and an agent's with it off. Only the owner,
   or a person above the employee, turns it on or off. Nobody sets their own, and the
   built-in HR lead never has it.
8. **A team has a member limit** (owner, 2026-11-18). Every team has `max_members`,
   6 unless changed. It counts the active employees, people and agents, whose home
   team it is. A hire into a full team is refused with the limit named, for every
   hirer; so is moving an employee into a full team. The limit is changed on the
   team by the owner, or by a person who leads that team or a team above it; an
   agent never changes it. This replaces the weekly and cost limits in Settings.
9. **An agent that may hire hires without a person.** That is what turning Allow
   hiring on for an agent means. It is off until a person turns it on.

## Rejected

- **Keeping the approval for requests from employees who may not hire.** It keeps
  every state and route this record removes, for a case a conversation with the lead
  already covers.
- **Letting the HR lead decide.** The HR lead drafts. It holds no authority.

## Acceptance criteria

APPR-1 to APPR-7 in the [hire flow](../docs/specs/doc-45%20-%20Spec-Hiring-Profiles-and-Team-Moves-UX.md), and:

| ID | Passing evidence |
| --- | --- |
| HIRE-1 | An agent without `may_hire` that runs `agent-os hr hire` gets `not_authorized` with the hint to ask its team lead; no request exists afterwards and the HR lead ran no turn. |
| HIRE-2 | The same agent's direct message or `@hr` mention asking for a hire gets the same refusal as a reply, and starts no turn for the HR lead. |
| HIRE-3 | A lead holding `may_hire`, person or agent, hires into its own team and into a team under it; no other employee is asked. The hire records that lead. |
| HIRE-4 | That lead's hire into a team it does not lead, directly or through a parent team, is refused. The owner's is not. |
| HIRE-5 | An employee's call to set its own `may_hire` is refused, and so is an agent's call to set anyone's. A new person record has it on and a new agent has it off. |
| HIRE-6 | A new team has a limit of 6 members. A hire into a team that has 6 active members is refused with the limit named, for a lead and for the owner; after a person raises the team's limit the hire succeeds. An agent's call to change the limit is refused. |
| HIRE-7 | With agents leading both an engineering team (`may_hire` off) and the director team above it (`may_hire` on), the engineering lead's hire is refused, and after it asks the director team's lead, that lead hires into the engineering team. No person acts and no project exists during the test. |
| HIRE-8 | A drafted proposal creates nothing until the hirer confirms it; a hire that stops partway shows the step that failed and finishes with Try again. |

## Consequences

- The states `waiting`, `approved` and `rejected`, the approver, the escalation list
  and the one-day fallback to the owner are removed from the data and the app.
- `gate_required` has no command that returns it in milestones one and two. It stays
  in the answer contract for a later approval.
- An agent lead with the permission adds employees, and so cost, with no person
  seeing each hire first. The permission is off for agents by default, every hire
  shows on the owner's Home, and budgets and each team's member limit are the bounds.
