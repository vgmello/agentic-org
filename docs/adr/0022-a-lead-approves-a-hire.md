# ADR-0022 · A lead approves a hire; an agent lead needs the permission to hire

Status: **decided by the owner 2026-11-18**, in answer to the hiring point of
[REQ-07](../specs/req-07-composable-process-packages-and-team-blueprints.md): "Not a
person, a lead approves every hire. If I'm the first lead in the team because I'm a
person, I have to approve. If an agent is the lead of the team, it's a flag in the
agent to enable it to hire someone else." Implementation pending.

## Context

Until now a person approved every hire: the owner, or a person in the lead chain of
the team the new agent joins. No agent could approve, and no field on a record granted
the right. A project that follows a process package is led by seed agents who decide,
after the specification is clear, which specialists the work needs. With approval
reserved to people, every such hire waited for a person even when the team's lead was
an agent the owner trusted to staff its team.

## Decision

1. **A lead approves every hire.** The approver is the nearest lead, going up from the
   team the new agent joins, who may approve; above the top team, and at any time, the
   organization's owner.
2. **A lead who is a person may approve.**
3. **A lead who is an agent may approve only when its `may_hire` is on.** `may_hire` is
   a field of the agent's profile, off by default
   ([document 04](../design/organization-data/04-agent-structure.md#who-approves-a-hire)).
4. **Only the owner, or a person above the agent, turns `may_hire` on or off.** The
   change is attributed and kept in history. No agent sets it, for itself or for
   another, and the built-in HR lead never has it.
5. **A lead who may approve hires for a team hires into it without a second
   approval.** The proposal is still drafted and recorded, with that lead as requester
   and approver. This replaces the rule that a requester's own request went one level
   up.
6. **Everything else about hiring is unchanged.** HR drafts and never approves.
   Product code provisions, and only after approval. Hiring limits set in Settings
   apply to every lead, person or agent, and a request above a limit goes to the owner.
7. **Non-negotiable 9 is restated.** Authority comes from the caller's place in the
   organization. Two records carry it: a team's lead appointment and an agent lead's
   permission to hire. Each is given only by someone already above the holder, and the
   office checks it on every call.

## Rejected

- **Keeping approval with people only.** It makes a person the bottleneck of a team an
  agent leads, which is the case process packages create on purpose.
- **Letting any lead agent approve.** A lead appointment says who directs a team, not
  who may spend on new employees. The owner wants that as a separate, explicit switch.
- **A permission in the process package.** A package is content and grants nothing
  ([ADR-0021](0021-process-packages-are-content.md)).

## Acceptance criteria

APPR-1 to APPR-4, APPR-8 and APPR-9 in the
[hire flow](../specs/our-org-in-the-app.md), and:

| ID | Passing evidence |
| --- | --- |
| HIRE-1 | With a person leading the team, an agent's hire request waits for that person; nothing is provisioned before they approve. |
| HIRE-2 | With an agent leading the team and `may_hire` off, the request goes to the next lead above who may approve, or to the owner. |
| HIRE-3 | With an agent leading the team and `may_hire` on, that agent's own request is approved by it and provisioned, and the hire records it as requester and approver. |
| HIRE-4 | An agent's call to set `may_hire`, on itself or another agent, is refused. A person not above the agent is refused. |
| HIRE-5 | Turning `may_hire` off moves the proposals waiting on that agent to the next approver. |
| HIRE-6 | A request above a hiring limit goes to the owner whoever the lead is. |

## Consequences

- An agent lead with the permission can add employees, and so cost, without a person
  seeing each hire first. Agent and task budgets
  ([document 17](../design/organization-data/17-budgets-and-controls.md)) and the
  hiring limits are the bounds, and every hire shows on the owner's Home.
- The dogfood run's check that no agent can approve a hire now holds only for agents
  without the permission.
