---
id: doc-38
title: 'Design: Teams, People and Org Chart'
type: specification
created_date: '2026-11-21T00:19:00Z'
updated_date: '2026-11-21T00:19:00Z'
tags:
  - design
  - teams
  - org-chart
  - people
  - v1
---
> [!NOTE] Review note (2026-11-21)
> Reviewed against Organization Data contracts (`01`, `04`) and current `v1` implementation.
> - **Stale 'What the code does today' section:** `B1b` already deleted the legacy `memberships` table, and `B2.1` (`7f3000bc`) landed the single-home-team `org_employees`, `org_teams`, and `org_team_leads` store records.
> - Remaining B2.2–B2.4 wire/HTTP/CLI/UI delivery is tracked in `TASK-34`, `TASK-35`, and `TASK-5` (resolving browser-to-office bridge ownership Q8).

# Design: teams and the org chart

The relationships are owned by [organization structure](organization-data/01-organization-structure.md),
[projects](organization-data/02-project-structure.md) and
[employees](organization-data/04-agent-structure.md). This design applies them to the
chart and to the rules HQ enforces when the structure changes. The screens and their
acceptance criteria are in the [offices and teams UX](../specs/offices-and-teams-ux.md).

## The model in short

- An organization has a recursive forest of teams. A division, portfolio or guild
  name does not introduce another entity type.
- Every employee, agent or person, has exactly one required home team. A team can
  have direct agents, direct people and child teams at once.
- An optional lead is a direct employee of the team and manages its entire subtree,
  including child-team leads. An ordinary member of an ancestor team has no
  management authority.
- Explicit person-to-person reporting is separate from the team tree and acyclic. A
  reporting line does not remove an ancestor lead's responsibility.
- Teams and projects live side by side (owner, 2026-11-18). A team is created on its
  own, at any depth, before any project exists: an executive team, the teams under
  it, the teams under those. A team needs no project and may be accountable for none,
  one or several. A project always needs a team; a team never needs a project.
- Every project has one accountable owner team and may name contributing teams.
  Working on a task never requires joining another team.
- Offices place agents; they own neither teams nor projects. People have no placement.
- The owner is a person: an ordinary employee with one home team, whose account the
  host records as the organization's owner. Every organization has exactly one, and
  the owner can hand ownership to another person who has an account. An owner bar on
  the chart presents that person; it is not an exemption from the employee rules.
- Creating an organization makes one top-level team named after it, without asking
  for a team. The creator's person is its lead, and the built-in HR lead is its
  other member. Creating a project creates its team by default: a new team named
  after the project, under the top-level team, accountable for the project. An
  existing team can be chosen instead, and a team can still be created directly.

## The model

Every record has a stable UUIDv7 ID, `schema_version`, entity `revision` and common
creator and update attribution. [Numbering](organization-data/06-identity-and-numbering.md)
owns readable references: `TEAM-` and `P-` numbers are allocated by HQ for the whole
organization. Names are display text, not IDs.

| Entity | Fields and meaning |
| --- | --- |
| Team | `id`, `number`, `name`, nullable `parent_team_id`, nullable `lead_employee_id`, `status: active \| archived`, `revision`. A null parent means a root team. `TEAM-<number>` is derived. |
| Employee | `id`, organization-unique lowercase `handle` stored without `@`, `kind: agent \| person`, `name`, required `home_team_id`, optional `role_id`, optional `reports_to_employee_id`, body, `status: active \| paused \| inactive` (`paused` for agents only), `revision`. |
| Employee detail | Optional `contact`, `title`, `focus`, `role_text`, `title_in_team` and `offboarded_at`. There is no owner field. `role_text` never produces a normalized role ID. |
| Project | `id`, `number`, organization-unique `slug`, `name`, required `owner_team_id`, optional `contributing_team_ids`, repository associations, revision. |
| Placement | An agent's `employee_id`, `office_id` and `placement_generation`, separate from its identity and rebuilt from durable placement authority, never from a folder's existence. |
| Execution | A separate `execution.md` selecting the model profile, model, tools, enabled state and prompt ([document 11](organization-data/11-model-profiles-and-agent-execution.md)). Credentials stay office-local. |

An employee has no membership rows. `title_in_team` describes the one home-team
appointment. A move changes `home_team_id` and the canonical path together and keeps
ID, handle and history; it never changes task assignment by itself. A null
`reports_to_employee_id` means no manager is recorded. The UI may group records
below the owner bar without inventing a manager reference.

## Rules HQ enforces

- Reject team and reporting cycles, references to another organization, path and
  field mismatches, and a lead whose home team is elsewhere.
- Creating an employee requires a valid home team. The owner's person and the HR
  lead get the top-level team made with the organization. An agent is created only
  by a confirmed hire from someone who may hire; the built-in HR lead is the one exception. A runnable agent
  also needs a valid execution configuration and placement.
- A home-team move is one revision-checked, recoverable operation. A lead
  appointment that the move would invalidate is resolved first, and paths change
  with the move. Moving an agent to another office is separate: it needs a safe
  workspace transfer and fencing.
- A project cannot be created or left without its owner team. Creating a project
  with no team named creates its team in the same operation. Contributors do not
  share accountability. Changing the owner updates the accountable `team_id` of the
  project's tasks in the same recoverable operation and records history; current
  assignments stay.
- Teams are archived to keep history. Deleting one first resolves its child teams,
  home employees, owned projects and tasks and every other reference. Employees are
  never cascaded into teamless records, nor projects into an unassigned group.
- An inactive employee keeps profile, home team, ID and handle. Leadership,
  reporting and assignment references are cleared or replaced explicitly. New
  assignment and ownership targets must be active. Offboarding does not delete files
  or task worktrees.
- Assignment is separate from structure: every task has an accountable `team_id`, an
  optional project and an independently nullable `assigned_team_id` and
  `assignee_id`, replaced together at an expected revision with a history event
  ([document 12](organization-data/12-task-assignment-and-history.md)). Clearing the
  assignee keeps accountability and does not change status.
- Messaging checks registered participants under
  [messaging](organization-data/15-messaging-and-chat.md). Employee identity grants
  no tools, repository access or security authority. Authority never comes from role
  text, a YAML flag or a position on the chart.

## Files

Canonical paths in the organization repository:

```text
organization.yaml
roles/<role-id>.md
teams/<root-team-id>/
├── team.yaml
├── agents/<employee-id>/
│   ├── profile.md
│   └── execution.md
├── people/<employee-id>/profile.md
└── teams/<child-team-id>/...
projects/<project-id>/project.yaml
```

The organization repository is remote. Code repositories are registered once in
`repositories.yaml`; each project's association with a repository carries its
required `integration_branch`, and one office cache per repository serves every
association. Protected messages, private notes, usage and authority have their own
durable sources; Git alone is not a full restore.

## Teams, Members and the org chart

The Teams area has an indented Teams index, a mixed Members directory (All · People ·
Agents) and one vertical reporting tree. `/teams`, `/teams/members`,
`/teams/org-chart`, `/teams/:teamId` and `/members/:memberId` use stable IDs.
Employee discovery uses Members and the shell search.

Team rows show the hierarchy, direct employee counts, the lead and the team's Owner
or Contributor role on projects. Reporting nodes show explicit reporting, the home
team and subtree-lead responsibility. The owner bar is a visual root, never a
manager reference; the owner's person stays in its real home team and reporting
position. HR is likewise an employee in a home team.

The actions are New team, Move team, Move home team, Change manager, and editing a
project's owner and contributors. Every drag has a menu or dialog equivalent.
Switching view focuses its heading; removing an item focuses a surviving neighbour or
ancestor. Invalid home-team or ownership data appears in an explicit repair state,
never in a teamless tray. Offices are managed in Organization settings and only
place agents.

## What the code does today

`crates/org-proto/src/org.rs` and `crates/org-office/src/organization` implement an
older model. It is replaced, not adapted (implementation plan B2):

| Code | Replaced by |
| --- | --- |
| `Team.owner_team_id`, `lead_member_id`, `version` | `parent_team_id`, `lead_employee_id`, `revision`, with the lead a direct member |
| `Member`, `Membership`, `team.add_member`, `team.remove_member`, `team.set_title`, `member.move` | One home team per employee and a revision-checked home-team move |
| `ProjectTeam`, `project.add_team`, `project.remove_team` | A required owner team plus optional contributors |
| `AgentProfile` runtime, model, home and run-mode fields and `office_id` | Separate employee identity, execution configuration and placement |
| A person's name used as the handle | Independent `handle` and `name` |
| ULID `command_id` | UUIDv7 |

The snapshot-and-subscribe read, version conflicts, command-ID deduplication and the
refusals `owner_protected`, `cycle`, `not_empty`, `conflict`, `not_found`,
`snapshot_required`, `expired` and `clock_skew` carry over to the new model. The
rules in `crates/org-office/src/rules` have tests worth keeping where a rule is
unchanged. The UI dialogs and charts in `apps/agentic-org-ui/src/lib/components`
run against fakes.
