# Design: teams and the org chart

Revision 3, 2026-11-15. Selected target relationships are owned by [organization structure](organization-data/01-organization-structure.md), [projects](organization-data/02-project-structure.md) and [employees](organization-data/04-agent-structure.md). This design applies them to the chart. It does not implement new commands. See the [authority map](documentation-authority.md).

## The user's model, in short

- An organization has a recursive forest of teams. A division, portfolio or guild name does not introduce another entity type.
- Every employee, agent or person, has exactly one required home team. A team can have direct agents, direct people and child teams at once.
- An optional lead must be a direct employee of the team. The lead manages the entire subtree, including child-team leads. An ordinary ancestor-team member has no automatic management authority.
- Explicit person-to-person reporting stays separate from the team tree and must be acyclic. A reporting reference does not remove ancestor-lead responsibility.
- Every project has one accountable owner team and may name contributing teams. Working on a task never requires joining another team.
- Offices place agents; they own neither teams nor projects. People have no execution placement.
- The authenticated owner is a security principal. If represented by an employee record, that record obeys the required home-team rule. An owner bar is a presentation of account authority, not an exemption from employee data constraints.

## The model

All records use stable IDs, `schema_version`, entity `revision` and common creator/update attribution. Stable IDs are UUIDv7. [Numbering](organization-data/06-identity-and-numbering.md) owns readable references and durable allocation. TEAM- and P- numbers are organization-scoped; names are display text, not IDs.

| Entity | Selected fields and meaning |
| --- | --- |
| Team | `id`, `number`, `name`, nullable `parent_team_id`, nullable `lead_employee_id`, `status: active | archived`, `revision`. Null parent means root. `TEAM-<number>` is derived. |
| Employee | `id`, organization-unique lowercase `handle` stored without `@`, `kind: agent | person`, `name`, required `home_team_id`, optional `role_id`, optional `reports_to_employee_id`, body, `status: active | inactive`, `revision`. |
| Retained employee detail | Optional `contact`, `title`, `focus`, `role_text`, `title_in_team` and `offboarded_at`. State is the single `status` field (`active`, `paused` for agents, `inactive`); there is no owner field on an employee. `role_text` does not fabricate a normalized role ID. |
| Project | `id`, `number`, organization-unique `slug`, `name`, required `owner_team_id`, optional `contributing_team_ids`, repository associations and revision. |
| Placement | Agent `employee_id`, `office_id`, `placement_generation`; separate from employee identity. Rebuild from durable placement authority, not from a folder's existence. |
| Execution | Separate `execution.md` selects `model_profile_id`, canonical `model_id`, tools, enabled state and prompt. Profiles contain ordered provider connections; credentials remain office-local. |

An employee has no membership rows in the selected model. `title_in_team` describes its sole home-team appointment. A move changes `home_team_id` and canonical path together, preserving ID/handle/history; it never silently changes task assignment. Null `reports_to_employee_id` means no explicit employee manager recorded. The UI may group these records below an owner bar without inventing an owner employee foreign key.

An employee has no owner field (document 04); no employee field grants login or administrator rights. The built-in HR employee likewise requires a valid home team. Bootstrap must receive or create a valid team through an explicitly chosen setup operation; this design does not choose its name, ID or lead.

## Rules HQ enforces

These are target write invariants, not claims about existing handlers:

- Reject team and reporting cycles, cross-organization references, path/field mismatches and a lead whose home team is elsewhere.
- Employee creation, including owner/HR employee bootstrap, requires a valid home team. A runnable agent additionally needs separate valid execution configuration and placement/readiness evidence.
- A home-team move is one revision-checked, recoverable operation. Resolve an old-team lead appointment before it would become invalid; update paths with the same identity change. An office move requires safe workspace transfer and fencing, independently of membership.
- A project cannot be created or left without its owner team. Optional contributors do not share accountable ownership. Changing the owner updates project tasks' accountable `team_id` in the same recoverable operation and records history, retaining current assignments.
- Archive teams to preserve history. A destructive delete must first resolve child teams, home employees, owned projects/tasks and other referenced records; never cascade employees into teamless records or projects into an unassigned group.
- Inactive/offboarded employees retain profiles, home team, stable ID and handle. Clear/replace live leadership and resolve reporting/assignment references explicitly. New assignment/ownership targets must be active. Do not infer that offboarding deletes files or task worktrees.
- Assignment is separate: every task has accountable `team_id`, optional project, independently nullable `assigned_team_id` and `assignee_id`. Replace the complete assignment pair atomically with expected revision and durable event history. Clearing an assignee preserves accountability and does not itself change task status.
- Messaging checks registered participants under [messaging/chat](organization-data/15-messaging-and-chat.md). Employee identity alone grants no tools, repository access or security authority.

Authorization, reserved messaging addresses and cryptographic identity namespaces are governed by their own contracts. The data model never derives authority from role text, YAML flags or chart position.

## How it changes

Selected canonical organization Git paths:

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

The organization repository must be remote. Register code repositories once in `repositories.yaml`; each project's repository association supplies its required `integration_branch`. One office cache per repository serves all those associations. Canonical employee files and task history are durable; runtime worktrees and indexed projections are not their sole source. Protected messages, private notes, usage and authority require their own durable sources; Git alone is not a full restore.

Target mutation services must validate revisions, commit recoverable related changes and expose pending/rejected outcomes.

## Teams, Members and Org chart

The shared Teams area has an indented Teams index, a mixed Members directory
(All · People · Agents), and one vertical reporting tree. `/teams`,
`/teams/members`, `/teams/org-chart`, `/teams/:teamId` and `/members/:memberId`
use stable IDs. The earlier Teams/People/Reporting lines canvases and their
View menu, Fit/zoom and chart-specific search are replaced. Employee discovery
uses Members and the shell search. [The UX specification](../specs/offices-and-teams-ux.md)
owns row fields, team-page tabs, keyboard interaction and acceptance.

Team rows retain recursive hierarchy, direct employee counts/lead and project
Owner/Contributor roles. Reporting nodes retain explicit reporting, home team
and distinct subtree-lead responsibility. An owner account card is a visual
root, never an employee foreign key; any linked owner employee remains in its
real home team/reporting position. HR is likewise a home-team employee.

Preserve New team, Move team, Move home team, Change manager and accountable
project owner/contributor editing under the selected rules above. Every drag
shortcut has a menu/dialog equivalent. View-switch navigation focuses its
heading; removal focuses a surviving neighbor or stable ancestor. Invalid
home-team/ownership data stays in explicit repair, never a teamless tray.
Offices move to Organization settings and still place agents only.

## In org-proto

### Current-contract evidence at c6e520a

The exact source at this reconciliation's base remains [org_proto::org](https://github.com/vgmello/agentic-org/blob/c6e520a36c4dc7c738c68915fac5e7d5d1cbe804/crates/org-proto/src/org.rs). Existing wire APIs must not be reinterpreted as the target fields:

| Code today | Target |
| --- | --- |
| `Team.owner_team_id`, `lead_member_id`, `version` | Replaced by `parent_team_id`, `lead_employee_id` and revision with validated direct home membership. |
| `Member`, `Membership`, `team.add_member`, `team.remove_member`, `team.set_title` | One home team; the appointment title lives on the employee. No target counterpart for additional memberships or removing the last team. |
| `member.move { membership_id, expected_version, new_membership_id, to_team_id, title_in_team? }` | Atomic remove/add of a membership; replaced by an employee-revision home-team move with coordinated file/lead updates. |
| `ProjectTeam`, `project.add_team`, `project.remove_team`, optional team lists | Require an accountable owner plus optional contributors; no current list-order inference of owner is permitted. Areas remain unselected. |
| `AgentProfile` runtime/model/home/run-mode fields and `office_id` | Separate portable employee identity, execution configuration and placement. Existing managed home remains local server state. Run modes/scheduling are earlier requirements, not selected engines. |
| `member.offboard` / `member.remove` | Existing offboarding cascades memberships and retains record/handle; target retains required home team. Hard delete remains for records without history only, subject to all references. |

Existing `org.snapshot` returns teams, members, memberships, projects/project links and a position; `subscribe { from_position }` resumes there. Keep no-gap reads, version conflicts, command-ID deduplication and queued/blocked outcomes. `command_id` is an uppercase ULID in the code today; in the target every ID is a UUIDv7, command IDs included (identity contract).

Current commands are app-only unless ADR-0003's call-by-call authorization table explicitly permits otherwise. Keep receiving-office authorization before forwarding, enrolled-office checks and the reserved `owner`/`office` and `run:` addresses. The code today also has handle-vs-ID disambiguation and the `owner_protected`, `cycle`, `not_empty`, `not_a_member`, `conflict`, `not_found`, `unknown_recipient`, `snapshot_required`, `expired` and `clock_skew` results, and uses a person's name as the handle; the target has independent handle and name fields. No target UI may claim that unsupported writes already enforce new constraints.

## Plan

| Step | Remaining target work | Responsibility |
| --- | --- | --- |
| T0 | Chart forms and criteria for home team, subtree leads and accountable projects | Product design / app owner |
| T1 | Target contract replacing current memberships/project links | Contract owner |
| T2 | Recoverable domain writes, canonical files and projection | Domain/store owners |
| T3 | Integrate live browser reads, pending outcomes and accessible editing after supported contracts exist | App owner |

Implementation needs its own review. No step is claimed complete by this documentation change.

## Decisions (the user, 2026-11-08)

The earlier decisions established recursive teams, optional human/agent leads, distinct people, reporting and shared project participation. The later IA replaces three chart views with the Teams index, Members directory and one reporting tree. Later organization-data decisions replace multi-membership with one home team and replace symmetric project-team links with one owner plus contributors. The earlier teamless owner is superseded for employee records; authenticated owner authority remains separate. Full earlier requirements and wire discussion remain in [the pre-reconciliation design](https://github.com/vgmello/agentic-org/blob/c6e520a36c4dc7c738c68915fac5e7d5d1cbe804/docs/design/teams-and-org-chart.md). No `Workflows` GitHub App permission is selected here; see the GitHub design.
