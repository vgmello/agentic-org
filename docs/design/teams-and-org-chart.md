# Design: teams and the org chart

Status: **draft for the user**, 2026-11-08. Owner: ada. It follows the user's model (2026-11-08). It builds on `offices-hq-and-satellites.md`: teams are organization data, so HQ owns them and every office has a copy.

## The user's model, in short
- An organization is made of **teams**. **A team can be owned by another team**, so teams form a tree.
- **A division is just a team** that owns other teams. There's no separate kind of thing.
- **A member is an agent or a person, and can belong to several teams.**
- **A project belongs to one team.**
- Example: create a "Product" division team and add people to it. Under it, create a "Checkout" project team for one project, and give the project to that team.
- **A guild is a team too** *(the user, 2026-11-08: "guilds are nothing more than teams")*: a team that groups members across other teams by discipline, e.g. "Frontend". The entity stays simple: there's one kind of thing, the team.

## The model
- **Team** `{ id, name, kind, owner_team_id?, lead_member_id?, description?, version }`
  - `kind` is `team` or `guild`, a **label only**: the same rules apply to both. A guild can own teams and projects, or be owned by a team, like any team; the charts show those cases (see "The two charts"). Relabelling is always allowed.
  - `owner_team_id` empty means a top-level team.
  - **Ownership forms a tree:** each team has at most one owner, and a team can't own itself or any team above it. HQ rejects a move that would create a loop (`cycle`).
  - **The lead** is optional, and must be a member of that team.
- **Member** `{ id, kind: agent | person, name, version }`. An agent is an AI employee (its record also says which office it's placed in). A person is a human, starting with you, the owner.
- **Membership** `{ team_id, member_id, title_in_team?, version }`: a member can be in **any number** of teams. `title_in_team` is a free-text label, e.g. "frontend lead". It's named that way so it isn't confused with the organization's role catalogue.
- **A project** belongs to one team: `team_id`. It's empty while **unassigned**, e.g. projects created before any teams exist, or created without choosing one. The charts show unassigned projects in their own group, and the app prompts to assign them.
- **Rules HQ enforces:**
  - no ownership loops
  - a lead must be a member. **Removing the lead from the team clears the lead**, in the same event, and the team's version goes up.
  - **deleting a team** that owns teams or projects is refused (`not_empty`) until they're moved or deleted. Its memberships go with it.
  - **removing a member from the organization** removes their memberships and clears them as lead of any team, in the same event. Each affected team's version goes up. **The owner's own person record can't be removed.**
  - **names:** a team's name is unique among teams with the same owner, **whatever their kind** (top-level teams count as siblings of each other). So a top-level "Frontend" team and "Frontend" guild clash. A **create, rename or move** that would clash returns `already_exists`.
  - **adding someone who's already a member** returns `already_exists`.

## How it changes
These are organization commands, so they go through HQ and every office gets the resulting events (offices design):
- `team.create { team_id, name, owner_team_id? }`, `team.rename`, `team.move { team_id, owner_team_id? }`, `team.set_lead`, `team.delete`
- `team.add_member { team_id, member_id, title_in_team? }`, `team.remove_member`, `team.set_title`
- **Members:** agents join the organization through hiring (the HR flow, its own design). People: `member.add_person { member_id, name }`. **`member.remove { member_id }`** removes anyone; offboarding an agent issues it, with the same cascade. The owner's own person record is created with the organization.
- `team.set_kind { team_id, kind }` to relabel a team as a guild or back
- `project.move { project_id, team_id }`

Each command carries a client-made id for anything it creates, and the version of what it edits, as in the offices design:
- `team.move` expects the **moved team's** version. The new owner is checked against HQ's current state, as is the loop check.
- Membership commands expect the **membership's** version (`set_title`, `remove_member`). `add_member` needs only that the team and member exist. A membership is its own entity, so adding or removing members doesn't change the team's version (clearing a departing lead does, as above).
- `project.move` changes only the project's version.
- `team.rename`, `team.set_lead`, `team.set_kind` and `team.delete` expect the team's version. `member.remove` expects the member's.
- `project.move` expects the project's version.

A command queued in a satellite's outbox can find its team, owner or member deleted, or its version stale. It's then rejected with `not_found` or `conflict`, and shown as blocked (offices design).

**Reading:** the app reads from its local office's copy with **`org.snapshot`**, the one read call. It returns the teams, members, memberships and projects (with their `team_id`), together with the **position** it reflects. `subscribe { from_position }` then resumes from exactly that point, so no change slips in between.

## The two charts
1. **Team chart:** the ownership tree of **all** teams. Guild-labelled teams appear in it like any team, with a small "guild" badge, so nothing is ever left without its parent.
   - Each team is a card: its name, lead, member avatars and projects. A division is simply a team card with teams below it.
   - A member who's in several teams appears in each, with a small "+2 teams" badge that lists the others.
   - Collapse and expand any branch. Drag a team onto another to move it, with a confirmation. HQ checks for loops.
   - A filter by office highlights agents placed in a given office.
2. **Guild chart:** the teams labelled `guild`, side by side, each listing its members, and each member showing the teams they're in. That shows a discipline across the whole organization. A guild that also owns teams or projects shows them as a count, linking to its place in the team chart.

Both charts are views of the same teams. Editing (create, add member, move, relabel) is available from both. Changes waiting in an offline satellite's outbox show as **pending** in the chart (e.g. a moved team shown in its new place, marked pending), and rejected ones as **blocked**, as in the offices design.

## In org-proto
- Types: `Team` (with `kind`), `Member`, `Membership`, plus `Project.team_id`.
- Calls: the commands above, `org.snapshot` (the read, with its position) and `subscribe { from_position }`.
- Events: one per command.
- Error codes: `cycle`, `not_empty`, `not_a_member`, `not_found`, `conflict` (the last two also used by the offices design), plus the existing ones.
- `project.create` gains `team_id?`, which amends `projects-git-github.md`'s contract (step T1).

## Plan
| Step | What | Who |
| --- | --- | --- |
| T0 | UX for both charts and the team editing flows | Eden (product designer), with app-frontend-dev |
| T1 | The contract in `org-proto`, including `team_id?` on `project.create` (amends the projects design) | ada |
| T2 | Teams, members and project ownership as commands, events and apply code in `org-office`, with the rules above and tests (loops, non-empty delete, multi-team membership, the owner can't be removed) | server-dev, after the HQ event log |
| T3 | The app: team chart, guild chart and editing. Built on a mock with the T1 types first, then on the office | app-frontend-dev |

Each step: its own branch, an independent review, my go, then merge into `v1`.

## Decisions (the user, 2026-11-08)
- Teams own teams, a division is a team, and a member can be in several teams.
- **A guild is just a team** (labelled `guild`), so the model has one entity.

## Open questions for the user
1. **Team leads:** one optional lead per team, as proposed? Or none for now, or several?
2. **Where your own person sits:** are you a member of every team by default, or only those you add yourself to?
3. **Projects:** exactly one team per project, as proposed? Or can a project be shared by several teams?
4. **Do a division's members limit its sub-teams?** Your flow is "add people to the division, then create a project team under it". Should a sub-team only take members who are already in its owner team (the division as a pool), or can anyone in the organization join any team? And should a division's member count include its sub-teams' members?
