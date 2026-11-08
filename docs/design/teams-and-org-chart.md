# Design: teams and the org chart

Status: **approved by the user** 2026-11-08 (decisions at the end); amended by ada 2026-11-08 (agents as members, `member.move`, one handle namespace). Owner: ada. It follows the user's model (2026-11-08). It builds on `offices-hq-and-satellites.md`: teams are organization data, so HQ owns them and every office has a copy.

## The user's model, in short
- An organization is made of **teams**, and **a team can be owned by another team**: just teams and their sub-teams.
- **There are only teams** *(the user: "guilds are not a thing, it's just teams"; divisions aren't a concept either)*. A "division" is just a team that owns other teams.
- **Members are agents or people (humans), and each can be in several teams.** People are tracked and shown differently from agents. Only agents are placed in offices.
- **Agents can only interact with people who have been added** to the organization.
- **A lead is optional, and can be an agent or a person.** A team's lead is also the lead of that team's projects.
- **The owner (you) isn't a member of any team.** You're the organization's special user:
  - you have full rights everywhere (every team, member and project)
  - any agent can message you, ask you or assign you work
  - you're shown separately in the chart, not on team cards
- **A project is usually worked on by one team, but several teams can share it**, through its own relationship.
- **Reporting lines stay** *(the user: "reports to is still valid")*: each agent or person can report to someone, with the owner at the top. They're separate from teams: the team chart doesn't replace them.
- **One chart, three ways to look at it:** the team tree, the same tree showing **only people** (the human org chart), and the **reporting lines**.

## The model
- **Team** `{ id, name, owner_team_id?, lead_member_id?, version }`
  - `owner_team_id` empty means a top-level team.
  - **Ownership forms a tree:** each team has at most one owner, and a team can't own itself or any team above it. HQ rejects a move that would create a loop (`cycle`).
  - **The lead** is optional: an agent or a person who's a member of that team. **The team's lead is also the lead of the team's projects.** A project shared by several teams has each of those teams' leads.
- **Member** `{ id, kind: agent | person, name, contact?, owner?, office_id? (agents only), agent? (agents only), reports_to_member_id?, version }`
  - `reports_to_member_id`: who they report to (an agent or a person). Empty means directly to the owner. The owner reports to no one.
  - **An agent** is an AI employee. Its record also says which office it's placed in.
  - `agent` is an agent's profile, `AgentProfile { role, title, focus, runtime, home, messaging_name, status }`. In org-proto it's `agent: Option<AgentProfile>`, set for agents and empty for people.
  - **A person** is a human added to the organization. `contact` is how they're reached, e.g. an email; that's for later.
- **The owner** is a person record with `owner: true`, created with the organization. It's the organization's **special user**:
  - **not a member of any team**, so it's never stored as a membership or counted in a team's members;
  - it has **full rights everywhere** (every team, member and project);
  - **any agent can message it, ask it or assign it work**;
  - it **can't be removed**, added to a team, or made a team's lead. Any of those returns `owner_protected`.
- **Membership** `{ id, team_id, member_id, title_in_team?, version }`: an agent or a person can be in **any number** of teams. `title_in_team` is a free-text label, e.g. "frontend lead". It's named that way so it isn't confused with the organization's role catalogue.
- **Project team** `{ id, project_id, team_id, version }`: the teams working on a project. Usually one, sometimes several. A project with none is **unassigned**, e.g. one created before any teams exist; the charts show those in their own group, and the app prompts to assign them.
- **Who agents may interact with:**
  - An agent may message, `ask` or assign a task to **another agent** of the organization, or to a **person who's a member** of the organization. The owner is always reachable.
  - Anyone else is rejected with `unknown_recipient`. This is the only way agents reach humans, and it's what makes adding a person meaningful.
  - **Where it's checked:** messages and tasks are office-first (offices design), so the **sender's office** checks the recipient against its own copy, at the organization position it has applied, and rejects at once. HQ checks again when it forwards to another office. A recipient removed in between gets the message dead-lettered, and the sender gets a `delivery_failed` notice.
  - **When a member is removed:** messages still queued for them are dead-lettered the same way. Their open tasks are reclaimed to HQ and become unassigned (offices design, reclaim), and they're shown to the owner for reassignment.

## Rules HQ enforces
- no ownership loops, and **no reporting loops** (nobody reports to themselves, directly or through others): `cycle`
- a lead must be a member of the team (`not_a_member` otherwise). **Removing the lead from the team clears the lead**, in the same event, and the team's version goes up.
- **deleting a team** that owns teams is refused (`not_empty`) until they're moved or deleted.
  - Its memberships and project links go with it.
  - A project left with no team becomes unassigned.
- **removing a member** (an agent or a person) removes their memberships, clears them as lead of any team, and makes whoever reported to them report to the owner, in the same event. Each affected team's and member's version goes up. **The owner can't be removed.**
- **names:** a team's name is unique among teams with the same owner, (top-level teams count as siblings of each other). A create, rename or move that would clash returns `already_exists`.
- **handles:** there's **one handle namespace**, compared ignoring case. It holds agents' `messaging_name`s, people's names and the reserved `owner`, the owner's handle. Adding or renaming an agent or a person whose handle is already taken returns `already_exists`, whichever kind holds it: an agent can't take a person's name, and a person can't take an agent's `messaging_name`.
- adding someone who's already in the team, or linking a team that already works on the project, returns `already_exists`.

## How it changes
These are organization commands, so they go through HQ and every office gets the resulting events (offices design):
- `team.create { team_id, name, owner_team_id? }`, `team.rename`, `team.move { team_id, owner_team_id? }`, `team.set_lead`, `team.delete`
- `team.add_member { membership_id, team_id, member_id, title_in_team? }`, `team.remove_member { membership_id }`, `team.set_title { membership_id, title_in_team? }`
- `project.add_team { project_team_id, project_id, team_id }`, `project.remove_team { project_team_id }`
- **Members:**
  - agents are added with **`member.add_agent { member_id, name, office_id, profile }`**. Hiring (the HR flow, its own design) issues it.
  - an agent's profile is edited with `member.update_agent`, and its status set with `member.set_agent_status`
  - people are added with `member.add_person { member_id, name, contact? }` and edited with `member.update_person { member_id, name?, contact?, clear_contact? }`
  - **`member.set_reports_to { member_id, reports_to_member_id? }`** sets a reporting line (`cycle` on a loop; `owner_protected` for the owner)
  - **`member.remove { member_id }`** removes anyone except the owner. Offboarding an agent issues it, with the same cascade.
  - **`member.move { membership_id, expected_version, new_membership_id, to_team_id, title_in_team? }`** (`title_in_team` is the new membership's title; the old title isn't carried over) moves a member from one team to another **atomically**: one event removes the old membership and adds the new one. If the member led the team they leave, removing them clears the lead, as above.

Each command carries a client-made id for anything it creates, and the version of what it edits, as in the offices design:
- `team.move` expects the moved team's version. The new owner and the loop check are judged against HQ's current state.
- `team.rename`, `team.set_lead` and `team.delete` expect the team's version.
- Membership commands name the membership by its **id** and expect its version (`set_title`, `remove_member`), so a stale queued command never hits a membership that was removed and re-added (that one has a new id). `add_member` needs only that the team and member exist. A membership is its own entity, so adding or removing members doesn't change the team's version (clearing a departing lead does).
- Project-team links are their own entities too, with ids: `add_team` needs the project and team to exist, and `remove_team` names the link by id and expects its version.
- `member.remove`, `member.update_person`, `member.update_agent` and `member.set_agent_status` expect the member's version. `member.move` names the old membership by id and expects its version, like `remove_member`.

A command queued in a satellite's outbox can find its team, owner or member deleted, or its version stale. It's then rejected with `not_found` or `conflict`, and shown as blocked (offices design).

**Reading:** the app reads from its local office's copy with **`org.snapshot`**, the one read call. It returns the teams, members (with the owner marked), memberships, projects and project-team links, together with the **position** it reflects. `subscribe { from_position }` then resumes from exactly that point, so no change slips in between.

## The chart
One org chart over the same data, with three ways to look at it:
1. **Teams:** the ownership tree of all teams.
   - Each team is a card showing its name, lead, members and projects.
   - **Agents and people look different** on the card: separate rows, different styles.
   - A member in several teams appears in each, with a "+2 teams" badge listing the others.
   - A shared project shows on each of its teams' cards, with a "shared" badge.
   - **The owner isn't on any card.** It appears separately, at the top, as the organization's owner.
   - Collapse and expand any branch. Drag a team onto another to move it, with a confirmation, and HQ checks for loops.
   - A filter by office highlights agents placed in a given office.
2. **People** (the human org chart): the same team tree **showing only people**. It's where you add a person, and it shows plainly that **agents can only reach people listed in the organization**.
3. **Reporting lines:** who reports to whom, with the owner at the top. Agents and people look different here too. It's where you set someone's manager.

Editing (create, add member, move, add a person, link a project, set who someone reports to) is available from all three. Changes waiting in an offline satellite's outbox show as **pending**, and rejected ones as **blocked**, as in the offices design.

## In org-proto
- Types: `Team`, `Member` (with `kind`, an `owner` flag and, for agents, `office_id` and `agent: Option<AgentProfile>`), `AgentProfile`, `Membership`, `ProjectTeam`, `Position`, `CommandAccepted` (`committed` with a position, or `queued` while HQ is unreachable).
- Calls: the commands above, `org.snapshot` (the read, with its position) and `subscribe { from_position }`.
- Events: one per command.
- Error codes: `cycle`, `not_empty`, `not_a_member` (a lead who isn't in the team), `owner_protected` (removing the owner, adding them to a team, making them a lead, or giving them someone to report to), `not_found` and `conflict` (both also used by the offices design), `unknown_recipient` (messaging and tasks, ADR-0003) `snapshot_required` (`subscribe` can't resume from the given position), and `expired` / `clock_skew` (any command, offices design), plus the existing ones.
- Events: `org.changed` (with the `command_id`) and **`org.rejected`** (a queued command HQ rejected, with its `command_id` and error), so a satellite's app can show it as blocked.
- Names (teams, people, agents) follow the organization-name rules (trimmed, not empty, no control characters: `invalid_name`), and sibling team names compare ignoring case. An agent's `messaging_name` has its own format: 1 to 64 ASCII letters, digits, `-`, `_` and `.`, starting with a letter or digit (`invalid_name` otherwise).
- `already_exists` covers a sibling team name, a membership or project-team link that already exists, and a handle that's already taken (agents' `messaging_name`s, people's names, `owner`; one namespace, ignoring case).
- **The crate (`org_proto::org`) is the reference for the exact shapes.**
- `project.create` gains `team_ids?` when the projects contract (GitHub design, G1) is written. It isn't in the crate yet.
- `member.add_agent`, `member.update_agent`, `member.set_agent_status`, `member.move` and `AgentProfile` aren't in the crate yet either, and neither is the handle rule for the existing `member.add_person` / `member.update_person` (`already_exists` on a clash with an agent's `messaging_name` or `owner`). server-dev's `proto-agents` change adds them.
- Messaging (ADR-0003) and tasks check recipients against members: agents, and people who've been added.

## Plan
| Step | What | Who |
| --- | --- | --- |
| T0 | UX for the chart's three views and the editing flows | Eden (product designer), with app-frontend-dev. Her spec needs updating for the owner, people and shared projects |
| T1 | The contract in `org-proto` (`org_proto::org`). `team_ids?` on `project.create` follows with the projects contract (G1) | ada |
| T2 | Teams, members (the owner, people, agents with `AgentProfile`), memberships and project-team links as commands, events and apply code in `org-office`, with the rules above and tests (loops, non-empty delete, multi-team membership, the owner can't be removed or lead, `member.move` atomicity and lead clearing, the handle namespace in both directions) | server-dev, after the HQ event log |
| T3 | The app: the chart's teams, people and reporting-lines views, and editing. Built on a mock with the T1 types first, then on the office | app-frontend-dev |

Each step: its own branch, an independent review, my go, then merge into `v1`.

## Decisions (the user, 2026-11-08)
- Teams own teams. **There are only teams:** no guilds, no divisions, no labels.
- Members (agents and people) can be in several teams. **Only agents are placed in offices.**
- **Leads are optional,** and can be an agent or a person. A team's lead also leads the team's projects.
- **The owner is the organization's special user:** in no team, full rights everywhere, reachable and assignable by any agent, and shown separately.
- **People are tracked and shown separately from agents.** Agents can only interact with people who've been added to the organization.
- **The human org chart is the team chart showing only people.**
- **Reporting lines stay**, separate from teams, with the owner at the top.
- **No `Workflows` permission** for the GitHub App (recorded here for completeness; see the GitHub design).
- **Projects can be shared by several teams** (usually one), through their own relationship.
