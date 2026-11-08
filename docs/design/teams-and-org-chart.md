# Design: teams and the org chart

Status: **draft for the user**, 2026-11-08. Owner: ada. It follows the user's model (2026-11-08). It builds on `offices-hq-and-satellites.md`: teams are organization data, so HQ owns them and every office has a copy.

## The user's model, in short
- An organization is made of **teams**. **A team can be owned by another team**, so teams form a tree.
- **A division is just a team** that owns other teams. There's no separate kind of thing.
- **A guild is a team too** ("guilds are nothing more than teams"): a team that groups members across other teams by discipline, e.g. "Frontend". There's one kind of thing, the team. *(ada's proposal: a `guild` label on the team, so the guild chart knows which teams to show; open question 4.)*
- **Members are agents or people (humans), and each can belong to several teams.** People are tracked and shown differently from agents.
- **Agents can only interact with people who have been added** to the organization. Adding a person is what lets agents message them, ask them and give them tasks.
- **The owner (you) is special:** a super-user, implicitly part of each agent team, **not shown as a member in the team chart**, but shown separately. You **can** also be part of the human org chart.
- **A project is usually worked on by one team, but several teams can share it.** That's its own relationship, separate from team ownership.
- Example: create a "Product" division team and add members to it. Under it, create a "Checkout" project team, and give it the Checkout project.

## The model
- **Team** `{ id, name, kind, owner_team_id?, lead_member_id?, description?, version }`
  - `kind` is `team` or `guild`, a **label only**: the same rules apply to both. A guild can own teams or be owned by a team, like any team, and relabelling is always allowed.
  - `owner_team_id` empty means a top-level team.
  - **Ownership forms a tree:** each team has at most one owner, and a team can't own itself or any team above it. HQ rejects a move that would create a loop (`cycle`).
  - **The lead** is optional, and must be a member of that team (an agent or a person, never the owner, who's in every team already).
- **Member** `{ id, kind: agent | person, name, contact?, version }`
  - **An agent** is an AI employee. Its record also says which office it's placed in.
  - **A person** is a human added to the organization. `contact` is how they're reached, e.g. an email; that's for later.
- **The owner** is a person record with `owner: true`, created with the organization.
  - It's **implicitly a member of every team**, with full rights everywhere, so it's never stored as a membership and never counted in a team's members. *(Whether that's every team or only teams with agents in them: open question 5.)*
  - It **can't be removed**, added to a team, or made a team's lead (it's already above every team). Any of those returns `owner_protected`.
- **Membership** `{ id, team_id, member_id, title_in_team?, version }`: an agent or a person can be in **any number** of teams. `title_in_team` is a free-text label, e.g. "frontend lead". It's named that way so it isn't confused with the organization's role catalogue.
- **Project team** `{ id, project_id, team_id, version }`: the teams working on a project. Usually one, sometimes several. A project with none is **unassigned**, e.g. one created before any teams exist; the charts show those in their own group, and the app prompts to assign them.
- **Who agents may interact with:**
  - An agent may message, `ask` or assign a task to **another agent** of the organization, or to a **person who's a member** of the organization. The owner is always a member.
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
- **names:** a team's name is unique among teams with the same owner, whatever their kind (top-level teams count as siblings of each other). A create, rename or move that would clash returns `already_exists`.
- adding someone who's already in the team, or linking a team that already works on the project, returns `already_exists`.

## How it changes
These are organization commands, so they go through HQ and every office gets the resulting events (offices design):
- `team.create { team_id, name, kind, owner_team_id? }`, `team.rename`, `team.move { team_id, owner_team_id? }`, `team.set_lead`, `team.set_kind`, `team.delete`
- `team.add_member { membership_id, team_id, member_id, title_in_team? }`, `team.remove_member { membership_id }`, `team.set_title { membership_id, title }`
- `project.add_team { project_team_id, project_id, team_id }`, `project.remove_team { project_team_id }`
- **Members:**
  - agents join through hiring (the HR flow, its own design)
  - people are added with `member.add_person { member_id, name, contact? }` and edited with `member.update_person { member_id, name?, contact? }`
  - **`member.remove { member_id }`** removes anyone except the owner. Offboarding an agent issues it, with the same cascade.

Each command carries a client-made id for anything it creates, and the version of what it edits, as in the offices design:
- `team.move` expects the moved team's version. The new owner and the loop check are judged against HQ's current state.
- `team.rename`, `team.set_lead`, `team.set_kind` and `team.delete` expect the team's version.
- Membership commands name the membership by its **id** and expect its version (`set_title`, `remove_member`), so a stale queued command never hits a membership that was removed and re-added (that one has a new id). `add_member` needs only that the team and member exist. A membership is its own entity, so adding or removing members doesn't change the team's version (clearing a departing lead does).
- Project-team links are their own entities too, with ids: `add_team` needs the project and team to exist, and `remove_team` names the link by id and expects its version.
- `member.remove` and `member.update_person` expect the member's version.

A command queued in a satellite's outbox can find its team, owner or member deleted, or its version stale. It's then rejected with `not_found` or `conflict`, and shown as blocked (offices design).

**Reading:** the app reads from its local office's copy with **`org.snapshot`**, the one read call. It returns the teams, members (with the owner marked), memberships, projects and project-team links, together with the **position** it reflects. `subscribe { from_position }` then resumes from exactly that point, so no change slips in between.

## The charts
1. **Team chart:** the ownership tree of **all** teams. Guild-labelled teams appear in it like any team, with a small "guild" badge.
   - Each team is a card showing its name, lead, members and projects.
   - **Agents and people look different** on the card: separate rows, different styles.
   - A member in several teams appears in each, with a "+2 teams" badge listing the others.
   - **The owner isn't shown on any card.** The owner appears separately, at the top of the chart, as the organization's owner with access to every team.
   - A shared project shows on each of its teams' cards, with a "shared" badge.
   - Collapse and expand any branch. Drag a team onto another to move it, with a confirmation, and HQ checks for loops.
   - A filter by office highlights agents placed in a given office.
2. **Guild chart:** the guild-labelled teams side by side, each listing its members (agents and people, styled apart). Each member shows the teams they're in.
3. **People chart** (the human org chart): the **people** of the organization, each with the teams they're in, and the owner if you choose (open question 5). It's where you add a person, and it shows plainly that **agents can only reach people listed here**.

All three are views of the same data. Editing (create, add member, move, relabel, add a person, link a project) is available from them. Changes waiting in an offline satellite's outbox show as **pending**, and rejected ones as **blocked**, as in the offices design.

## In org-proto
- Types: `Team` (with `kind`), `Member` (with `kind` and an `owner` flag), `Membership`, `ProjectTeam`.
- Calls: the commands above, `org.snapshot` (the read, with its position) and `subscribe { from_position }`.
- Events: one per command.
- Error codes: `cycle`, `not_empty`, `not_a_member` (a lead who isn't in the team), `owner_protected` (removing the owner, adding them to a team, or making them a lead), `not_found` and `conflict` (both also used by the offices design), and `unknown_recipient` (messaging and tasks, ADR-0003), plus the existing ones.
- `project.create` gains `team_ids?`, which amends `projects-git-github.md`'s contract (step T1).
- Messaging (ADR-0003) and tasks check recipients against members: agents, and people who've been added.

## Plan
| Step | What | Who |
| --- | --- | --- |
| T0 | UX for the three charts and the editing flows | Eden (product designer), with app-frontend-dev. Her spec needs updating for the owner, people and shared projects |
| T1 | The contract in `org-proto`, including `team_ids?` on `project.create` (amends the projects design) | ada |
| T2 | Teams, members (the owner, people), memberships and project-team links as commands, events and apply code in `org-office`, with the rules above and tests (loops, non-empty delete, multi-team membership, the owner can't be removed or lead) | server-dev, after the HQ event log |
| T3 | The app: the team, guild and people charts, and editing. Built on a mock with the T1 types first, then on the office | app-frontend-dev |

Each step: its own branch, an independent review, my go, then merge into `v1`.

## Decisions (the user, 2026-11-08)
- Teams own teams, a division is a team, and members (agents and people) can be in several teams.
- **A guild is just a team**, so the model has one team entity.
- **The owner is a super-user:** implicitly part of each agent team, not shown as a team member, shown separately, and can be part of the human org chart.
- **People are tracked and shown separately from agents.** Agents can only interact with people who've been added to the organization.
- **Projects can be shared by several teams** (usually one), through their own relationship.

## Open questions for the user
1. **Team leads:** one optional lead per team (an agent or a person), as proposed? Or none for now, or several?
2. **Do a division's members limit its sub-teams?** Your flow is "add people to the division, then create a project team under it". Should a sub-team only take members who are already in its owner team (the division as a pool), or can anyone in the organization join any team? And should a division's member count include its sub-teams' members?
3. **People and offices:** a person isn't placed in an office the way an agent is. They reach agents through the app, from any office. Is that right?
4. **Guild label:** to draw the guild chart, the app has to know which teams are guilds. ada proposes a simple `guild` label on a team (same rules otherwise). Is that what you want, or should guilds be told apart some other way?
5. **The owner and the human side:** you're part of each **agent** team. Should you count as part of teams that have only people in them too? And does the human org chart have its own structure (human teams), with your place in it optional, or is it the same teams, shown for people?
