# ADR-0022 · A lead with hiring power approves a hire

Status: **decided by the owner 2026-11-18**, in answer to the hiring point of
[REQ-07](../specs/req-07-composable-process-packages-and-team-blueprints.md): "Not a
person, a lead approves every hire. If I'm the first lead in the team because I'm a
person, I have to approve. If an agent is the lead of the team, it's a flag in the
agent to enable it to hire someone else." And: "Allow hiring should be a flag, because a
team lead of an engineering team may not, but a team lead of a director team could."
Implementation pending.

## Context

Until now a person approved every hire: the owner, or a person in the lead chain of
the team the new agent joins. No agent could approve, and no field on a record granted
the right. A project that follows a development process is led by seed agents who decide,
after the specification is clear, which specialists the work needs. With approval
reserved to people, every such hire waited for a person even when the team's lead was
an agent the owner trusted to staff its team.

## Decision

1. **A lead with hiring power approves a hire.** Hiring power is the `may_hire`
   permission of an employee, shown as Allow hiring
   ([document 04](../design/organization-data/04-agent-structure.md#who-approves-a-hire)).
2. **A request escalates until it reaches someone who has it.** It goes up the lead
   chain from the team the new agent joins and stops at the first lead whose
   `may_hire` is on. A lead without it does not decide; the request passes on, and
   that lead and the requester are told who it went to. The organization's owner
   always has hiring power, and so does the server's administrator acting in the
   organization; a request that no lead can decide reaches the owner.
3. **The permission is given lead by lead, to a person or an agent.** A person's
   record is created with it on and an agent's with it off. The lead of an
   engineering team can have it off while the lead of the director team above has it
   on. Hiring is into a team; no project is involved.
4. **Only the owner, or a person above the employee, turns `may_hire` on or off.** The
   change is attributed and kept in history. Nobody sets their own, and the built-in
   HR lead never has it.
5. **A lead who has the permission hires into its own teams without a second
   approval.** The proposal is still drafted and recorded, with that lead as requester
   and approver. This replaces the rule that a requester's own request went one level
   up.
6. **Everything else about hiring is unchanged.** HR drafts and never approves.
   Product code provisions, and only after approval. Hiring limits set in Settings
   apply to every lead, and a request above a limit goes to the owner.
7. **Non-negotiable 9 is restated.** Authority comes from the caller's place in the
   organization. Two records carry it: a team's lead appointment and a lead's
   permission to hire. Each is given only by someone already above the holder, and
   the office checks it on every call.

## Rejected

- **Keeping approval with people only.** It makes a person the bottleneck of a team an
  agent leads, which is the case development processes create on purpose.
- **Letting every lead approve.** A lead appointment says who directs a team, not who
  may spend on new employees. The owner wants that as a separate, explicit switch,
  for people and agents alike.
- **A permission in the development process.** A development process is content and grants nothing
  ([ADR-0021](0021-development-processes-are-content.md)).

## Acceptance criteria

APPR-1 to APPR-4, APPR-8 and APPR-9 in the
[hire flow](../specs/our-org-in-the-app.md), and:

| ID | Passing evidence |
| --- | --- |
| HIRE-1 | With the team's lead holding `may_hire`, an agent's hire request waits for that lead; nothing is provisioned before they approve. |
| HIRE-2 | With the team's lead, person or agent, not holding `may_hire`, the request goes to the next lead above who holds it, or to the owner, and the lead passed over and the requester are told. |
| HIRE-3 | A lead holding `may_hire` who asks for a hire into its own team has it approved by that request and provisioned, and the hire records it as requester and approver. |
| HIRE-4 | An employee's call to set its own `may_hire` is refused, and so is an agent's call to set anyone's. A person not above the employee is refused. A new person record has it on and a new agent has it off. |
| HIRE-5 | Turning `may_hire` off moves the proposals waiting on that lead to the next approver. |
| HIRE-6 | A request above a hiring limit goes to the owner whoever the lead is. |
| HIRE-7 | With an agent leading an engineering team (`may_hire` off) under a director team led by an agent with `may_hire` on, a hire into the engineering team waits for the director team's lead, and that lead approves it. No project exists in the organization during the test. |

## Consequences

- An agent lead with the permission can add employees, and so cost, without a person
  seeing each hire first. Agent and task budgets
  ([document 17](../design/organization-data/17-budgets-and-controls.md)) and the
  hiring limits are the bounds, and every hire shows on the owner's Home.
- The dogfood run's check that no agent can approve a hire now holds only for agents
  without the permission.
- An agent that leads a team and lacks the permission does not need to work out whom
  to ask: the request is routed up the chain for it.
