# Design: teams and the org chart

Revision 2, 2026-11-15. Selected target relationships are owned by [organization structure](organization-data/01-organization-structure.md), [projects](organization-data/02-project-structure.md) and [employees](organization-data/04-agent-structure.md). This design applies them to the chart. It does not enact a migration or implement new commands. See the [authority map](documentation-authority.md).

## The user's model, in short

- An organization has a recursive forest of teams. A division, portfolio or guild name does not introduce another entity type.
- Every employee, agent or person, has exactly one required home team. A team can have direct agents, direct people and child teams at once.
- An optional lead must be a direct employee of the team. The lead manages the entire subtree, including child-team leads. An ordinary ancestor-team member has no automatic management authority.
- Explicit person-to-person reporting stays separate from the team tree and must be acyclic. A reporting reference does not remove ancestor-lead responsibility.
- Every project has one accountable owner team and may name contributing teams. Working on a task never requires joining another team.
- Offices place agents; they own neither teams nor projects. People have no execution placement.
- The authenticated owner is a security principal. If represented by an employee record, that record obeys the required home-team rule. An owner bar is a presentation of account authority, not an exemption from employee data constraints.

## The model

All records use stable IDs, `schema_version`, entity `revision` and common creator/update attribution. New stable IDs are UUIDv7; existing IDs remain unchanged. [Numbering](organization-data/06-identity-and-numbering.md) owns readable references and durable allocation. TEAM- and P- numbers are organization-scoped; names are display text, not IDs.

| Entity | Selected fields and meaning |
| --- | --- |
| Team | `id`, `number`, `name`, nullable `parent_team_id`, nullable `lead_employee_id`, `status: active | archived`, `revision`. Null parent means root. `TEAM-<number>` is derived. |
| Employee | `id`, organization-unique lowercase `handle` stored without `@`, `kind: agent | person`, `name`, required `home_team_id`, optional `role_id`, optional `reports_to_employee_id`, body, `status: active | inactive`, `revision`. |
| Retained employee detail | Optional `contact`, `title`, `focus`, `role_text`, `title_in_team`, plus `is_owner`, optional agent-only `agent_status` and `offboarded_at`. `role_text` does not fabricate a normalized role ID. |
| Project | `id`, `number`, organization-unique `slug`, `name`, required `owner_team_id`, optional `contributing_team_ids`, repository associations and revision. |
| Placement | Agent `employee_id`, `office_id`, `placement_generation`; separate from employee identity. Rebuild from durable placement authority, not from a folder's existence. |
| Execution | Separate `execution.md` selects `model_profile_id`, canonical `model_id`, tools, enabled state and prompt. Profiles contain ordered provider connections; credentials remain office-local. |

An employee has no membership rows in the selected model. `title_in_team` describes its sole home-team appointment. A move changes `home_team_id` and canonical path together, preserving ID/handle/history; it never silently changes task assignment. Null `reports_to_employee_id` means no explicit employee manager recorded. The UI may group these records below an owner bar without inventing an owner employee foreign key.

The owner employee marker preserves historical data; it does not grant login or administrator rights. The built-in HR employee likewise requires a valid home team. Bootstrap must receive or create a valid team through an explicitly chosen setup operation; this design does not choose its name, ID or lead.

## Rules HQ enforces

These are target write/import invariants, not claims about existing handlers:

- Reject team and reporting cycles, cross-organization references, path/field mismatches and a lead whose home team is elsewhere.
- Employee creation, including owner/HR employee bootstrap, requires a valid home team. A runnable agent additionally needs separate valid execution configuration and placement/readiness evidence.
- A home-team move is one revision-checked, recoverable operation. Resolve an old-team lead appointment before it would become invalid; update paths with the same identity change. An office move requires safe workspace transfer and fencing, independently of membership.
- A project cannot be created or left without its owner team. Optional contributors do not share accountable ownership. Changing the owner updates project tasks' accountable `team_id` in the same recoverable operation and records history, retaining current assignments.
- Archive teams to preserve history. A destructive delete must first resolve child teams, home employees, owned projects/tasks and other referenced records; never cascade employees into teamless records or projects into an unassigned group.
- Inactive/offboarded employees retain profiles, home team, stable ID and handle. Clear/replace live leadership and resolve reporting/assignment references explicitly. New assignment/ownership targets must be active. Do not infer that offboarding deletes files or task worktrees.
- Assignment is separate: every task has accountable `team_id`, optional project, independently nullable `assigned_team_id` and `assignee_id`. Replace the complete assignment pair atomically with expected revision and durable event history. Clearing an assignee preserves accountability and does not itself change task status.
- Messaging checks registered participants under [messaging/chat](organization-data/15-messaging-and-chat.md). Employee identity alone grants no tools, repository access or security authority.

Current authorization, reserved messaging addresses and cryptographic identity namespaces remain governed by existing contracts. A data-model migration must preserve them rather than deriving new authority from role text, YAML flags or chart position.

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

Target mutation services must validate revisions, commit recoverable related changes and expose pending/rejected outcomes without silently selecting one old membership as home. Import of legacy multi-membership members requires an explicit home-team choice; preserve unselected associations in staging. Existing task IDs/numbers and history survive; old B-/REQ- references require explicit mappings rather than renaming prefixes.

## The chart

One chart has three views:

1. **Teams:** recursive cards show TEAM- reference/name, direct employees separated into Agents and People, direct lead, child-team count, and projects marked **Owner** or **Contributor**. A person appears once at their home team. A project can appear on several cards with one clear owner; no synthetic set of project co-leads is inferred.
2. **People:** the same team tree with human employees only. Agent leads may still be named as agent leads; empty teams remain compact cards so hierarchy remains visible.
3. **Reporting lines:** explicit reporting relationships, distinct from subtree lead responsibility. Records with no explicit manager can appear under the owner presentation with that absence stated. Every employee appears once.

The owner security bar can appear above every view. A linked owner employee still appears/counts at its home team. The built-in HR shortcut can also appear above the chart while its employee remains visible at its required home team. Former members retain historical home-team context in their own view, rather than becoming teamless.

Support collapse/expand, keyboard and list alternatives, search preserving selection across tabs, menus for every drag action, and office highlighting of agents without changing the tree. Forms require a home team or project owner team and explain stale state, failed validation and pending office outbox outcomes. No active **Not in a team** or **Unassigned projects** group is valid; unresolved legacy imports belong to a repair/staging view.

## In org-proto

### Current-contract evidence at c6e520a (pre-migration)

The exact source at this reconciliation's base remains [org_proto::org](https://github.com/vgmello/agentic-org/blob/c6e520a36c4dc7c738c68915fac5e7d5d1cbe804/crates/org-proto/src/org.rs). Existing wire APIs must not be reinterpreted as the target fields:

| Existing contract | Target adaptation required |
| --- | --- |
| `Team.owner_team_id`, `lead_member_id`, `version` | Map parent/lead to `parent_team_id`, `lead_employee_id` and revision with validated direct home membership. |
| `Member`, `Membership`, `team.add_member`, `team.remove_member`, `team.set_title` | Choose one home team explicitly; retained appointment title lives on employee. No direct target counterpart for arbitrary additional memberships or removing the last team. |
| `member.move { membership_id, expected_version, new_membership_id, to_team_id, title_in_team? }` | Preserve current atomic remove/add semantics on this wire; selected target needs an employee-revision home-team move and coordinated file/lead updates. Do not invent a replacement command. |
| `ProjectTeam`, `project.add_team`, `project.remove_team`, optional team lists | Require an accountable owner plus optional contributors; no current list-order inference of owner is permitted. Areas remain unselected. |
| `AgentProfile` runtime/model/home/run-mode fields and `office_id` | Separate portable employee identity, execution configuration and placement. Existing managed home remains local server state. Run modes/scheduling are earlier requirements, not selected engines. |
| `member.offboard` / `member.remove` | Existing offboarding cascades memberships and retains record/handle; target retains required home team. Hard delete remains for records without history only, subject to all references. |

Existing `org.snapshot` returns teams, members, memberships, projects/project links and a position; `subscribe { from_position }` resumes there. Retain no-gap reads, version conflicts, command-ID deduplication, queued/blocked outcomes and `org.changed`/`org.rejected`. `command_id` remains the existing uppercase ULID wire identity; selecting UUIDv7 for new stable domain IDs does not change this protocol or signing namespaces.

Current commands are app-only unless ADR-0003's call-by-call authorization table explicitly permits otherwise. Preserve receiving-office authorization before forwarding, enrolled-office checks, reserved `owner`/`office` and `run:` addresses, handle-vs-ID disambiguation, `owner_protected`, `cycle`, `not_empty`, `not_a_member`, `conflict`, `not_found`, `unknown_recipient`, `snapshot_required`, `expired` and `clock_skew` semantics on that wire. Existing people-name-as-handle behavior needs explicit migration to independent handle/name fields. No target UI may claim that unsupported writes already enforce new constraints.

## Plan

| Step | Remaining target work | Responsibility |
| --- | --- | --- |
| T0 | Chart forms and criteria for home team, subtree leads and accountable projects | Product design / app owner |
| T1 | Versioned contract and explicit migration mapping from current memberships/project links | Contract owner |
| T2 | Recoverable domain writes, canonical files, projection and preserved legacy history | Domain/store owners |
| T3 | Integrate live browser reads, pending outcomes and accessible editing after supported contracts exist | App owner |

Implementation and migration need their own review. No step is claimed complete by this documentation change.

## Decisions (the user, 2026-11-08)

The earlier decisions established recursive teams, optional human/agent leads, distinct people, three chart views, reporting and shared project participation. Later organization-data decisions replace multi-membership with one home team and replace symmetric project-team links with one owner plus contributors. The earlier teamless owner is superseded for employee records; authenticated owner authority remains separate. Full earlier requirements and wire discussion remain in [the pre-reconciliation design](https://github.com/vgmello/agentic-org/blob/c6e520a36c4dc7c738c68915fac5e7d5d1cbe804/docs/design/teams-and-org-chart.md). No `Workflows` GitHub App permission is selected here; see the GitHub design.
