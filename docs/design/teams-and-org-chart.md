# Design: teams and the org chart

Status: **approved by the user** 2026-11-08 (decisions at the end). Owner: ada. It follows the user's model (2026-11-08). It builds on `offices-hq-and-satellites.md`: teams are organization data, so HQ owns them and every office has a copy.

## The user's model, in short
- An organization is made of **teams**, and **a team can be owned by another team**: just teams and their sub-teams.
- **"Division" and "guild" aren't concepts. They're labels on a team**, nothing more.
- **Members are agents or people (humans), and each can be in several teams.** People are tracked and shown differently from agents. Only agents are placed in offices.
- **Agents can only interact with people who have been added** to the organization.
- **A lead is optional, and can be an agent or a person.** A team's lead is also the lead of that team's projects.
- **The owner (you) isn't a member of any team.** You're the organization's special user:
  - you have full rights everywhere (every team, member and project)
  - any agent can message you, ask you or assign you work
  - you're shown separately in the chart, not on team cards
- **A project is usually worked on by one team, but several teams can share it**, through its own relationship.
- **One chart, three ways to look at it:**
  - the team tree
  - the teams labelled "guild"
  - the same tree filtered to show **only people** (the human org chart)

## The model
- **Team** `{ id, name, labels, owner_team_id?, lead_member_id?, description?, version }`
  - `labels` is a set of free-text labels, e.g. `division` or `guild`. They change nothing about the rules. The chart uses `guild` to show the guild view, and filters by any label.
  - `owner_team_id` empty means a top-level team.
  - **Ownership forms a tree:** each team has at most one owner, and a team can't own itself or any team above it. HQ rejects a move that would create a loop (`cycle`).
  - **The lead** is optional: an agent or a person who's a member of that team. **The team's lead is also the lead of the team's projects.** A project shared by several teams has each of those teams' leads.
- **Member** `{ id, kind: agent | person, name, contact?, owner?, office_id? (agents only), version }`
  - **An agent** is an AI employee. Its record also says which office it's placed in.
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
- no ownership loops
- a lead must be a member of the team (`not_a_member` otherwise). **Removing the lead from the team clears the lead**, in the same event, and the team's version goes up.
- **deleting a team** that owns teams is refused (`not_empty`) until they're moved or deleted.
  - Its memberships and project links go with it.
  - A project left with no team becomes unassigned.
- **removing a member** (an agent or a person) removes their memberships and clears them as lead of any team, in the same event, and each affected team's version goes up. **The owner can't be removed.**
- **names:** a team's name is unique among teams with the same owner, whatever their labels (top-level teams count as siblings of each other). A create, rename or move that would clash returns `already_exists`.
- adding someone who's already in the team, or linking a team that already works on the project, returns `already_exists`.

## How it changes
These are organization commands, so they go through HQ and every office gets the resulting events (offices design):
- `team.create { team_id, name, labels?, owner_team_id? }`, `team.rename`, `team.move { team_id, owner_team_id? }`, `team.set_lead`, `team.set_labels { team_id, labels }`, `team.delete`
- `team.add_member { membership_id, team_id, member_id, title_in_team? }`, `team.remove_member { membership_id }`, `team.set_title { membership_id, title }`
- `project.add_team { project_team_id, project_id, team_id }`, `project.remove_team { project_team_id }`
- **Members:**
  - agents join through hiring (the HR flow, its own design)
  - people are added with `member.add_person { member_id, name, contact? }` and edited with `member.update_person { member_id, name?, contact? }`
  - **`member.remove { member_id }`** removes anyone except the owner. Offboarding an agent issues it, with the same cascade.

Each command carries a client-made id for anything it creates, and the version of what it edits, as in the offices design:
- `team.move` expects the moved team's version. The new owner and the loop check are judged against HQ's current state.
- `team.rename`, `team.set_lead`, `team.set_labels` and `team.delete` expect the team's version.
- Membership commands name the membership by its **id** and expect its version (`set_title`, `remove_member`), so a stale queued command never hits a membership that was removed and re-added (that one has a new id). `add_member` needs only that the team and member exist. A membership is its own entity, so adding or removing members doesn't change the team's version (clearing a departing lead does).
- Project-team links are their own entities too, with ids: `add_team` needs the project and team to exist, and `remove_team` names the link by id and expects its version.
- `member.remove` and `member.update_person` expect the member's version.

A command queued in a satellite's outbox can find its team, owner or member deleted, or its version stale. It's then rejected with `not_found` or `conflict`, and shown as blocked (offices design).

**Reading:** the app reads from its local office's copy with **`org.snapshot`**, the one read call. It returns the teams, members (with the owner marked), memberships, projects and project-team links, together with the **position** it reflects. `subscribe { from_position }` then resumes from exactly that point, so no change slips in between.

## The chart
One org chart over the same data, with three ways to look at it:
1. **Teams:** the ownership tree of all teams.
   - Each team is a card showing its name, labels, lead, members and projects.
   - **Agents and people look different** on the card: separate rows, different styles.
   - A member in several teams appears in each, with a "+2 teams" badge listing the others.
   - A shared project shows on each of its teams' cards, with a "shared" badge.
   - **The owner isn't on any card.** It appears separately, at the top, as the organization's owner.
   - Collapse and expand any branch. Drag a team onto another to move it, with a confirmation, and HQ checks for loops.
   - A filter by office highlights agents placed in a given office.
   - A filter by label, e.g. `division`.
2. **Guilds:** a saved label filter for `guild`, because you asked for a guild org chart. It shows the teams labelled `guild` side by side, each listing its members, and each member showing the teams they're in.
3. **People** (the human org chart): the same team tree **showing only people**. It's where you add a person, and it shows plainly that **agents can only reach people listed in the organization**.

Editing (create, add member, move, label, add a person, link a project) is available from all three. Changes waiting in an offline satellite's outbox show as **pending**, and rejected ones as **blocked**, as in the offices design.

## In org-proto
- Types: `Team` (with `labels`), `Member` (with `kind` and an `owner` flag), `Membership`, `ProjectTeam`.
- Calls: the commands above, `org.snapshot` (the read, with its position) and `subscribe { from_position }`.
- Events: one per command.
- Error codes: `cycle`, `not_empty`, `not_a_member` (a lead who isn't in the team), `owner_protected` (removing the owner, adding them to a team, or making them a lead), `not_found` and `conflict` (both also used by the offices design), and `unknown_recipient` (messaging and tasks, ADR-0003), plus the existing ones.
- `project.create` gains `team_ids?`, which amends `projects-git-github.md`'s contract (step T1).
- Messaging (ADR-0003) and tasks check recipients against members: agents, and people who've been added.

## Plan
| Step | What | Who |
| --- | --- | --- |
| T0 | UX for the chart's three views and the editing flows | Eden (product designer), with app-frontend-dev. Her spec needs updating for the owner, people and shared projects |
| T1 | The contract in `org-proto`, including `team_ids?` on `project.create` (amends the projects design) | ada |
| T2 | Teams, members (the owner, people), memberships and project-team links as commands, events and apply code in `org-office`, with the rules above and tests (loops, non-empty delete, multi-team membership, the owner can't be removed or lead) | server-dev, after the HQ event log |
| T3 | The app: the chart's team, guild and people views, and editing. Built on a mock with the T1 types first, then on the office | app-frontend-dev |

Each step: its own branch, an independent review, my go, then merge into `v1`.

## Decisions (the user, 2026-11-08)
- Teams own teams. **"Division" and "guild" are only labels** on a team.
- Members (agents and people) can be in several teams. **Only agents are placed in offices.**
- **Leads are optional,** and can be an agent or a person. A team's lead also leads the team's projects.
- **The owner is the organization's special user:** in no team, full rights everywhere, reachable and assignable by any agent, and shown separately.
- **People are tracked and shown separately from agents.** Agents can only interact with people who've been added to the organization.
- **The human org chart is the team chart showing only people.**
- **Projects can be shared by several teams** (usually one), through their own relationship.
