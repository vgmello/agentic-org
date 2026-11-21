# ADR-0027 · A lead with hiring power approves a hire

Status: **decided by the owner 2026-11-18**, in answer to the hiring point of
[REQ-07](../specs/req-07-composable-process-packages-and-team-blueprints.md): "Not a
person, a lead approves every hire. If I'm the first lead in the team because I'm a
person, I have to approve. If an agent is the lead of the team, it's a flag in the
agent to enable it to hire someone else." And: "Allow hiring should be a flag, because a
team lead of an engineering team may not, but a team lead of a director team could."
Implementation pending.

**Replaced in part on 2026-11-18 by [ADR-0029](0029-only-who-may-hire-hires.md):** there
is no approval and no escalation. Only someone who may hire can start a hire, and
everyone else is refused. Decisions 1, 2, 5 and 6 below, and criteria HIRE-1 to
HIRE-7, are history. Decisions 3, 4 and 7 stand: the `may_hire` permission, how it is
given, and the two records that carry authority.

## Context

Until now a person approved every hire: the owner, or a person in the lead chain of
the team the new agent joins. No agent could approve, and no field on a record granted
the right. A project that follows a development process is led by seed agents who decide,
after the specification is clear, which specialists the work needs. With approval
reserved to people, every such hire waited for a person even when the team's lead was
an agent the owner trusted to staff its team.

## Decision

> **Note:** Decisions 1, 2, 5, and 6 (hire approval chains, escalation, and hiring limits) were superseded on 2026-11-18 by [ADR-0029](0029-only-who-may-hire-hires.md) (no approval or escalation: only someone who holds `may_hire` can start a hire, bounded by `teams.max_members`). The surviving decisions from this ADR are preserved below.

3. **The permission is given employee by employee, to a person or an agent.** A person's
   record is created with `may_hire` on and an agent's with it off. The lead of an
   engineering team can have it off while the lead of the director team above has it
   on. Hiring is into a team; no project is involved.
4. **Only the owner, or a person above the employee, turns `may_hire` on or off.** The
   change is attributed and kept in history. Nobody sets their own, and the built-in
   HR lead never has it.
7. **Non-negotiable 9 is restated.** Authority comes from the caller's place in the
   organization. Records carrying authority (`teams.lead_employee_id`,
   `employees.reports_to_employee_id`, and `employees.may_hire`) are granted only by
   someone already above the holder, and the office checks them on every call.

## Rejected

- **Keeping hiring power with people only.** It makes a person the bottleneck of a team an
  agent leads, which is the case development processes create on purpose.
- **Letting every lead hire automatically.** A lead appointment says who directs a team, not who
  may spend on new employees. The owner wants that as a separate, explicit switch,
  for people and agents alike.
- **A permission in the development process.** A development process is content and grants nothing
  ([ADR-0026](0026-development-processes-are-content.md)).

## Acceptance criteria

Criteria HIRE-1, HIRE-2, HIRE-3, HIRE-5, HIRE-6, and HIRE-7 were superseded by [ADR-0029](0029-only-who-may-hire-hires.md) (only someone holding `may_hire` starts a hire; team capacity uses `teams.max_members`). The surviving `may_hire` permission criterion is:

| ID | Passing evidence |
| --- | --- |
| HIRE-4 | An employee's call to set its own `may_hire` is refused, and so is an agent's call to set anyone's. A person not above the employee is refused. A new person record has it on and a new agent has it off. |

## Consequences

- An agent with `may_hire` can add employees, and so cost, without a person
  approving each hire first. Agent and task budgets
  ([document 17](../design/organization-data/17-budgets-and-controls.md)) and
  `teams.max_members` are the bounds, and every hire shows on the owner's Home.
- See [ADR-0029](0029-only-who-may-hire-hires.md) for the active hiring flow and acceptance criteria.
