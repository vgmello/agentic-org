---
kind: spec
title: "Offices, organizations and teams (UX)"
status: for-review
revision: 16
owner: ac-product-designer-eden
created: "2026-11-08"
updated: "2026-11-10"
requested_by: ac-staff-engineer-ada
reviewers: [ac-staff-engineer-ada, owner]
builds_on: []
---

# Spec · Offices, organizations and teams (UX)

**Revision 12 (2026-11-09): standalone server pivot.**
[server-installation-and-access-ux.md](server-installation-and-access-ux.md) supplies
the v1 installation/access requirements. The browser's machine is not an office.
MENU-1 through MENU-6 are retired from v1 (native shell deferred to v2), OPEN/JOIN
and OFC criteria below are revised for HQ HTTP access and CLI enrollment. Existing
team, people, reporting and owner semantics remain. Old changelog entries record
history and do not reintroduce the desktop flows.

Requested by ada for the user.

**Sources:** ada's `docs/design/offices-hq-and-satellites.md` (approved), `docs/design/teams-and-org-chart.md` (**approved by the user 2026-11-08**) and `docs/design/create-organization.md`. Look and feel: `prototype-agency` and Tailwind Plus application UI (dark sidebar, gray-900 ground, indigo-500 accent), as the app's onboarding screens already use.

**What it changes in my earlier work:** it replaces, for the office model, `desktop-app-v1.md` (in Eden's notes, not in this repository) §2.5 (workspace switcher), §3.2 (department and reporting-line chart) and §7.1 (Machines). Revision 5 of that spec folds this in. Acceptance-criterion IDs here are stable: changed criteria are rewritten under the same ID, retired IDs are never reused.

## Changelog

**Revision 16 (2026-11-10): repair instructions, restart states, the countdown
and the details sheet.** From the Principal Product Designer's decision of
2026-11-10 on narrow widths and its follow-ups:
- **`unsupported_version`:** the repair instructions' toggle, copy and Copy
  command, naming the office's host; "the office's machine" for the legacy host
  "This Mac"; no invented instance name, and never "this Mac" for the office's
  machine. No restart button in this state (§3.3).
- **The office row's screen-reader label** says "(you're here)" only when the
  host says so (§6.1).
- **Restart busy and refusal:** the restart button stays focusable while busy
  and the busy state is announced; a refusal shows its reason in an alert with
  focus kept on the button, and clears when the office's status or reason
  changes; a refusal as `unavailable` shows the reason row, not the host's raw
  message (§3.3).
- **The countdown** never overstates the time left (§3.3).
- **The details sheet** has one Close control, and closing it returns focus to
  whatever opened it (§6.1).
- §6.1's footer copy, "Shown from HQ. Statuses include last-seen information.",
  was already there; it now has a criterion.

**Criteria rewritten (same ID):** none. **New:** OFC-39 to OFC-48.

**Revision 15 (2026-11-10): offices on this instance, joins that end, and waiting
for running agents.** From Rowan's PR #5 design fixes (merged at 1059d88) and his
X-02 (`org-proto` v2, approved by ada):
- **Add office › On this instance needs the instance admin**, because it
  provisions a key and a worker on HQ's instance. Inviting, approving and removing
  a satellite stay open to any authorized owner. For an owner without the grant
  the choice isn't offered, as for Create organization (§5b, §6.1, §6.2).
- **New §5c, joining from setup:** what setup shows while a join waits (its
  state, check code and last error); which errors end a join (invitation expired,
  cancelled after 3 wrong check codes, declined) and which keep it waiting (HQ
  can't be reached), with copy and repair; and `agentic-org office remove-pending`.
- **§3.3:** a satellite whose join ended gets its own row with **Remove…**, never
  Restart office, which the host refuses for it. §3.1: a pending join shows
  Waiting for approval, then, once HQ approves it, Starting · copying records
  until its first copy of records completes (§6.3); on the joining instance,
  the status read's `join_phase` tells the two apart (A102, resolved).
- **Two views of a joining office:** Waiting for approval and the "join ended"
  row show only in the joining instance's own view (its status and, if it has
  one, its local status page), never in HQ's Offices list, which keeps §6.3's
  rows (Wants to join, Invitation cancelled, and the invitation's row after a
  decline or expiry) (§3.1, §3.3, §5c; OFC-26, OFC-32).
- **Waiting for running agents:** Create organization and Add office › On this
  instance wait, with Cancel, while agents are running on that instance, until the
  host can ask offices to relaunch their sessions (X-02 Q9) (§5a, §6.2).
- §5b: showing a recovery code follows `server-installation-and-access-ux.md`
  rev 5 (`agentic-org recovery release`, instance admin). Glossary: Instance admin
  and Recovery code.
- §5b: adding an account and changing or resetting a password follow
  `server-installation-and-access-ux.md` rev 5 (`agentic-org account create`,
  `set-password`, `reset-password`), and the admin grant alone opens no
  organization (its A86, resolved).
Also new: JOIN-17 (remove-pending refusals, including a non-admin) and JOIN-18 (noninteractive setup ends with waiting_for_idle); §5b lists every HQ-machine command that needs the instance-admin grant.
Also new: JOIN-20 and §5c "Signing in first" (a join on an instance with accounts needs an instance admin).

- *(Review of 07a8a4b)* **Waiting for running agents** ends only when no agent
  session on that instance is running: each agent Asleep, Paused or stopped. An
  agent that's Idle between turns still counts (`agent-sessions.md`, ADR-0003).
  The copy reads "Waiting for 3 agents to stop. Their current sessions must end
  first." No agent is stopped or relaunched for it (§5a, §6.2; OFC-28, OFC-29).
- *(Rowan's answers, 2026-11-10; X-02 Q9)* A101: the wait's number comes from
  `waiting_for_idle { sessions }`; Cancel cancels the same operation; a waiting
  organization shows in the switcher, and a waiting office as a pending row in
  the Offices list; a satellite join waits the same way in setup, where Ctrl-C
  cancels; there's no time limit, and `agentic-org doctor` shows the wait (§5a,
  §5c, §6.2). A102: `join_phase` tells Waiting for approval from Starting ·
  copying records (§3.1, §5c); `join_cancelled` covers a cancelled or replaced
  invitation; an HQ identity mismatch ends as `join_rejected` and isn't retried;
  an unsupported version keeps the join waiting until the invitation expires
  (§3.3, §5c). New A105: telling a decline from a mismatch, both
  `join_rejected`. §5b lists `agentic-org account link`, `unlink` and `revoke`
  (`server-installation-and-access-ux.md` rev 5, its A103).
- *(Review of 02316bb)* The wait for running agents names the agents it waits
  for, each with a link to its profile and **Pause** (DEAL-3), and says
  Always-on agents keep running until paused, since their sessions restart.
  Pausing them ends the wait once their sessions stop; nothing is stopped
  automatically. Setup names them and says to pause them in the app, and its
  noninteractive message prints the cancel command with the office ID.
  `office remove-pending` on a join waiting for running agents asks "Cancel
  joining…? The join hasn't reached HQ yet." §5b no longer lists
  `office remove-pending` among the HQ-machine commands (§5a, §5b, §5c, §6.2;
  OFC-28, OFC-29, OFC-37, JOIN-11, JOIN-18, JOIN-19). New A106.
- *(Review of 0802754)* The CLI follows the browser's rule for agents of
  organizations the reader doesn't own: counted, not named. Setup with an
  instance admin signed in names the agents of organizations that account owns,
  each with "Pause it in the app", and shows "<n> agents in <organization> · ask
  its owner to pause them" for each other organization. Without a session
  (`agentic-org status`, `doctor`, setup on an instance with no accounts, and
  noninteractive setup), it shows counts only, one for each organization:
  "Waiting for 3 agents to stop (2 in Lumen Labs, 1 in Side Project)". A106 now
  asks a read without a session for those counts, not names (§5a, §5c, §6.2;
  JOIN-18, OFC-38).

**Criteria rewritten (same ID):** OFC-7, JOIN-7 *(rev 15)*, then OFC-28, OFC-29,
OFC-32 and JOIN-7 again, then OFC-28, OFC-29, OFC-37, JOIN-11 and JOIN-18 again,
then JOIN-15, JOIN-18 and OFC-38 again.
**New:** OFC-26 to OFC-38, JOIN-9 to JOIN-20. Open questions A101 and A102,
both since resolved; A105 and A106.

**Revision 14 (2026-11-10): Create organization and the instance admin.** New
§5a, **Create organization**, for instance admins only, from the organization
switcher (where the old "+ Create organization" entry was): the name and its
validation, Creating…, Starting, ready, failed with Try again, and a lost response
resolved by looking up the same operation (`create-organization.md`). New §5b,
**Who can do what**, from the owner's decision O-1. Decision 17. Glossary: Account,
Instance admin and Recovery code. §10 points to the recovery-code screens in
`server-installation-and-access-ux.md` §4a. Open questions A91, A92 and A95.
**Criteria added:** OFC-14 to OFC-25. No existing criterion changed.

**Revision 11 (2026-11-09): the menu has New request.** The menu-bar menu gains **New request… ⌥⌘N** and, while captures wait for the office, "n requests waiting to send" (`feature-requests-ux.md` §1.1). No criteria changed here; see RQS-1, RQS-17.

**Revision 10 (2026-11-08): key custody wording (ada).** With key custody (the host holds the office keys, River's `river/host-keys`, not yet on `v1`; until it merges, ADR-0002 and `office-host-boundary.md` still describe the Keychain model, and ada updates them), `key_missing` covers both a key that isn't there and a key that doesn't open the databases. The new copy is right under both models. Its copy no longer names the Keychain: "This office's key isn't available on <office machine / instance>, so its data can't be opened" (§3.3). The action is unchanged. No criteria changed.

**Revision 9 (2026-11-08): host capabilities (A50).** The host's `hello` lists its capabilities, and that list replaces version comparison for status events and restart. Without `office.status_changed` the app shows "Status unknown" (as in rev 8). Without `office.restart` the restart button is hidden and "Update Agentic Org to restart offices from here" shows, besides the refusals for `unsupported_version` and `invalid_request` (§3.3). `HelloResult.capabilities` is on v1 (`crates/org-proto/src/calls.rs`, with `capability::OFFICE_RESTART`, `OFFICE_STATUS_CHANGED` and `HelloResult::supports`; ada, merged at `b9f1198`). **Criteria rewritten (same ID):** OFC-11, OFC-12 *(rev 9)*.

**Revision 8 (2026-11-08): office status follows the contract (team reviewer).** §3.3 now follows ada's branch ada/proto-office-status (`2f78e40`) and `office-host-boundary.md` at `504f112` (branch ada/docs-followups-3), neither on v1 yet.

- §3.1 maps each wire status to one label: `offline` and `unavailable` on this Mac show as Stopped with §3.3's reason; Offline is only for another computer's office.
- `in_use`: new copy ("still in use by another Agentic Org process"), and Restart office is offered, as the contract accepts it.
- `unsupported_version`: its own copy ("Parts of Agentic Org on this Mac don't match…") with Check for updates · Copy details, no longer update_needed's words.
- `office.restart`'s answers are one list (§3.3): `conflict` keeps the spinner, `unknown_method` hides the button, `not_found` refreshes the list, `not_authorized` is an error, `not_ready` retries once (the app's request timeout must be over 30 s).
- "Restarting in N s" shows whenever `next_restart_at` is present, whatever the reason. The crash countdown's button is now **Restart office** too, the one label for that call and state.
- A missing `status_reason` (the host doesn't know why) has its own row, "It isn't running", kept apart from `unknown`.
- §3.2 says which reasons each row covers: "Can't open its data" for `key_missing` and `storage_error`, "Its office on this Mac stopped" for every other.
- Accessibility (§6.1): the row label doesn't tick ("restarting shortly"), the visible countdown updates at most every 10 s, and status changes of this Mac's offices are announced politely.
- `storage_error` no longer offers to show its data folder (the folder layout isn't part of the contract).
- A host whose `hello` doesn't list the capability `office.status_changed` (A50) counts as not live: its offices show "Status unknown". A current host that sends no event simply has nothing new.
- **Criteria rewritten (same ID):** OFC-11, OFC-12, OFC-13.

**Revision 7 (2026-11-08): review fixes.** §3.3: `office.restart` answers `conflict` only when it's pressed while the office is already starting or running. During a crash countdown the office is offline, so the restart button is accepted (the old example said otherwise). The list of refusals is now complete: `not_authorized` ("You can't restart this office from here.") and `not_found` ("This office no longer exists.") show as errors in plain words with Copy details. No criteria changed.

**Revision 6 (2026-11-08): why an office stopped, in plain words.** ada's `OfficeInfo.status_reason`, `next_restart_at`, the live `office.status_changed` event and `office.restart` (branch ada/proto-office-status, with the reasons `unsupported_version`, `invalid_request`, `unknown` and the error `not_ready`) get their copy and actions in §3.3. From the Offices review: status is shown only when live, each status has its own icon, and the build's good choices are now part of the spec (an Unknown row in §3.1, and Sample data and the disabled Add office in §6.1). `key_missing` and `storage_error` copy follows `office-host-boundary.md`. The remove dialog's no-manager variant is written outside the dialog text (§7.4). OFC-11 to OFC-13 added.

**Revision 5 (2026-11-08): reporting lines follow org-proto on v1.** Removing a member sends their reports to that member's own manager, or to the owner if they had none (§7.4, §7.8, PEOPLE-5).

**Revision 4 (2026-11-08): one handle namespace (org-proto on v1, `86a6a9f`).** A person's name is also their handle, unique across every member ignoring case (agents' messaging names, people's names and the reserved `owner`), so two people can't share a name and a person can't take an agent's handle. §7.4's "Add anyway" is replaced by a refusal with a suggestion. PEOPLE-4 rewritten, PEOPLE-8 added.

**Revision 3 (2026-11-08): reviewer fixes.** A removed person's tasks come back **unassigned** for the owner to reassign (teams design). "Open organization" stays on the first-launch screen (ada, A38). "Join organization" is used only for joining with an invitation link (phase 3).

**Revision 2 (2026-11-08): the user's final team model, and ada's answers.**

- **Only teams.** The Guilds tab, the guild chart and every label are gone. A "division" is just a team that owns teams. **GUILD-1 and GUILD-2 are retired** (IDs not reused).
- **Members are agents or people**, styled differently (§7.0). People are never placed in offices. Agents can only reach people who've been added.
- **The owner** is the organization's special user: in no team, full rights, reachable by any agent, shown once above the chart (§7.1), never on a card.
- **Lead** is optional, an agent or a person, and also leads the team's projects.
- **Shared projects:** a project can have several teams (usually one), with a **Shared** badge. A project with no team is **unassigned**. §7.6 rewritten (it used to move a project between teams).
- **Three views of one chart:** **Teams**, **People** (where you add a person, §7.3 and §7.4) and **Reporting lines** (where you set someone's manager, §7.8). The nav item is **Org chart** again (decision 10).
- **Product name: Agentic Org.** All copy says "Agentic Org" where it said "agentic.org". The sample organization in this spec is renamed **Lumen Labs** so copy like "Join Lumen Labs?" can't be confused with the app.
- **Resolved:** U1 (keep "HQ" and "office" for now), U2 (reporting lines stay, as their own view), U3 (no departments, no labels), U4 (one optional lead; the owner in no team; shared projects allowed), A1 (join warning copy), A2 (stable check code: the "code changed" state is removed), A3 (three wrong codes cancel the invitation), A4 (a removed office's agents become unplaced), A5 (Add office and approval only on HQ's Mac), A6 (8 characters, Crockford base32, two groups of four).
- **Criteria rewritten (same ID):** JOIN-3, OFC-6, OFC-7, OFC-8, TEAM-1, TEAM-3, TEAM-4, TEAM-5, TEAM-8, TEAM-9, TEAM-10.
- **Criteria added:** JOIN-8, OFC-10, TEAM-11 to TEAM-15, PEOPLE-1 to PEOPLE-7, REPORT-1 to REPORT-5, OWNER-1 to OWNER-3.

**Revision 1 (2026-11-08):** first draft. 13 decisions, 4 flows, 37 criteria.

## 0. Decisions in this spec

1. **HQ organization navigation.** The in-page switcher lists authorized organizations and their offices. The native menu is deferred to v2.
2. **Switching organization never asks for confirmation.** Offices run under the login service, so nothing stops when you switch. The only prompt is for unsaved input ("Discard the new team?").
3. **The UI says "HQ" and "office", never "satellite".** HQ gets a badge; every other office is just an office. (`satellite` stays the engineering term.) The user is OK with this for now and may revisit it.
4. **Locations identify the service machine or instance.** Support macOS and Linux; never label the browser machine as the executing office.
5. **Services run without a browser.** Pending joins and agents needing attention appear in HQ Needs you; native background notifications are v2.
6. **Enrollment uses target-machine setup:** validate invitation, confirm destination/execution consent, await HQ approval, synchronize and report readiness.
7. **The invitation is shown once.** HQ retains only its verifier/public material. The screen uses **Copy invitation** and **New invitation**; creating a replacement cancels the old invitation. Destination setup accepts the secret through protected input, not a desktop URL handler.
8. **The check code identifies the joining request/key.** The CLI displays it on the joining machine; the owner verifies it through a trusted session to that machine. Do not treat terminal copy prevention as a security guarantee.
9. **Check-code input is one text field**, not one box per character (screen readers, paste, password managers). Case, spaces and dashes are ignored.
10. **Org chart** is one nav item with three pill tabs: **Teams | People | Reporting lines**. Offices has its own nav item (bottom group, above Settings).
11. **There are only teams.** No guilds, no divisions, no labels. A team that owns teams simply shows "3 teams".
12. **Every edit has a non-drag path** (menu, dialog, keyboard). Drag-to-move and drag-to-change-manager are shortcuts, never the only way.
13. **Office outboxes report Pending and Blocked.** HQ shows their known outcomes. A disconnected browser marks its view stale and its own unsubmitted text unsaved; it does not become a satellite outbox.
14. **Agents and people never look alike**, and the difference is never color only (§7.0).
15. **The owner is shown once, above the chart**, never inside it, except as the root of Reporting lines, where everyone ultimately reports to them.
16. **Reporting lines and teams are independent.** Leading a team doesn't make you anyone's manager, and the UI says so where people would assume it.
17. **Accounts sign in; people don't** *(rev 14, O-1)*. An account belongs to the instance. Only an instance admin creates organizations, and whoever creates one owns it. People are records agents can reach, never sign-ins. Accounts are added and passwords reset only on the HQ machine (`server-installation-and-access-ux.md` §4a.1).

## 1. Words (proposed additions to the design-system glossary)

| Term | Means | Don't say |
| --- | --- | --- |
| Agentic Org | The app | agentic.org (in copy), Agency |
| Office | Where an organization's agents run and its data is kept. One organization has one or more. | Server, node, satellite, machine |
| HQ | The organization's first office. It keeps the records and approves new offices. | Primary, master, main server |
| This instance | The selected server instance, independent of the browser machine | This Mac, localhost |
| Invitation link | The one-time link that lets another computer join as an office | Token, enrollment, invite code |
| Check code | The short code a joining computer shows, typed at HQ to approve it | PIN, OTP, verification token |
| Team | A group of agents and people. A team can be part of another team. | Department, division, guild, squad |
| Member | An agent or a person in the organization | User, employee (in the UI) |
| Agent | An AI employee. It's placed in an office. | Bot, assistant, subagent |
| Person | A human added to the organization. Agents can reach them. | User, contact, human (in labels) |
| Owner | You: the organization's special user, with full access | Admin, root, super user |
| Account *(rev 14)* | What you sign in with: a username and a password on this HQ instance. It can own organizations. People and agents don't have one. | User, login, profile |
| Instance admin *(rev 14)* | An account that can create organizations on this HQ instance, and add offices on it *(rev 15)*. Setup makes the first account one. It isn't the same as owning an organization. | Admin (alone), superuser, root, owner |
| Recovery code *(rev 14)* | The code, collected on the HQ machine *(rev 15: shown once, and again only until it's confirmed as stored)*, that will let the owner recover an organization if HQ is lost. Never shown in the browser. | Backup code, recovery key, password |
| Lead | A team's optional lead, agent or person. Also leads the team's projects. | Manager, head |
| Reports to / Manager | Who someone reports to. Separate from teams. | Lead, boss |
| Shared | A project worked on by more than one team | Cross-team, joint |
| Unassigned | A project with no team, or an agent with no office ("Unplaced") | Orphan |

"Department" (from the role catalog) is not a grouping in the app, and there are no labels.

The owner and the instance admin are often the same account, but they are different words for different rights: the owner has full access to one organization; the instance admin can create organizations (§5b) *(rev 14)*.

## 2. Visual direction (Tailwind Plus application UI)

| Need | Component |
| --- | --- |
| Menu-bar dropdown (v2 only) | Deferred native macOS tray menu; v1 organization navigation and Needs you are in-page browser controls |
| In-app switcher | Sidebar footer button + Tailwind Plus *Dropdown with dividers* |
| Lists (organizations, offices, members) | *Stacked list* with a status *Badge with dot* (text always present) |
| Forms (Add office, New team, Add person) | *Slide-over* (right, `max-w-md`) |
| Confirmations and approval | *Modal dialog* (centered, with icon) |
| Warnings (join, remove) | *Alert* with yellow left accent border; red for security errors |
| Details of an office or member | *Description list* inside the slide-over |
| Pickers (owner team, members, project, manager) | *Combobox* with secondary text |
| Charts | Cards on `bg-gray-800/50`, `outline-white/10`, rounded-lg; connectors `white/15` (decorative) |
| View tabs | *Tabs in pills* (Teams, People, Reporting lines) |
| Owner bar | *Card with header*, `bg-indigo-500/10`, `outline-indigo-500/30` |

Status colors reuse the existing tokens: green = online, blue = starting, gray = offline/invited, amber = needs you / waiting, red = unreachable / stopped.

## 3. Status vocabulary

### 3.1 Office status (one at a time, from HQ, with reported location and freshness)

| Label | Icon (Lucide) | Color | When | Secondary text |
| --- | --- | --- | --- | --- |
| Online | `circle` filled | green | The office runs and is in sync with HQ (HQ itself: it runs) | — |
| Starting | `loader` | blue | Its owning instance reports the office starting | —; for a joining office HQ has approved, "copying records" until its first copy of records completes *(rev 15)* |
| Offline | `circle` outline | gray | The office is disconnected from HQ; its current execution state is not confirmed | "last seen 2 h ago" |
| Unreachable | `wifi-off` | red | HQ cannot be reached by the browser or the joining CLI | "last synced 10 min ago" |
| Stopped | `circle-x` | red | Its owning instance confirms the office is stopped/unavailable | the reason from §3.3, in plain words |
| Update needed | `circle-arrow-up` | amber | The office's server is too old for the organization's level | "needs Agentic Org 1.4 or later" |
| Invited | `mail` | gray | An invitation link exists and hasn't been used | "expires in 21 h" |
| Wants to join | `bell-ring` | amber (needs you) | A computer used the link and waits for its check code at HQ | "from 192.168.1.44 · 2 min ago" |
| Waiting for approval | `hourglass` | amber | The selected joining office awaits HQ approval. Only in the joining instance's own view, never in HQ's Offices list *(rev 15)* | "type the code at HQ" |
| Unknown | `circle-help` | gray | The app has no live status for it, or the status is `unknown` or missing (never shown as healthy) *(rev 6, rev 8)* | "Status unknown" |

**From the wire to the label** *(rev 8)*. `OfficeInfo.status` shows as one label:

- `active` → **Online**. `starting` → **Starting**. `update_needed` → **Update needed**. `pending` (invited, not joined yet) → **Invited**.
- **Two views of a joining office** *(rev 15)*. The joining instance's own view is its `agentic-org status` (and setup) and, if it has one, its local status page. HQ's Offices list is HQ's view. Each shows only its own states, as below (§5c).
- `starting` with `status_reason: join_pending` (a satellite whose join waits for approval or its first copy of records) *(rev 15)*:
  - In the joining instance's own view, from the status read's `join_phase` (A102, resolved: Rowan, 2026-11-10; X-02 Q9): `awaiting_approval` → **Waiting for approval**, never Starting or Online; `initial_sync` (HQ approved it, and its first copy of records runs) → **Starting** with "copying records" (shown as "Starting · copying records"), never Online.
  - In HQ's Offices list: never Waiting for approval. Before approval the request is a **Wants to join** row (§6.1, OFC-5). After approval → **Starting** with "copying records" (shown as "Starting · copying records", as in §6.3), never Online. This holds for an office on HQ's own instance too: a local satellite joins its own instance's HQ (`offices-hq-and-satellites.md`, "Provisioning a satellite", step 3, Checks), and HQ's list shows it as HQ sees it, whatever that instance's status reports.
  - The office becomes `active` (**Online**) only once that first copy completes (`offices-hq-and-satellites.md`, "Provisioning a satellite", steps 4 and 5; `office-host-boundary.md` §4).
- `unavailable` with a `join_*` reason → in the joining instance's own view only, **Stopped**, with §3.3's "join ended" row. HQ's Offices list never shows that row: it shows the invitation's own row from §6.3, with **New invitation** and **Remove**.
- `offline` or `unavailable`, for an office whose local instance reports it stopped → **Stopped**, with §3.3's reason as secondary text. The two look the same; the reason and its action say what differs.
- **Offline** means an office is not connected to HQ. Distinguish this from a locally confirmed Stopped state and from Unknown when evidence is unavailable.
- `unknown`, or no status at all → **Unknown** ("Status unknown").

### 3.2 Organization availability in the browser

The browser can open an organization when HQ can authenticate the owner and serve
that organization's data. A stopped satellite does not make HQ's synchronized
view unavailable. Its last-seen time and unavailable live controls remain visible.
If HQ cannot be reached, an open page says Live updates paused and marks cached data
stale. If HQ's storage/key is unavailable, show the specific reason and local
diagnostic action. The browser cannot open OS Login Items or start a stopped daemon.
Joining/approval/synchronization states belong to the target office and can be
inspected through setup/status or HQ when reachable.

### 3.3 Why an office stopped (rev 6)

Shown as the Stopped row's secondary text and at the top of its details, with the fix as a button. The office's status updates live (`office.status_changed`), and a status that isn't live is never shown: the row reads "Status unknown" instead. A host whose `hello` doesn't list the capability `office.status_changed` (A50) counts as not live, so its offices show "Status unknown"; a current host that sends no event simply has nothing new *(rev 9)*.

**Countdown** *(rev 8)*: whenever `next_restart_at` is present, whatever the reason, the copy ends with "Restarting in N s" (N from `next_restart_at`; for `in_use` the row's own "Trying again in N s"). The host sends it only while the office is `offline` with a restart pending. The visible countdown updates no more often than every 10 s; the screen-reader label doesn't tick (§6.1). *(rev 16)* The countdown never overstates the time left: N is rounded down, and it's correct from its first frame, including right after a status push.

| `status_reason` | Copy (secondary text) | Action |
| --- | --- | --- |
| `crashed` | "It stopped unexpectedly", then "Restarting in 20 s" when a restart is pending | **Restart office** · Copy details *(rev 8)* |
| `start_timeout` | "It didn't start in time" | **Restart office** · Copy details |
| `restarts_exhausted` | "It kept stopping, so it won't restart on its own" | **Restart office** · Copy details |
| `storage_error` | "It can't read or write its data on <office machine / instance>" (also used when the Keychain doesn't allow access, `office-host-boundary.md`) | **Restart office** · Copy details *(rev 8)* |
| `in_use` | "Its data is still in use by another Agentic Org process. Trying again in N s" (N from `next_restart_at`) *(rev 8)* | **Restart office** · Copy details (the host also retries by itself) *(rev 8)* |
| `key_missing` | "This office's key isn't available on <office machine / instance>, so its data can't be opened" *(rev 10)* (the databases exist, and no key that opens them is available to the owning instance: missing, or not the right one) | **Try again** (`office.restart`, which asks for the key again and returns straight to this reason if it still isn't available) · Copy details. The button says Try again, never Restart. |
| `stopped_by_owner` | "You stopped it" | **Start office** (`office.restart`). Phase 1 has no stop call, so this appears only once one exists. |
| `unsupported_version` | "The release components on <office machine / instance> don't match. Run diagnostics and repair the selected instance there." *(rev 8)* | **Show local CLI repair instructions** · Copy details. No office restart until the instance release is repaired. |
| `invalid_request` | "It couldn't start because of an internal error" | Copy details (to send to support). No Restart: the contract refuses it. |
| `join_expired`, `join_cancelled`, `join_rejected` *(rev 15)*: a satellite whose join ended. The host never restarts it, and `office.restart` answers `unavailable` for it. Shown only in the joining instance's own view, never in HQ's Offices list (§3.1, §5c) | `join_expired`: "Its invitation expired before the join was approved". `join_cancelled`: "Its invitation was cancelled or replaced at HQ (3 wrong check codes also cancel it)". `join_rejected`: "Its join was declined at HQ"; for an HQ identity mismatch, "HQ's identity didn't match the invitation, so it stopped joining"; when the status doesn't say which (A105), "Its join was refused: declined at HQ, or HQ's identity didn't match the invitation" | **Remove…** · Copy details. Never Restart office, Try again or Start office. |
| `unknown` (a reason this version doesn't know, from a newer host) | "It stopped for a reason this version doesn't know" | **Restart office** · Copy details |
| No `status_reason` (the host doesn't know why) *(rev 8)* | "It isn't running" | **Restart office** · Copy details |

- **Show local CLI repair instructions** *(rev 16)*, for `unsupported_version`:
  - The toggle reads **Show local CLI repair instructions**, and **Hide repair
    instructions** while they're shown. It has `aria-expanded` and
    `aria-controls` (pointing at the section), and focus stays on the toggle.
  - The section reads:

    ```
     On studio-mac, run:
       agentic-org doctor                                   [Copy command]
     It checks the Agentic Org instance on that machine and says what to repair.
     Follow its steps there. This page updates on its own when the office is back.
     If studio-mac runs more than one instance, add --name <instance>.
    ```

    "On <host>, run:", then `agentic-org doctor` in a code block with **Copy
    command**, which changes to **Copied**. Then "It checks the Agentic Org
    instance on that machine and says what to repair. Follow its steps there.
    This page updates on its own when the office is back." Then "If <host> runs
    more than one instance, add `--name <instance>`."
  - **<host>** is the office's host name. When the host is the legacy value "This
    Mac", both the reason ("<office machine / instance>" in the table) and the
    steps say "the office's machine" instead.
  - Never invent an instance name, and never say "this Mac" for the office's
    machine: the browser may be on another device.
  - The restart button doesn't show in this state.
- **Remove…** *(rev 15)*, for a join that ended, appears only in the joining instance's own view. It can't remove the join from a page: the join lives only on its own instance, and only its CLI removes it. It opens a small dialog:

  ```
   Remove Studio Mac's join
   Studio Mac never joined, so nothing is lost. On studio-mac (instance default), run:
     agentic-org office remove-pending 01JBX7…
                                                   [Copy command]  [Close]
  ```

  The command has the office's full ID and, when the instance isn't `default`, `--name <instance>`. The row goes away once the office is removed there (§5c). To try again, the owner sends a new invitation (§6.2).
- **Restart office** submits an authorized HQ operation, routed to the owning instance's `office.restart` capability. It is available only while that instance can receive it; otherwise show its local CLI repair instructions. While it runs: "Restarting…" with a spinner, then the live status. `office.restart` covers Restart office, Try again and Start office (ada). The contract refuses a restart only for `unsupported_version`, `invalid_request` and the `join_*` reasons, so only those rows go without it; every other reason, and a missing one, offers it *(rev 8, rev 15)*. A host whose `hello` doesn't list the capability `office.restart` (A50) gets no restart button on any row (Restart office, Try again, Start office), and the row shows "Update Agentic Org to restart offices from here" instead, as for `unknown_method` *(rev 9)*.
- **While a restart runs** *(rev 16)*, the restart button stays focusable (`aria-disabled` plus `aria-busy`), and the busy state ("Restarting…") is announced.
- **A refusal** *(rev 16)* shows its reason in an alert, and focus stays on the restart button. The refusal message clears when the office's status or reason changes.
- **What `office.restart` answers, and what the app does** *(rev 8)*. The row always shows the office's `status_reason`, not the call's error code.
  - Accepted: keep the spinner until the live status arrives.
  - `conflict` (the office is already starting or running, e.g. a double click or the host's own countdown got there first): no error. Keep the spinner until the live status arrives (Starting, then Online).
  - `unknown_method` (an older host): hide the restart button and show "Update Agentic Org to restart offices from here".
  - `not_found` (the office is gone): refresh the list.
  - `not_authorized`: an error in plain words, "You can't restart this office from here." · Copy details.
  - `not_ready` (the host is still starting): retry once. The host may already hold the call for up to 30 s before it answers, so the app's request timeout must be longer than 30 s. If the retry answers `not_ready` too: "The office host is still starting. Try again in a moment." and the button comes back.
  - `unavailable` (the office's reason is `unsupported_version`, `invalid_request` or a `join_*` reason; it can arrive if the reason changed after the row was drawn): show that reason's row from the table, which has no restart button, so the row can't offer Restart office again *(rev 15)*. *(rev 16)* It shows the reason row, never the host's raw message.
- **Copy details** copies the office name, id, status, reason, the time and the app version. It never includes keys or tokens.

## 4. Native menu (deferred to v2)

There is no v1 menu-bar application, Dock presence, native notification, global
capture shortcut or desktop window. Relevant alerts appear in HQ's browser UI.
The in-page organization switcher lists organizations authorized by that HQ.

## 5. Open organization and enroll an office

An authenticated browser opens an organization available from HQ and can remember
the last selection. Its own machine does not need Agentic Org installed. A browser
cannot start a stopped local daemon; setup/start/status are local CLI operations.

Enrollment happens on the target machine through standalone setup. The CLI checks
the invitation, displays the destination/office identity and execution consent,
then submits the join proof. HQ's browser shows the pending office and check-code
approval. Closing the browser does not cancel a join; restarting setup resumes
the same pending request/key. Identity mismatch, expiry, rejection, incompatible
versions and connectivity failures have distinct explanations and repair actions
(§5c *(rev 15)*).

Same-machine office provisioning must still prove office identity. Display locations
relative to HQ/the selected instance, never infer them from the browser's hostname.
Status and snapshots carry freshness; a disconnected browser says Live updates paused.
See SRV-7 through SRV-12 for the network/authorization behavior.

Signing in, the session and sign out are in
[server-installation-and-access-ux.md §4a](server-installation-and-access-ux.md#4a-sign-in-first-access-and-recovery-code-rev-4).

### 5a. Create organization (instance admin only) *(rev 14)*

Only an account with the instance-admin grant can create an organization. Whoever
creates it owns it. The mechanics are in `docs/design/create-organization.md`.

**Entry point: the organization switcher** (sidebar footer, §2):

```
 ┌──────────────────────────────────────┐
 │ [LL] Lumen Labs                    ✓ │
 │      HQ · Online                     │
 │ [SP] Side Project                    │
 │      HQ · Starting                   │
 ├──────────────────────────────────────┤
 │ + Create organization                │  ← instance admins only
 └──────────────────────────────────────┘
 [LL] Lumen Labs · HQ  ▾                    ← sidebar footer button
```

- An instance admin sees **+ Create organization** as the last item, after a divider.
- Any other account sees only its organizations: no divider, no item, no disabled
  item and no hint. Nothing suggests a right it doesn't have.
- An instance admin with no organizations also gets **Create organization** on the
  empty page after sign-in (`server-installation-and-access-ux.md` §4a.5).

**The form** (slide-over, as for other forms in §2):

```
 Create organization                                    (slide-over)
 Name        [ Side Project                        ]
 ┃ ⓘ It will be stored on HQ instance default (hq-mac), and you'll own it.
 ┃   Its recovery code is then collected on that machine.
                                    [Cancel]  [Create organization]
```

- **Name:** required, trimmed. The form has no other field and no folder picker.
  - Empty: "Give the organization a name."
  - Too long: "Use 64 characters or fewer."
  - Control characters: "The name can't contain control characters."
  - Invisible or direction-changing characters: "The name can't contain invisible or
    direction-changing characters."
  - These are the same messages HQ returns (`crates/org-proto/src/name.rs`), so the
    pre-check and HQ never disagree.
  - Already used, from HQ: "There's already an organization called Side Project
    on this HQ. Choose another name." (names are compared ignoring case, as HQ does).
  - **Create organization** stays enabled; pressing it with an invalid name shows
    the error and sends nothing.
- The note names the HQ instance that will store it and the HQ machine (A93), and
  says the creator will own it.

**States:**

| State | What shows |
| --- | --- |
| Waiting for running agents *(rev 15)* | "Waiting for 3 agents to stop: Kai (Always on), Remy, Riley. Their current sessions must end first. Always-on agents keep running until you pause them." with a spinner, then the list of those agents, each with **Pause** (below the table), then "Side Project is created once they've stopped. Nothing is stopped for it." and **Cancel**. Name is read-only. One agent: "Waiting for 1 agent to stop: Remy. Its current session must end first." The Always-on sentence shows only while an Always-on agent is in the list. See **Waiting for running agents** below the table. **Cancel** stops it: "Side Project wasn't created." and the form is back with the name. If it's already being created: "Too late to cancel: Side Project is being created now." and the panel shows Creating…. Closing the panel doesn't cancel it. |
| Creating | **Creating…**, disabled. Name is read-only. **Cancel** becomes **Close**, with "Closing this panel doesn't stop it." A second click sends nothing. |
| Created, starting | The new organization opens. Its HQ shows **Starting** (§3.1), and the page says "Side Project is starting. This usually takes a few seconds." Toast: "Created Side Project. You own it." The switcher lists it with "HQ · Starting". |
| Ready | The organization's pages load as usual. The recovery-code notice shows for its owner once the code is waiting to be collected (a new organization starts as Being prepared, `server-installation-and-access-ux.md` §4a.6). |
| Created, then stopped | The organization exists: its HQ shows **Stopped** with §3.3's reason. This is not a failed create. |
| Failed | A red alert in the panel, with the reason, **Try again** and **Copy details**. Name keeps its text. Try again is a new attempt. |
| Checking (the response was lost) | "Checking whether Side Project was created…" with a spinner. The browser looks up the same attempt; it never sends a new one while the outcome is unknown. |
| Session ended | The sign-in-again dialog (`server-installation-and-access-ux.md` §4a.4). The form keeps its text. After signing in, press **Create organization** again. |

**Waiting for running agents** *(rev 15; A101, resolved: Rowan, 2026-11-10; X-02
Q9)*. A new organization changes what the agents running on that instance may
access, and the host can't yet ask offices to relaunch their sessions (X-02 Q9's
`layout_changed`; `agent-sessions.md`). So HQ creates it only once **no agent
session on that instance is running**, and never stops or relaunches an agent
for it:
- An agent counts as running while it has a live session, **including Idle**,
  between turns (ADR-0003). It stops counting once it's **Asleep**, **Paused**
  or stopped (Offline). Agents of every organization on that instance count.
- The create stays accepted, and its lookup and HQ's status feed report how many
  sessions it waits for (`waiting_for_idle { sessions: n }`). "3" in the copy is
  that number, and it goes down live as sessions end. When HQ doesn't give the
  number: "Waiting for running agents to stop. Their current sessions must end
  first."
- **It names the agents it waits for** *(review of 02316bb)*, because an
  Always-on agent's session restarts when it ends (`agent-run-modes-ux.md`
  decision 1), so the wait never ends while one is Always on and running.
  Under the copy, one row per agent:

  ```
   Kai     Lumen Labs · Always on · Working                     [Pause]
   Remy    Lumen Labs · On call · Idle                          [Pause]
   Riley   Lumen Labs · Scheduled · Idle                        [Pause]
  ```

  - The name links to the agent's profile. Leaving the panel doesn't cancel the
    create; the switcher's waiting entry reopens it.
  - **Pause** is the agent's usual Pause (`our-org-in-the-app.md` §9, DEAL-3),
    with its dialog. The row then shows "Paused · stop pending at office" and
    no Pause, and leaves the list once the agent's session stops; the number
    goes down with it. Pausing them ends the wait once their sessions stop.
    Nothing is stopped automatically.
  - The copy marks Always-on agents ("Kai (Always on)"). On call and Scheduled
    agents go back to sleep on their own once idle (`agent-run-modes-ux.md`
    decision 7), so they're named without a mark.
  - Agents of an organization this account doesn't own are counted, not named,
    because the admin grant alone doesn't open it (§5b): one row "1 agent in
    Other Org · Ask its owner to pause it", with no link and no Pause. The copy
    reads "Waiting for 3 agents to stop: Kai (Always on), Remy, 1 agent in Other
    Org."
  - Until HQ gives the agents (A106), or when it doesn't, the copy keeps its
    count-only form, without the list.
- There's no time limit: it waits until the sessions end or someone cancels.
  `agentic-org doctor` on the HQ machine lists it with a count for each
  organization and no names, because it runs without a session, so it can't
  tell which organizations its reader owns *(review of 0802754)*: "Creating
  Side Project: waiting for 3 agents to stop (3 in Lumen Labs), since 14:05."
  With an agent of Other Org waiting too, it reads "(2 in Lumen Labs, 1 in
  Other Org)" *(review of 18d3f56)*.
- **Cancel** cancels that same create (its operation ID), from this panel or
  from the switcher's waiting entry, in any tab or browser of that account.
- **While it waits, it isn't an organization yet.** After the panel is closed,
  the organization switcher shows a waiting entry for it, driven by the
  operation: "Side Project · Waiting for 3 agents to stop", with an `hourglass`
  icon. It can't be opened; choosing it reopens this panel in its Waiting state,
  with **Cancel**. Once the create starts, the entry becomes "HQ · Starting" (OFC-23).
  After Cancel it goes away.

**Failure reasons** (the alert's words). `organization.create` fails only when
nothing was stored. An organization that was stored but whose office didn't start
is **Created, then stopped** (above), never a failure, so Try again can't hit
"already exists" (`create-organization.md`; `org-proto` `calls.rs`).

| Wire error | Copy |
| --- | --- |
| `invalid_name` | The matching inline name message (above). |
| `already_exists` | "There's already an organization called Side Project on this HQ. Choose another name." (OFC-17) |
| `storage_error` | "Side Project wasn't created: HQ couldn't write its data on <HQ machine>. Run `agentic-org doctor` there, then try again." |
| `not_ready` | "HQ is still starting. Try again in a moment." **Try again** |
| `unavailable` | "Several organizations are being created right now. Try again in a moment." **Try again** |
| Not an instance admin (refused by the gateway, A95) | "Only an instance admin can create organizations." No Try again. The switcher item goes away. |
| Any other error | "Side Project wasn't created. Try again." |

Whether a recovery package that can't be created fails the create is A89; if it
does, its copy is "Side Project wasn't created: its recovery material couldn't be
prepared. Try again."

**Resolving a lost response** (the Checking state):

| Lookup finds | What shows |
| --- | --- |
| It's waiting for running agents (`waiting_for_idle`) *(rev 15)* | As Waiting for running agents, with its number of sessions. |
| It's still being created | Stays in Checking ("Still creating Side Project…") and checks again every few seconds, never sending a new attempt. |
| It was created | As Created, starting. |
| It failed | As Failed, with its reason. |
| HQ has no record of it | "Side Project wasn't created. Try again." **Try again** resends the same attempt. |
| HQ still can't be reached | "Couldn't confirm whether Side Project was created, because HQ can't be reached." **Check again**. No Try again until the outcome is known. |
| The attempt is too old to look up (A92) | "This attempt is too old to check. Look for Side Project in the organization list before creating it again." **Open organization list** |

- **Copy details** copies the organization name, the attempt's ID, the error code,
  the time and the app version. It never includes keys, tokens or a recovery code.
- **Closing the panel** while Creating or Checking doesn't stop the attempt. A
  created organization appears in the switcher with "HQ · Starting". If the tab
  closes before a lost response is resolved, the organization list shows whether
  it was created.

**Accessibility:** Name has a visible label; its rule, the note and any error are
linked with `aria-describedby`, and an invalid name sets `aria-invalid`. Errors and
the failure alert are announced when they appear, and focus moves to the alert on
failure. Creating…, Checking and Starting are announced through a polite live
region. Waiting for running agents is announced once, with its number, when it
starts; the number going down isn't announced, so it doesn't tick *(rev 15)*.
The agents it waits for are a list; each **Pause** is named "Pause <agent>". An
agent leaving the list isn't announced; if its row had focus, focus moves to the
next row's Pause, or to **Cancel** when none is left. When the new organization opens, focus moves to its main heading. The
panel traps focus and returns it to the switcher button when it closes. In the
switcher, ↑/↓ reach **Create organization**, Enter opens it and Esc closes the menu.

### 5b. Who can do what *(rev 14)*

From the owner's decision O-1. People records and agents never sign in.

| Action | Who | Where |
| --- | --- | --- |
| Sign in | Any account | Browser |
| Create an organization | An instance admin only. Setup creates the first one. | Browser (§5a), or `agentic-org setup` |
| Open an organization and act in it (teams, people, approvals, and inviting, approving and removing satellite offices) | Its owner: any account linked to it. The admin grant alone doesn't open it *(rev 15, server A86 resolved)* | Browser |
| Add an office on this instance *(rev 15)* | An owner who is also an instance admin | Browser (§6.2) |
| See the recovery-code state | The organization's owner | Browser (state only, never the code) |
| Show a recovery code and confirm it's stored *(rev 15)* | An instance admin, signed in on the HQ machine, who confirms it's them there | HQ machine only (`agentic-org recovery release`, `server-installation-and-access-ux.md` §4a.6) |
| Add an account *(rev 15)* | An instance admin, signed in on the HQ machine, who confirms it's them there | HQ machine only (`agentic-org account create`, `server-installation-and-access-ux.md` §4a.2) |
| Link an account to any organization on the instance, unlink it, or revoke an account *(rev 15)* | An instance admin, signed in on the HQ machine, who confirms it's them there. The instance always keeps an instance admin, and each organization a linked account | HQ machine only (`agentic-org account link`, `unlink`, `revoke`, `server-installation-and-access-ux.md` §4a.2) |
| Change your own password *(rev 15)* | Any account, signed in with its current password | HQ machine only (`agentic-org account set-password`) |
| Reset a forgotten password *(rev 15)* | Anyone at the HQ machine who confirms it's them there (Mac), or root (Linux, `sudo`). No sign-in | HQ machine only (`agentic-org account reset-password <username>`) |
| Start, stop or repair the instance | Whoever can run `agentic-org` as the instance's user on the HQ machine | HQ machine only |

- In v1.0, the browser actions that need the instance-admin grant are creating
  an organization (SRV-24) and adding an office on this instance (OFC-7)
  *(rev 15)*. On the HQ machine, the CLI commands that need it are `account
  create`, `link`, `unlink` and `revoke`, and `recovery release` and `recovery
  status` (the account table in `server-installation-and-access-ux.md` §4a.2).
  An owner without the grant can do none of these, and neither can an employee
  or an office.
- `agentic-org office remove-pending` runs on the joining machine, not the HQ
  machine. It needs that instance's admin only when the instance has accounts
  (§5c).
- The account menu says "Instance admin" for an account with the grant
  (`server-installation-and-access-ux.md` §4a.4).
- **No browser screen lists, invites or adds accounts in v1.0.** Add person (§7.4)
  adds a record agents can reach; it has no invite, password or sign-in option.

### 5c. Joining from setup: waiting, errors and repair *(rev 15)*

Setup on the joining machine hands the invitation to its instance, then watches
the join until it ends (`offices-hq-and-satellites.md`, "Provisioning a
satellite"). The examples use instance `default`; for another instance every
command shown includes `--name <instance>`. `01JBX7…` stands for the office's
full ID, which setup always prints in full.

**Signing in first** *(rev 15)*. On an instance that already has accounts (for
example a satellite office joining on the HQ instance), `office.join` needs an
instance admin's session (`offices-hq-and-satellites.md`, "Provisioning a
satellite", step 3; X-02 Q9). Setup asks for an instance admin's username and
password before it hands over the invitation, with the sign-in failures of the
account table in `server-installation-and-access-ux.md` §4a.2. An account
without the grant is refused: "sam isn't an instance admin, so it can't join
offices on this instance. Sign in as an instance admin of instance default." and
nothing is sent to HQ. On an instance with no accounts yet (a new satellite
machine), setup asks for no sign-in here.

**While the join waits** (the host reports `join_pending`), setup shows its state,
the check code and the last error, if any:

```
Joining Lumen Labs as "Studio Mac" (instance default)
Check code:  K7QP 4MZT
At HQ, approve Studio Mac (Offices › Review and approve) and type this code.
The invitation expires tomorrow at 14:02.

Status: waiting for approval at HQ · checked 14:05
```

- **Status** reads "waiting for approval at HQ" while the status read's
  `join_phase` is `awaiting_approval`, then "approved · copying records from HQ"
  once it's `initial_sync` (A102, resolved: Rowan, 2026-11-10; X-02 Q9), with
  the time setup last checked.
- **Last error**, while there is one, is a line under Status. It's cleared when
  the next check works:

  ```
  Status: waiting for approval at HQ · checked 14:05
  Last error: can't reach HQ at hq.example.com:8443 (connection refused), 14:04.
  Setup keeps trying. The request stays open until the invitation expires.
  ```

- In a terminal, the Status and Last error lines update in place; the
  check code never moves, so a screen reader can find it. Without a terminal,
  setup writes a new line only when the state or the error changes.
- **Joined:** "Studio Mac joined Lumen Labs. Office ready." Setup goes on with
  its remaining steps.
- **Ctrl-C while waiting:** "Stopped watching. The join request is still open,
  with the same check code. See it with agentic-org status." The result is
  awaiting approval, not success and not an error (JOIN-6).
- **Noninteractive setup** waits for its bounded time, then ends with the
  `awaiting_approval` result and the same Check code and Status lines once.
- `agentic-org status` shows the same Check code, Status and Last error lines for
  a pending join.

**Waiting for running agents** *(rev 15; A101, resolved: Rowan, 2026-11-10;
X-02 Q9)*. Adding the joining office changes what the agents already running on
this instance may access, so, as for Add office › On this instance (§6.2), the
join waits while any agent session on this instance is running (an Idle agent
counts; an Asleep, Paused or stopped one doesn't). No agent is stopped or
relaunched for it. With an instance admin signed in (an instance with accounts,
"Signing in first", above; here alex, who owns Lumen Labs but not Other Org),
setup shows:

```
Joining Lumen Labs as "Studio Mac" (instance default)
Waiting for 3 running agents to stop…
  Kai (Always on) · Lumen Labs · Pause it in the app
  Remy · Lumen Labs · Pause it in the app
  1 agent in Other Org · ask its owner to pause it
Their current sessions must end first. They aren't stopped or restarted for this.
Always-on agents keep running until they're paused.
Press Ctrl-C to cancel.
```

- The number, the names and the counts go down in place as sessions end
  ("Waiting for 1 running agent to stop…" for one). Without a terminal, setup
  writes new lines only when they change. Then setup goes on as above.
- *(Review of 02316bb; review of 0802754)* **Who's named** follows the
  browser's rule (§5a): agents of an organization the reader doesn't own are
  counted, not named. With an instance admin signed in, setup names the agents
  of organizations that account owns, one line each with "Pause it in the app",
  and "(Always on)" after each Always-on one: its session restarts when it ends
  (`agent-run-modes-ux.md` decision 1), so the join waits until it's paused.
  For each other organization it shows one line, "2 agents in Other Org ·
  ask its owner to pause them" ("1 agent in Other Org · ask its owner to
  pause it" for one), with no names. The Always-on line shows only while an
  Always-on agent is named. There's no CLI command to pause an agent, so setup
  says to pause it in the app (`our-org-in-the-app.md` §9, DEAL-3). Nothing is
  stopped automatically.
- *(Review of 0802754)* **Without a session**, the CLI shows counts only, one
  for each organization, and no names. That covers setup on an instance with no
  accounts, noninteractive setup (even after signing in), `agentic-org status`
  and `agentic-org doctor` (their lines are below). Setup on an instance with no
  accounts, where the only organization is the joining machine's own (here
  Side Project), shows:

  ```
  Waiting for 3 running agents to stop (3 in Side Project)…
  Their current sessions must end first. They aren't stopped or restarted for this.
  Always-on agents keep running until they're paused in the app.
  Press Ctrl-C to cancel.
  ```

  It can't tell which agents are Always on, so the Always-on line always
  shows. Setup still shows counts only there, for consistency, even though
  that machine has just one organization, if any.
- Until the instance gives the agents and their organizations (A106), setup,
  `status` and `doctor` show only the total, "Waiting for 3 running agents to
  stop…", without names or counts for each organization.
- There's no time limit. Noninteractive setup doesn't wait here: it ends with
  the `waiting_for_idle` result and prints, without a session:

  ```
  Waiting for 3 agents to stop before joining (2 in Lumen Labs, 1 in Side Project).
  This instance finishes the join when they stop; see agentic-org status.
  Always-on agents keep running until they're paused in the app.
  To cancel the join: agentic-org office remove-pending 01JBX7…
  ```

  The last line carries this join's office ID, which `remove-pending` needs.
  `agentic-org status` shows the same lines until the wait ends or the join is
  cancelled. `agentic-org doctor` lists the wait: "Joining Lumen Labs as Studio
  Mac: waiting for 3 agents to stop (2 in Lumen Labs, 1 in Side Project), since
  14:05."
- **Ctrl-C here cancels the join** (unlike Ctrl-C while waiting for approval,
  which only stops watching): "Cancelled. Studio Mac wasn't added to instance
  default. To join, run agentic-org setup again with the invitation." Exit
  status Error.

**When the join ends, or can't go on** *(JOIN-7, JOIN-13, JOIN-14)*. Exit status
Error for the ones that end it:

| What happened | The join | Setup and `status` say | Repair (printed under it) |
| --- | --- | --- | --- |
| The invitation expired before approval (`join_expired`) | Ends | "The invitation for Studio Mac expired before the join was approved. Nothing was added to Lumen Labs." | "Remove this request (agentic-org office remove-pending 01JBX7…), then ask for a new invitation at HQ (Offices › Studio Mac › New invitation) and run agentic-org setup again." |
| The invitation was cancelled or replaced at HQ after it was used, or got 3 wrong check codes, which cancel it (`join_cancelled`; HQ refuses the join) | Ends | "This invitation was cancelled or replaced at HQ, so Studio Mac can't join with it. (3 wrong check codes at HQ also cancel it.) Nothing was added to Lumen Labs." | "Remove this request (agentic-org office remove-pending 01JBX7…), then ask the owner for a new invitation and run agentic-org setup again. When approving, type the code exactly as setup shows it." |
| Declined at HQ (`join_rejected`) | Ends | "The join for Studio Mac was declined at HQ. Nothing was added to Lumen Labs." | "Remove this request (agentic-org office remove-pending 01JBX7…). If it was declined by mistake, ask for a new invitation and run agentic-org setup again." |
| HQ's identity doesn't match the invitation: the server at HQ's address doesn't hold the certificate authority the invitation pins (`join_rejected`, a security failure) | Ends, and setup doesn't retry | "Stopped joining: the server at hq.example.com:8443 couldn't prove it's Lumen Labs' HQ. Its identity doesn't match the invitation. Studio Mac didn't join it, and setup won't try again." | "Check HQ's address with the owner of Lumen Labs. This can mean the address now points to another server, or that someone is intercepting the connection. Remove this request (agentic-org office remove-pending 01JBX7…), then ask for a new invitation and run agentic-org setup again." |
| HQ doesn't support this instance's version (unsupported version) | Stays waiting, until the invitation expires | Status: "This instance needs an update to join this HQ. Update it, then setup continues." | "Update Agentic Org on this machine with agentic-org upgrade. Setup checks again on its own. The request stays open until the invitation expires (tomorrow at 14:02)." |
| HQ can't be reached | Stays waiting | The Last error line above, with the reason and its time. | "Check that HQ is running and that this machine can reach hq.example.com:8443. Nothing to remove: setup keeps trying." |

- *(A102, resolved: Rowan, 2026-11-10; X-02 Q9)* An HQ identity mismatch ends
  the join as `join_rejected`, and is never retried. An unsupported version
  keeps it waiting; if the invitation expires first, the join ends as
  `join_expired`, as above. A familiar name with a different identity is still
  flagged before the join starts (JOIN-4).
- Both a decline and an identity mismatch end as `join_rejected`. Setup and
  `agentic-org status` show the matching row when the instance's status says
  which it was (A105). When it doesn't, they show one row for both: "The join
  for Studio Mac was refused: it was declined at HQ, or HQ's identity didn't
  match the invitation. Nothing was added to Lumen Labs." with the repair "Check
  HQ's address with the owner of Lumen Labs. Then remove this request
  (agentic-org office remove-pending 01JBX7…), ask for a new invitation and run
  agentic-org setup again."

- A join that ended is never reported as an office that joined, and it isn't
  restarted.

**Two views of a joining office** *(rev 15)*. The joining instance and HQ each
show only what they decide or see:
- **The joining instance's own view**, its `agentic-org status` (and setup) and,
  if it has one, its local status page: **Waiting for approval** while the join
  waits, and §3.3's "join ended" row (Stopped, with **Remove…**, which shows
  `agentic-org office remove-pending <office id>`) once it ends. These never
  appear in HQ's Offices list.
- **HQ's Offices list** keeps §6.3's rows: **Wants to join** before approval
  (§6.1, OFC-5), then Starting · copying records once approved. After 3 wrong
  check codes the row is **Invitation cancelled**; after a decline or an expired
  invitation, the invitation's row. Each of these has **New invitation** and
  **Remove**.
- An office on HQ's own instance can be in `join_pending` too: a local satellite
  joins its own instance's HQ (`offices-hq-and-satellites.md`, "Provisioning a
  satellite", step 3, Checks). HQ's Offices list still shows HQ's view of it;
  Waiting for approval and the "join ended" row show only in that instance's
  `agentic-org status`.

**Removing a join** (on the joining machine, through its instance): `agentic-org
office remove-pending <office id>`. On an instance that has accounts, it asks an
instance admin to sign in first, as setup does when it joins ("Signing in first", above). Its sign-in failures use
the account commands' words (`server-installation-and-access-ux.md` §4a.2,
SIGN-30), in the table below.

```
$ agentic-org office remove-pending 01JBX7…
Removed the ended join for Studio Mac (Lumen Labs) from instance default.
To join again, ask for a new invitation and run agentic-org setup.
```

For a join that's still waiting for approval, it asks first:

```
$ agentic-org office remove-pending 01JBX7…
Studio Mac is still waiting for approval at HQ. Removing it here stops the join.
If HQ approves it later, remove Studio Mac at HQ too (Offices › Remove office).
Remove it? [y/N]: y
Removed the join request for Studio Mac from instance default.
```

*(Review of 02316bb)* For a join that's still waiting for running agents, the
join hasn't reached HQ (it waits on this instance first), so the prompt doesn't
mention HQ approval. It cancels the join, as Ctrl-C does in setup:

```
$ agentic-org office remove-pending 01JBX7…
Cancel joining Lumen Labs as Studio Mac? The join hasn't reached HQ yet. [y/N]: y
Cancelled. Studio Mac wasn't added to instance default. To join, run agentic-org
setup again with the invitation.
```

| Case | CLI output | Exit |
| --- | --- | --- |
| Waiting for approval, answered anything but y | "Not removed. The join request is still open." | Error |
| Waiting for approval, no terminal | "Studio Mac is still waiting for approval. Run this in a terminal to confirm removing it." Nothing removed. | Error |
| Waiting for running agents, answered anything but y *(review of 02316bb)* | "Not cancelled. This instance finishes the join when the agents stop." | Error |
| Waiting for running agents, no terminal *(review of 02316bb)* | "Studio Mac is waiting for running agents before joining Lumen Labs. Run this in a terminal to confirm cancelling it." Nothing cancelled. | Error |
| Waiting for running agents, but the agents stopped and the join reached HQ before y *(review of 02316bb)* | "Not cancelled: the agents stopped and the join was sent to HQ. Studio Mac is now waiting for approval. Run this again to remove it." Nothing removed. | Error |
| The office has joined | "Studio Mac has joined Lumen Labs, so there's no pending join to remove. To remove the office, use Offices › Remove office at HQ." Nothing removed. | Error |
| Unknown ID | "There's no join request 01JBX7… on instance default. agentic-org status lists them." | Error |
| Wrong username or password at sign-in (an instance with accounts) | "That username and password don't match. Try again." Asks again. Too many tries: "Too many sign-in attempts. Try again in a few minutes." Nothing removed. | — / Error |
| The account that signed in isn't an instance admin | "sam isn't an instance admin, so it can't remove join requests. Sign in as an instance admin of instance default." Nothing removed. | Error |
| Instance not running | "Instance default isn't running. Start it with agentic-org start, then run this command again." | Error |

## 6. Flow 3 · Offices and Add office

### 6.1 Offices screen (nav: Offices)

```
 Offices                                                        [+ Add office]
 Where Lumen Labs' agents run. HQ keeps the records and approves new offices;
 every other office keeps a copy.

 ┌───────────────────────────────────────────────────────────────────────────────┐
 │ ! Laptop wants to join · from 192.168.1.44 · 2 min ago      [Review and approve] │  ← highlighted row
 ├───────────────────────────────────────────────────────────────────────────────┤
 │ [HQ] Main office        HQ instance                 ● Online     4 agents   ⋯ │
 │      Studio Mac         from 192.168.1.31            ● Online     2 agents   ⋯ │
 │      Build box          last seen 2 h ago            ○ Offline    1 agent    ⋯ │
 │      Office 2           HQ instance                     ● Online     0 agents   ⋯ │
 │      Laptop             invitation · expires in 21 h ✉ Invited     [Cancel]  ⋯ │
 ├───────────────────────────────────────────────────────────────────────────────┤
 │ ! 2 unplaced agents · from a removed office                   [Place or offboard] │  ← only when any
 └───────────────────────────────────────────────────────────────────────────────┘
 Shown from HQ. Statuses include last-seen information.
```

- Order: join requests, HQ, offices on the HQ instance, other offices A–Z, invitations, then unplaced agents.
- **A row opens a slide-over** (the details sheet) with a description list: name (**Rename**), role (HQ / Office), where (the service machine/instance, with last-seen address where appropriate), status, last seen, sync ("Up to date" or "Catching up: 34 changes"), Agentic Org version, agents placed here, projects it follows. Actions: **Rename** · **Remove office…** (not on HQ) · later, **Make this HQ…** (phase 4, hidden until then). *(rev 16)* The sheet has one Close control, and closing it returns focus to whatever opened it.
- **Version holding back a feature:** when HQ can't raise the organization level, a quiet note above the list: "Build box runs an older version of Agentic Org, so new features wait until it updates." It links to the row.
- **Empty is impossible** (HQ always exists). With HQ alone, a one-line hint under the list: "Add an office to run agents on another machine, or to keep a copy of the records there."
- **Loading:** skeleton rows. **Live:** statuses update in place; a row that changes status doesn't move.
- **Row label for screen readers** *(rev 6, rev 8, OFC-13)*: "Main office, HQ, HQ instance, Online, 6 agents. Opens details." For Stopped it adds the reason, with a restart in a form that doesn't tick: "…, Stopped: it stopped unexpectedly, restarting shortly, …", or "restarting at 10:42" when the restart is a minute or more away. The visible countdown updates no more often than every 10 s. A status change confirmed by its owning instance is announced through a polite live region ("Office 2 stopped: it stopped unexpectedly", "Office 2 is online"). *(rev 16)* The label doesn't claim "(you're here)" unless the host says so.
- **Footer** *(rev 16, OFC-48)*: "Shown from HQ. Statuses include last-seen information."
- **Historical build note** *(rev 6; not the standalone-v1 acceptance target)*: **Add office** is shown disabled with "Available in a later release." While the data is invented, a **Sample data** badge says so. A status the app can't confirm shows as **Unknown** (§3.1), never as healthy.
- **HQ-only actions:** Add office, Review and approve and Remove office are available to an authorized owner through HQ from any browser. Add office › **On this instance** is offered only to an owner who is also an instance admin (§6.2) *(rev 15)*. Unauthorized sessions cannot invoke or discover privileged operations.

### 6.2 Add office (authorized HQ owner; on this instance, an instance admin)

Enter a unique office name and choose **On this instance** or **On another
machine/instance**. The first uses authenticated local provisioning and records
office identity. Show Creating, Synchronizing and Ready separately; a failure is
visible with its specific reason and an idempotent retry.

**On this instance needs the instance admin** *(rev 15)*, because it provisions a
key and a worker on HQ's instance (`standalone-server.md`, "Create organization").
- An owner who isn't an instance admin isn't offered the choice: Add office opens
  straight on the invitation for another machine, titled "Add an office on another
  machine", with no disabled option and no hint, as for Create organization (OFC-14).
- If HQ refuses it anyway (for example, the grant was removed meanwhile): "Only an
  instance admin can add an office on this instance. You can still invite another
  machine." with **Invite another machine**, which keeps the name. No Try again.
- Inviting, approving and removing a satellite stay open to any authorized owner
  (OFC-7).

**Waiting for running agents** *(rev 15; A101, resolved: Rowan, 2026-11-10;
X-02 Q9)*. A new office on this instance changes what the agents running there
may access, so the office is added only once **no agent session on that
instance is running**, as for Create organization (§5a): an Idle agent, between
turns, still counts; an Asleep, Paused or stopped one doesn't. They're never
stopped or relaunched for it. The form shows "Waiting for 3 agents to stop:
Kai (Always on), Remy, Riley. Their current sessions must end first. Always-on
agents keep running until you pause them." with a spinner, then the list of
those agents, each with a link to its profile and **Pause**, then "Office 2 is
added once they've stopped. Nothing is stopped for it." and **Cancel**. The name
is read-only. The number comes from the operation's `waiting_for_idle {
sessions: n }` and goes down live. The list, its Pause, the agents of
organizations this account doesn't own, and the one-agent and no-number forms
are §5a's *(review of 02316bb)*: pausing them ends the wait once their sessions
stop, and nothing is stopped automatically.
- **Cancel** cancels that same Add office (its operation ID), as for Create
  organization: "Office 2 wasn't added." The form is back with the name.
- Already being added: "Too late to cancel: Office 2 is being added now." Then
  Creating, as usual.
- **In the Offices list,** a waiting office isn't an office yet. Closing the
  form doesn't cancel it, and the list shows a pending row driven by the
  operation, like the switcher's waiting organization: `hourglass`, "Office 2",
  "Waiting for 3 agents to stop · on <instance>", and **Cancel**, which asks
  "Stop adding Office 2? It wasn't added yet." (**Stop adding** / **Keep
  waiting**). It has no status dot, no agents and no other action. Choosing
  the row reopens the form in its Waiting state, with the agents and their
  Pause *(review of 02316bb)*. When the office is added, the row becomes the
  office (Starting, §3.1); after Cancel it goes away.
- There's no time limit, and `agentic-org doctor` on that machine lists it with
  a count for each organization and no names, as in §5a *(review of 0802754)*:
  "Adding Office 2: waiting for 3 agents to stop (3 in Lumen Labs), since
  14:05." *(review of 18d3f56)*
- An office on another machine doesn't wait here. Its own instance may wait the
  same way while it joins (§5c).

For another machine/instance, generate a one-time invitation and show the CLI
setup instructions. The secret is shown once, with Copy invitation and an expiry;
it is not placed in shell arguments or ordinary logs. Setup on the destination
accepts it through a protected input and displays the check code. HQ's owner
approves that request here. Replacing an invitation cancels the previous one.
Expiry/rejection shows New invitation and Remove; a reused setup resumes its
pending request instead of creating a duplicate office. No desktop URL handler or
browser folder picker is required.

### 6.3 Approve a join (through an authorized HQ browser session)

Triggers: the HQ Needs-you inbox and highlighted Offices row. Each opens this accessible dialog; native notifications and menu rows are deferred to v2.

```
 ┌──────────────────────────────────────────────────────────┐
 │ (!) Approve Studio Mac?                                   │
 │ A computer at 192.168.1.44 used the invitation for        │
 │ Studio Mac (created today at 14:02) 2 minutes ago.        │
 │                                                           │
 │ Type the check code shown on Studio Mac                   │
 │ [ K7QP 4MZT                 ]                             │
 │ Verify the code on the destination terminal or through   │
 │ your trusted authenticated session to that machine.      │
 │ Decline unsolicited or unverified relayed codes.         │
 │                                                           │
 │                       [Decline]   [Approve Studio Mac]    │
 └──────────────────────────────────────────────────────────┘
```

- **Approve** is enabled once the code has 8 characters. Case, spaces and dashes are ignored. As in Crockford base32, a typed O counts as 0 and I or L as 1, so look-alikes never cause a mismatch. A U, or any other character that can't be in a code, is rejected as typed with the hint "Codes use digits and letters, without U".
- **Wrong code:** "That code doesn't match Studio Mac's. Check its terminal or your trusted session. 2 tries left." After 3 wrong codes the invitation is cancelled (A3, resolved): "Join declined after 3 wrong codes. The invitation is cancelled; create a new invitation to try again." The row becomes **Invitation cancelled** with **New invitation** and **Remove**.
- **Decline** asks once: "Decline Studio Mac? Its invitation is cancelled." · **Decline**.
- **Approved:** the row moves to the office list as "Starting · copying records" (§3.1), then Online once its first copy of records completes. Toast: "Studio Mac joined Lumen Labs."
- **Two requests on one link** (shouldn't happen, but HQ may see a retry from a new address): both are listed. Approving one declines the rest.

### 6.4 Remove office (through an authorized HQ browser session)

Modal, red icon. "Remove Studio Mac from Lumen Labs? It can't sync again. Its 2 agents stop running there and become unplaced: you choose a new office for each, or offboard them. HQ takes back their 5 tasks. If Studio Mac is offline, it keeps its copy until it next connects, then deletes it. To add it again, send a new invitation." Primary: **Remove Studio Mac** (red).

**After removal** (A4, resolved): the agents are **Unplaced**. They keep their teams, reporting lines and history, but don't run. They show:
- as a needs-you item: "2 agents are unplaced in Lumen Labs…", until each is placed or offboarded;
- in the Offices list's last row, **Place or offboard**, which opens a list of them, each with **Place…** (office picker, the agent profile's placement flow) and **Offboard…** (the HR flow);
- in the org chart with an **Unplaced** tag, and under **Office: Unplaced** in the office filter.

Connection-loss copy follows LIVE-8 exactly: `Live updates paused. Showing what was loaded at <HH:MM>.`, or `Live updates paused.` if nothing has loaded, with Retry / Retrying… and the last successful load time in 24-hour format.

### 6.5 HQ connectivity and office outboxes

An open browser with a lost HQ connection shows Live updates paused and stale data.
It cannot save new organization changes until HQ acknowledges them. A satellite's
local agents retain the existing command/message outboxes; HQ displays their
Pending or Blocked outcomes when known. Retry/discard of a blocked office command
is authorized and uses the office protocol's operation IDs. This is distinct from
an unsaved browser draft. Local work and shutdown supervision continue without a
browser connection.

## 7. Flow 4 · Org chart: Teams, People and Reporting lines

Nav **Org chart**, with pill tabs **Teams | People | Reporting lines**. One chart over the same data: switching tabs keeps the search and the selected member. Toolbar: Search members · **Office: All ▾** (Teams and Reporting lines only) · Collapse all / Expand all · zoom (− % + Fit) · List view · the view's primary action: **New team** (Teams), **Add person** (People) or none (Reporting lines). Editing works from all three views (create a team, add a member, move, add a person, link a project, change a manager).

### 7.0 Agents and people

Agents and people are always told apart by **shape and words**, never by color alone:

| | Agent | Person |
| --- | --- | --- |
| Avatar | Rounded **square** tile with the robot glyph, indigo tint | **Circle** with initials, neutral (gray-700) |
| Shown as | `[R]` in the wireframes | `(S)` in the wireframes |
| Secondary text | its office ("Studio Mac", or **Unplaced**) | contact, if any; never an office |
| Screen reader | "Riley, agent" | "Sam Ortiz, person" |
| On a team card | the **Agents** row | the **People** row |

### 7.1 The owner, above every view

```
 ┌───────────────────────────────────────────────────────────────────────────┐
 │ (AK) Alex Kim (you) · Owner of Lumen Labs                                  │
 │      Full access to every team, member and project. Any agent can          │
 │      message you, ask you or assign you work.                              │
 └───────────────────────────────────────────────────────────────────────────┘
```

- One owner bar, pinned above the chart in all three views. It's the only place the owner appears in Teams and People. In Reporting lines it is also the root (§7.8).
- **Never** on a team card, in "Not in a team", in member or lead pickers, or in team and organization counts.
- The owner bar opens the owner's quick view, which has no Remove, Add to team or Change manager action, only Edit name and contact.
- **Wherever agents' recipients or assignees are picked** (messages, Ask, Assign), the owner is always listed first as "Alex Kim (you)".

### 7.2 Teams view

```
 (owner bar, §7.1)
                         [ Lumen Labs · 4 teams · 7 agents · 3 people ]
                       ┌────────────────────┴─────────────────────┐
         ┌─────────────────────────────┐            ┌─────────────────────────────┐
         │ Product                   ⋯ │            │ Platform                  ⋯ │
         │ Lead: (S) Sam Ortiz         │            │ Lead: [A] ada               │
         │ Agents [E]          1       │            │ Agents [A][R][R]    3       │
         │ People (S)          1       │            │ People —                    │
         │ 2 teams · no projects       │            │ agentic-org app  Shared     │
         └─────────────┬───────────────┘            └─────────────────────────────┘
          ┌────────────┴─────────────┐
 ┌──────────────────────────┐ ┌──────────────────────────┐   ┌ Not in a team (3) ┐
 │ Checkout               ⋯ │ │ Growth                 ⋯ │   │ [J] Jules         │
 │ Lead: [R] Riley          │ │ No lead                  │   │ [N] Nova          │
 │ Agents [R][M]   2        │ │ Agents [E]   1           │   │ (P) Priya Shah    │
 │ People (J)      1        │ │ People —                 │   └───────────────────┘
 │ checkout                 │ │ agentic-org app  Shared  │   ┌ Unassigned projects (1) ┐
 │                          │ │                          │   │ onboarding  Assign team…│
 └──────────────────────────┘ └──────────────────────────┘   └─────────────────────────┘
```

- **Root** is the organization card (name and real counts, not counting the owner). Top-level teams hang off it.
- **Team card:** name · ⋯ menu · lead ("Lead:" with the agent tile or person circle, or "No lead") · **Agents** row and **People** row, each with up to 4 avatars + "+N" and a count (a dash when none) · "N teams" when it owns teams · projects as chips (the first 2, then "+N"), with a **Shared** badge on a project that other teams also work on.
- **Expand a card** (click or Enter) → it lists members under **Agents** and **People** headings: avatar · name · title in this team ("frontend lead", free text) · a **"+2 teams"** badge when they're in others. The badge's popover lists the other teams as links; each link scrolls to and highlights that team.
- **Shared badge:** its popover reads "Also worked on by Growth" with a link; each team's lead is listed as a lead of the project.
- **Not in a team** (a side tray) lists agents and people with no team, so no one disappears. It's hidden when empty. The owner is never in it.
- **Unassigned projects** (a side tray) lists projects with no team, each with **Assign team…**. While any exist, a quiet note above the chart reads "1 project has no team · Assign". Hidden when empty.
- **Office filter:** Office: All · each office · **Unplaced**. Agents placed in the chosen office get a ring and an office tag; everyone else, people included, dims to 40%. A line above the chart reads "Highlighting 3 agents at Studio Mac · Clear". Never hidden, never color only.
- **Selecting a member** (click the avatar) opens their quick view, with **Teams** and **Reports to** sections (§7.4, §7.8).
- **Accessibility:** the chart is a `role="tree"`: ↑ owner team, ↓ first sub-team, ←/→ siblings, Enter expand, Space opens the menu. Connectors are decorative. **List view** shows the same tree as an indented list.

### 7.3 People view (the human org chart)

```
 (owner bar, §7.1)
 ┃ ⓘ Agents can only reach the people listed here. Add someone so your
 ┃   agents can message them, ask them or assign them work.     [Add person]

                         [ Lumen Labs · 3 people ]
                       ┌────────────────────┴─────────────────────┐
         ┌─────────────────────────────┐            ┌──────────────────────┐
         │ Product                   ⋯ │            │ Platform           ⋯ │
         │ Lead: (S) Sam Ortiz         │            │ No people · 3 agents │   ← compact card
         │ (S) Sam Ortiz   lead        │            └──────────────────────┘
         └─────────────┬───────────────┘
          ┌────────────┴─────────────┐
 ┌──────────────────────────┐ ┌──────────────────────┐   ┌ Not in a team (1) ┐
 │ Checkout               ⋯ │ │ Growth             ⋯ │   │ (P) Priya Shah    │
 │ Lead: [R] Riley (agent)  │ │ No people · 1 agent  │   └───────────────────┘
 │ (J) Jo Park   design     │ └──────────────────────┘
 └──────────────────────────┘
```

- The **same team tree**, showing **only people** on the cards. Agents don't appear, except as a team's lead, which is named with "(agent)" so the card still says who leads it.
- Teams without people stay in the tree as **compact cards** ("No people · 3 agents"), so the shape matches the Teams view and you can add someone there (⋯ › **Add person to Platform…**).
- The **info alert** about reaching people always shows (it's the rule that makes adding a person matter).
- **Empty** (no people yet): the tree shows compact cards only, and the alert's primary action is **Add your first person**.
- **Loading, live updates, Pending and Blocked:** as in the Teams view.

### 7.4 Add, edit and remove a person

Entry points: the People toolbar's **Add person**; a team's ⋯ › **Add person to Checkout…** (prefills the team); in any member picker, the last option **Add a new person "Sam"…** (prefills the name).

```
 Add person                                          (slide-over)
 Name         [ Sam Ortiz                ]
 Contact      [ sam@example.com          ]  optional
              How to reach them outside Agentic Org, e.g. an email.
 Teams        [ Search teams…            ]  optional (multi-select, team path shown)
              Product ×
 Reports to   [ (AK) Alex Kim (you) ▾    ]
 ┃ ⓘ Once added, any agent in Lumen Labs can message Sam, ask Sam or
 ┃   assign Sam work.
                                        [Cancel]  [Add person]
```

- **Name:** required, trimmed, no control characters ("Enter a name." / "Names can't contain special control characters."). Names are handles: agents address people by name (`--to "Sam Ortiz"`), so a name must be unique across all members ignoring case, and can't be `owner` *(rev 4)*. As the owner types, a clash shows inline: "Sam Ortiz is already in Lumen Labs. Use a different name, such as Sam Ortiz (Design)." or "codex-dev is an agent's name. Choose another." **Add person** stays disabled until the name is free.
- **Contact** is free text, stored now and used later (the design says so). Not validated beyond trimming.
- **Added:** the person appears in their teams (highlighted for 2 s), or in **Not in a team**. Toast: "Added Sam Ortiz. Agents can now reach Sam." On an office that can't reach HQ, it shows as **Pending**.
- **Person quick view:** name · Person · contact · teams (links) · reports to · their reports · "Agents can reach Sam". Actions: **Edit** (name, contact; clearing contact is allowed) · **Edit teams…** · **Change manager…** · **Remove from Lumen Labs…**.
- **Remove a person** (modal, red icon), naming everything that changes, only the lines that apply:
  > Remove Sam Ortiz from Lumen Labs?
  > - Sam leaves Product and Checkout, and stops leading Product.
  > - Jo Park, who reports to Sam, will report to Sam's manager, Lee Chen.
  > - Agents can no longer reach Sam. 3 messages waiting for Sam won't be delivered, and Sam's 2 open tasks come back unassigned for you to reassign.
  >
  > **Remove Sam Ortiz** (red)

  When Sam has no manager, the second line reads "Jo Park, who reports to Sam, will report to you."
- **Removing an agent** isn't done here: an agent's quick view offers **Offboard…**, which is the HR flow (same consequences, listed the same way).
- **When an agent can't reach someone** (the recipient isn't in the organization, or was removed): the agent's activity shows "Couldn't reach "Sam": Sam isn't in Lumen Labs. **Add a person…**". Removed recipients read "…was removed from Lumen Labs". The owner also sees the reclaimed tasks in Needs you, to reassign.

### 7.5 Create a team (under another team)

Entry points: Teams toolbar **New team**; a team's ⋯ › **Add team under Product** (prefills "Part of"); the empty state.

```
 New team                                           (slide-over)
 Name        [ Checkout                 ]
 Part of     [ Product              ▾ ]   (combobox; "None: top level" first)
 Members     [ Search agents and people… ]  (multi-select; agent tiles and person circles,
             [R] Riley ×   (J) Jo Park ×     each option shows its current teams)
 Lead        [ No lead ▾ ]   (optional; only members chosen above, agents or people)
             The lead also leads Checkout's projects.
                                         [Cancel]  [Create team]
```

- The owner is never offered as a member or lead.
- Validation inline: "Product already has a team called Checkout." (unique among siblings, ignoring case; top-level teams are siblings of each other) · "Name a team (1–64 characters)."
- **Created:** the new card appears under its owner team, highlighted for 2 s, and gets focus. On an office that can't reach HQ it appears **Pending** (§6.5).
- **Empty organization:** "Teams group your agents and people. A team can be part of another team, so a whole division is just a team with teams under it." **Create your first team**. Every member shows in "Not in a team".

### 7.6 Add a member to several teams

Two directions, same result:

- **From a team:** ⋯ › **Add members…** → a combobox multi-select of agents and people; each option shows "Also in: Platform". The last option is **Add a new person…** (§7.4).
- **From a member** (quick view or profile, **Teams** section) → **Edit teams…** → a dialog with the team tree as a checklist, and an optional title per checked team:

```
 Teams for Riley
 ☑ Product
   ☑ Checkout        title [ frontend lead ]
   ☐ Growth
 ☑ Platform          title [ frontend      ]
                                   [Cancel]  [Save]
```

- Unchecking the team a member leads: "Riley leads Checkout. Removing Riley also clears Checkout's lead." · **Remove and clear lead**.
- Unchecking every team: the member moves to **Not in a team**. No warning needed; it's reversible.
- Saving sends one change per team. If one fails, the dialog stays open, names it ("Couldn't add Riley to Growth: Growth was deleted.") and keeps the rest.

### 7.7 Move a team

- **Drag** a card onto another card (or onto the root for top level). While dragging, the card itself and every team under it are dimmed with a "can't drop here" cursor; valid targets get an indigo outline.
- **Without drag:** ⋯ › **Move to…** → a dialog with the team tree; the team and its sub-teams are disabled with "Can't move a team under itself or its own teams".
- **Confirm (both ways):** "Move Checkout (with 1 team and 2 projects) from Product to Platform? Members and projects stay with it." · **Move Checkout**.
- **Done:** toast "Moved Checkout to Platform · **Undo**" (Undo = the reverse move, for 8 s).
- **Rejected by HQ** (someone changed the tree meanwhile): "Couldn't move Checkout: Platform is now under Checkout. Nothing changed." The chart shows the current tree.

### 7.8 Reporting lines view

```
 (owner bar is the root here)
                       ┌ (AK) Alex Kim (you) · Owner ┐
          ┌────────────────────┼────────────────────────┐
 ┌───────────────────┐ ┌───────────────────────┐ ┌───────────────────┐
 │ [A] ada · Agent ⋯ │ │ (S) Sam Ortiz · Person⋯│ │ [J] Jules · Agent⋯│
 │ Platform          │ │ Product, Checkout     │ │ No team           │
 │ 2 reports         │ │ 2 reports             │ │                   │
 └─────────┬─────────┘ └───────────┬───────────┘ └───────────────────┘
      ┌────┴─────┐            ┌────┴─────┐
   [R] Riley  [R] Remy     [E] Eden   (J) Jo Park
 Reporting lines are separate from teams: leading a team doesn't make someone a manager.
```

- **Root:** the owner. Anyone without a manager hangs directly under the owner. Everyone in the organization appears exactly once.
- **Node:** avatar (§7.0) · name · "Agent" or "Person" · their teams (or "No team") · "N reports". Agents show the **Unplaced** tag when it applies; the office filter works as in the Teams view.
- **Change manager:** ⋯ › **Change manager…**, the quick view's **Reports to** section, or **drag** a node onto another (shortcut).

```
 Who does Riley report to?
 [ Search agents and people…        ]
   (AK) Alex Kim (you) · Owner
   [A] ada · Agent · Platform                      (current)
   (S) Sam Ortiz · Person · Product
   [E] Eden · Agent        Reports to Riley         (disabled)
                                         [Cancel]  [Save]
```

- The member isn't listed. Everyone who reports to them, directly or through others, is disabled with "Reports to Riley", so a loop can't be picked.
- **Saved:** no confirmation; toast "Riley now reports to Sam Ortiz · **Undo**" (8 s). The node moves and keeps focus.
- **Rejected by HQ** (a loop created meanwhile): "Couldn't change Riley's manager: Sam now reports to Riley. Nothing changed." The view shows the current lines.
- **The owner's node** has no ⋯ actions apart from opening the quick view: "The owner reports to no one."
- **Removing a member** sends their reports to that member's own manager, or to the owner if they had none (org-proto on v1), as the remove dialog says (§7.4).
- The footnote about teams always shows (decision 16).

### 7.9 Give a project to a team, or share it

- **From a project** (project header and project settings): a **Teams** field, a combobox multi-select with the team path ("Product › Checkout"). Usually one team.
- **From a team:** ⋯ › **Add project…** → a project picker; each project shows its current teams, or "No team".
- **Adding a team** to a project that already has one needs no confirmation: the project now shows on both cards with **Shared**. Toast: "checkout is now shared by Checkout and Platform."
- **Removing a team** (× on the team chip in the project, or the team's ⋯ › project › **Remove from Checkout**): no confirmation while other teams remain. Removing the **last** team confirms: "Remove checkout from Checkout? It will have no team and show under Unassigned projects." · **Remove**.
- **The project's lead:** the project header shows "Lead: Riley (Checkout)", or for a shared project one per team ("Leads: Riley (Checkout), ada (Platform)"), or "No lead" when its teams have none. It changes only through the teams' leads.
- **New project** offers **Teams** prefilled with the team you started from. Leaving it empty creates an unassigned project. (This needs `team_ids?` on `project.create`, which arrives with the projects contract, G1. Until then a new project is created unassigned and the app opens **Assign team…** right after.)

### 7.10 Delete a team, and other states

- **Delete a team that owns teams** is refused up front: "Checkout still has 1 team under it. Move or delete it first." with a link to each. No disabled-without-reason buttons.
- **Delete any other team:** a confirmation naming what changes: "Delete Growth? Eden stays in Lumen Labs and in Product. Its project agentic-org app stays with Platform." When a project or member has nothing else, the dialog says so instead: "onboarding has no other team, so it moves to Unassigned projects." / "Jo Park moves to Not in a team." · **Delete Growth**.
- **Loading:** the owner bar and root card at once, team cards (or nodes) as skeletons.
- **Large organization** (> 30 teams, or > 50 people in Reporting lines): opens collapsed below level 2, with Fit applied.
- **Stale edit** (version conflict): "Checkout changed at HQ while you were editing. Here's the latest." The form reloads with the latest values and keeps the field you typed in, marked.
- **Someone else's change arrives live:** cards update in place; a removed card animates out and focus moves to its owner team (or, in Reporting lines, to the manager).
- **Errors from HQ** that the UI should have prevented (`owner_protected`, `not_a_member`, `already_exists`) show plain messages: "The owner can't be added to a team." · "Riley isn't in Checkout any more, so can't lead it." · "Riley is already in Checkout."

## 8. UX acceptance criteria

**Menu bar (MENU)**
- MENU-1 · Retired from v1 in rev 12. The optional native shell is v2; this ID is not reused.
- MENU-2 · Retired from v1 in rev 12. The optional native shell is v2; this ID is not reused.
- MENU-3 · Retired from v1 in rev 12. The optional native shell is v2; this ID is not reused.
- MENU-4 · Retired from v1 in rev 12. The optional native shell is v2; this ID is not reused.
- MENU-5 · Retired from v1 in rev 12. The optional native shell is v2; this ID is not reused.
- MENU-6 · Retired from v1 in rev 12. The optional native shell is v2; this ID is not reused.

**Open organization and join (OPEN, JOIN)**
- OPEN-1 · *(rev 12)* After authentication, open the last authorized organization for this HQ, or its organization list. This preference does not create a local office.
- OPEN-2 · *(rev 12)* List authorized organizations served by HQ and their offices, with location and freshness. No browser-machine filesystem or local-office discovery is required.
- OPEN-3 · *(rev 12)* The browser and setup never read the clipboard without an explicit user action.
- JOIN-1 · *(rev 12)* Standalone setup validates invitation syntax and expiry before a network join, with a specific error for invalid input.
- JOIN-2 · *(rev 12)* Enrollment requires explicit destination/execution consent in the wizard or automation inputs. A browser URL never silently enrolls a machine.
- JOIN-3 · *(rev 12)* Setup shows organization/office identity, HQ addresses and expiry, and explains that agents run under the configured service user with its permitted access.
- JOIN-4 · *(rev 12)* A familiar organization name with a different identity is flagged; setup cannot silently replace an existing instance or office.
- JOIN-5 · *(rev 12)* The joining CLI displays the check code accessibly; HQ requires the owner to verify that code before approval.
- JOIN-6 · *(rev 12)* Closing the browser does not cancel enrollment. Setup/status can show the existing pending request and its check code again.
- JOIN-7 · *(rev 15)* Setup and `agentic-org status` show an expired invitation, an invitation cancelled or replaced at HQ (3 wrong check codes included) and a declined join as ended, each with its §5c copy and `agentic-org office remove-pending <office id>` as the repair; HQ that can't be reached shows as the last error while the request stays waiting. No failure is reported as an active office.
- JOIN-8 · *(rev 12)* The check code identifies the same persisted invitation/request key across reconnects and setup restarts; a second setup cannot create a duplicate office.
- JOIN-9 · *(new, rev 15)* While a join waits, setup shows its status, the check code and the last error when there is one, and clears the error once a check works.
- JOIN-10 · *(new, rev 15)* `agentic-org office remove-pending <office id>` removes a join that ended without asking, and prints how to join again.
- JOIN-11 · *(new, rev 15; rewritten, review of 02316bb)* `agentic-org office remove-pending` on a join that's still waiting for approval says it stops the join and to remove the office at HQ if HQ approves it later, and removes it only after y in a terminal.
- JOIN-12 · *(new, rev 15)* `agentic-org office remove-pending` on an office that has joined removes nothing and points to Remove office at HQ.
- JOIN-13 · *(new, rev 15, A102)* When the server at HQ's address doesn't match the certificate authority the invitation pins, the join ends as `join_rejected` and setup makes no further attempt. Setup shows §5c's "couldn't prove it's <organization>'s HQ" row, or, while the status can't say which `join_rejected` it was (A105), the combined row; never the decline row alone.
- JOIN-14 · *(new, rev 15, A102)* When HQ doesn't support the joining instance's version, the join stays pending and setup shows "This instance needs an update to join this HQ. Update it, then setup continues." until the invitation expires.
- JOIN-15 · *(new, rev 15, A101; rewritten in review of 0802754)* A join on an instance with running agent sessions waits, and setup shows "Waiting for <n> running agents to stop…" (without a session, "Waiting for <n> running agents to stop (<n> in <org>, …)…", with a count for each organization) and "Press Ctrl-C to cancel", with <n> going down as sessions end; no agent is stopped or relaunched for it.
- JOIN-16 · *(new, rev 15, A101)* Ctrl-C while a join waits for running agents cancels the join: setup prints "Cancelled. <office> wasn't added to instance <name>…", and the instance has no office or pending join for it.
- JOIN-17 · *(new, rev 15)* Each row of §5c's `office remove-pending` table shows its copy and exit status and removes nothing; signed in as an account without the instance-admin grant it's refused with "<username> isn't an instance admin, so it can't remove join requests…".
- JOIN-18 · *(new, rev 15, A101; rewritten, reviews of 02316bb and 0802754)* With an instance admin signed in, setup's wait for running agents names each agent of an organization that account owns, one line each with "Pause it in the app" and "(Always on)" after Always-on agents, and shows "Always-on agents keep running until they're paused." while one is named; each other organization gets one line, "<n> agents in <organization> · ask its owner to pause them", with no names. Without a session (setup on an instance with no accounts, noninteractive setup, `agentic-org status`, `agentic-org doctor`), no agent is named: the wait shows a count for each organization, "(2 in Lumen Labs, 1 in Side Project)", and setup's message always shows "Always-on agents keep running until they're paused in the app." (`agentic-org doctor`'s one line doesn't). Noninteractive setup that reaches the wait ends with the `waiting_for_idle` result and §5c's message: the counts, "This instance finishes the join when they stop", and "To cancel the join: agentic-org office remove-pending <office id>" with this join's office ID. `agentic-org status` shows the same lines and `agentic-org doctor` the wait with its counts, until it ends or the join is cancelled.
- JOIN-19 · *(new, rev 15, review of 02316bb)* `agentic-org office remove-pending <office id>` on a join waiting for running agents asks "Cancel joining <organization> as <office>? The join hasn't reached HQ yet. [y/N]", with no mention of HQ approval, and after y in a terminal prints "Cancelled. <office> wasn't added to instance <name>…", leaving the instance with no office or pending join for it. Any other answer, no terminal, or a join that reached HQ before y cancels nothing, with §5c's copy and exit status Error.
- JOIN-20 · *(new, rev 15)* On an instance that has accounts, setup asks for an instance admin's sign-in before joining; an account without the instance-admin grant is refused with "<username> isn't an instance admin, so it can't join offices on this instance…" and no join request reaches HQ. On an instance with no accounts, setup asks for no sign-in.

**Offices (OFC)**
- OFC-1 · *(rev 12)* HQ lists pending joins, HQ and satellites, with names, locations, status and last-seen/sync information. Browser location does not alter the office classification.
- OFC-2 · *(rev 12)* Adding an office on the same instance uses authenticated provisioning and office identity checks. It is idempotent and failures remain visible without duplicate offices.
- OFC-3 · Add office › On another machine/instance shows the invitation once with Copy invitation, expiry and the shown-only-once warning; closing leaves an Invited row with Cancel invitation and New invitation. Destination CLI setup may be headless.
- OFC-4 · New invitation cancels the previous invitation after confirmation. Approval requires the matching check code observed on the destination terminal or through a trusted authenticated session to that machine; an unsolicited relayed code is insufficient.
- OFC-5 · *(rev 12)* A pending join appears in HQ Needs you and the Offices list and opens the approval dialog. Native notification/menu requirements are deferred to v2.
- OFC-6 · *(rev 2)* Approve is enabled only when the typed code has 8 characters; case, spaces and dashes are ignored, and O, I and L are read as 0, 1 and 1; a wrong code shows the mismatch message with the tries left, and the third wrong code cancels the invitation.
- OFC-7 · *(rev 15)* Add office › On this instance is offered only to an owner who is also an instance admin; any other owner's Add office opens on another machine, with no disabled option or hint. Any authorized owner may invite, approve or remove a satellite through HQ from any browser. Unauthorized clients cannot discover or invoke those actions.
- OFC-8 · *(rev 2)* Remove office names the office, its agents (which become unplaced), their tasks and the offline case before removing.
- OFC-9 · *(rev 12)* HQ shows an unreachable satellite with last-seen/sync state. A browser disconnected from HQ shows stale data and Live updates paused; it does not become an offline satellite or claim drafts are saved. Office outbox outcomes are shown when HQ has them.
- OFC-10 · *(new, rev 2)* After an office is removed, its agents show as Unplaced (Offices list, org chart tag, office filter), with Place… and Offboard… for each, and a needs-you item until every one is handled.
- OFC-11 · *(rev 12)* Stopped/unavailable offices show the reason and an authorized remedy. An office restart is offered only when its owning instance can receive and perform it; starting a stopped daemon requires local lifecycle control.
- OFC-12 · *(rev 12)* Office status arrives through the authenticated HQ HTTP/event contract with capability and freshness information. Missing or stale live status is Unknown, never stale Online.
- OFC-13 · *(rev 12)* Each status has an icon and text, with office name, role, location, reason and agent count available to assistive technology. Countdown announcements do not tick, and changes use a polite live region.

**Create organization and who can do what (OFC)** *(new, rev 14, §5a, §5b)*
- OFC-14 · For an instance admin, the organization switcher ends with a divider and **+ Create organization**. For any other account it has no divider, no Create organization item (enabled or disabled) and no hint.
- OFC-15 · The Create organization form has one field, Name, and no folder picker. Its note names the HQ instance and machine that will store it and says the creator will own it.
- OFC-16 · An empty or too long name, or one with control, invisible or direction-changing characters, shows its inline message and sends no request.
- OFC-17 · A name HQ refuses as already used shows "There's already an organization called <name> on this HQ. Choose another name." and no organization is created.
- OFC-18 · While creating, the button reads **Creating…** and is disabled; double-clicking sends one request.
- OFC-19 · A created organization opens showing Starting, with the toast "Created <name>. You own it.", and then loads normally. Its owner is the account that created it.
- OFC-20 · A confirmed failure shows its reason's copy, keeps the name, and offers Try again and Copy details. Copy details contains no key, token or recovery code.
- OFC-21 · When the create response is lost, the panel shows "Checking whether <name> was created…" and looks up the same attempt. Each lookup outcome shows its copy (including "still being created", which keeps checking), and repeating the check never creates a second organization.
- OFC-22 · HQ's refusal for a missing instance-admin grant shows "Only an instance admin can create organizations." with no Try again.
- OFC-23 · Closing the panel while creating doesn't stop it: the new organization then appears in the switcher with "HQ · Starting".
- OFC-24 · The form meets §5a's accessibility: labelled Name with linked rule and errors, `aria-invalid` on an invalid name, announced failures with focus on the alert, polite announcements of progress, focus on the new organization's heading, and focus returned to the switcher on close.
- OFC-25 · Add person and the person quick view offer no invite, password or sign-in option.

**Joins that end, waiting for running agents, and offices on this instance (OFC)** *(new, rev 15, §3.3, §5a, §6.2)*
- OFC-26 · In the joining instance's own view (its status and, if it has one, its local status page), a satellite whose join ended (`join_expired`, `join_cancelled`, `join_rejected`) shows its §3.3 copy and Remove…, which shows `agentic-org office remove-pending <office id>` with Copy command, and never offers Restart office, Try again or Start office. HQ's Offices list never shows this row: after 3 wrong check codes, a decline or an expired invitation it shows §6.3's row with New invitation and Remove.
- OFC-27 · On the joining instance's own status (CLI or local status page), when `office.restart` answers `unavailable` because the office's join ended, the office shows the join-ended row, with no restart action. HQ's Offices list is unaffected and keeps §6.3's rows.
- OFC-28 · *(rewritten in rev 15, reviews of 07a8a4b and 02316bb)* Create organization while any agent session on the HQ instance is running shows "Waiting for <n> agents to stop: <names>. Their current sessions must end first…" with Cancel, and creates the organization only once no agent session on that instance is running (each agent Asleep, Paused or stopped); no agent is stopped or relaunched for it. It lists each agent it waits for in the account's organizations, with a link to its profile and Pause (DEAL-3), marks Always-on ones "(Always on)", and says "Always-on agents keep running until you pause them." while one is listed. Agents of an organization the account doesn't own show as a count with no name, link or Pause. Pausing a listed agent shows "Paused · stop pending at office", and the agent leaves the list and the count once its session stops; with an Always-on agent running, the wait doesn't end until it's paused.
- OFC-29 · *(rewritten in rev 15, reviews of 07a8a4b and 02316bb)* Add office › On this instance while any agent session on that instance is running shows "Waiting for <n> agents to stop: <names>. Their current sessions must end first…" with Cancel, and adds the office only once no agent session on that instance is running (each agent Asleep, Paused or stopped); no agent is stopped or relaunched for it. Its list of agents, their links and Pause, the Always-on copy and the agents of other organizations behave as in OFC-28.
- OFC-30 · Cancel while waiting for running agents creates nothing and keeps the name; once creating has started it shows "Too late to cancel…" and the create continues.
- OFC-31 · HQ's refusal of Add office › On this instance for an account without the grant shows "Only an instance admin can add an office on this instance. You can still invite another machine." with Invite another machine and no Try again.
- OFC-32 · *(rewritten in rev 15, A102)* In the joining instance's own view, a satellite whose join is pending (`join_pending`) shows Waiting for approval while its `join_phase` is `awaiting_approval`, then Starting · copying records while it's `initial_sync`, and never Online before its first copy of records completes. HQ's Offices list never shows Waiting for approval: it shows Wants to join before approval, then Starting · copying records until the first copy completes, and never Online before then, also for an office on HQ's own instance.
- OFC-33 · *(new, rev 15)* An agent that's Idle, between turns, keeps a create or Add office waiting; an agent that's Asleep, Paused or stopped doesn't.
- OFC-34 · *(new, rev 15, A101)* The waiting copy's number is HQ's `waiting_for_idle` session count, and it goes down without a reload as sessions end.
- OFC-35 · *(new, rev 15, A101)* Cancel on a waiting create or Add office, from the panel, another tab, the switcher's waiting entry or the Offices list's pending row, cancels that same operation, and the organization or office is never created.
- OFC-36 · *(new, rev 15, A101)* After its panel is closed, a waiting organization shows in the switcher as "<name> · Waiting for <n> agents to stop", can't be opened, and reopens the panel in its Waiting state when chosen.
- OFC-37 · *(new, rev 15, A101; rewritten, review of 02316bb)* A waiting Add office shows in the Offices list only as a pending row with "Waiting for <n> agents to stop" and Cancel, with no status dot or other action; choosing it reopens the form in its Waiting state with the agents and their Pause, and it becomes the office once it's added.
- OFC-38 · *(new, rev 15, A101; rewritten, review of 0802754)* A wait for running agents has no time limit, and `agentic-org doctor` on that machine lists each waiting create, Add office and join with its number of agents and a count for each organization ("2 in Lumen Labs, 1 in Other Org"), naming no agent.

**Repair, restart and details (OFC)** *(new, rev 16, §3.3, §6.1)*
- OFC-39 · *(new, rev 16)* For `unsupported_version`, the toggle reads "Show local CLI repair instructions", then "Hide repair instructions" while they're shown, with `aria-expanded` and `aria-controls`, and focus stays on the toggle.
- OFC-40 · *(new, rev 16)* The repair instructions show "On <host>, run:", then `agentic-org doctor` in a code block with Copy command, which changes to Copied; then "It checks the Agentic Org instance on that machine and says what to repair. Follow its steps there. This page updates on its own when the office is back."; then "If <host> runs more than one instance, add `--name <instance>`."
- OFC-41 · *(new, rev 16)* <host> is the office's host name; when the host is the legacy value "This Mac", the reason and the steps both say "the office's machine". The copy never invents an instance name and never says "this Mac" for the office's machine.
- OFC-42 · *(new, rev 16)* The office row's screen-reader label doesn't claim "(you're here)" unless the host says so.
- OFC-43 · *(new, rev 16)* An office whose reason is `unsupported_version` shows no restart button.
- OFC-44 · *(new, rev 16)* While a restart runs, the restart button stays focusable (`aria-disabled` plus `aria-busy`), and the busy state is announced.
- OFC-45 · *(new, rev 16)* A refused restart shows its reason in an alert with focus kept on the restart button, and the message clears when the office's status or reason changes. A restart refused as `unavailable` shows the reason row, not the host's raw message.
- OFC-46 · *(new, rev 16)* The restart countdown never overstates the time left: it rounds down, and it's correct from its first frame, including right after a status push.
- OFC-47 · *(new, rev 16)* The office details sheet has one Close control, and closing it returns focus to whatever opened it.
- OFC-48 · *(new, rev 16)* The Offices list's footer reads "Shown from HQ. Statuses include last-seen information."

**Org chart: Teams (TEAM)**
- TEAM-1 · *(rev 2)* The Teams view draws the ownership tree from the organization root; each card shows its name, the lead or "No lead", an Agents row and a People row with avatars and counts, its sub-team count and its projects.
- TEAM-2 · A member in several teams appears in each, with a "+N teams" badge listing the others.
- TEAM-3 · *(rev 2)* Agents and people in no team appear in "Not in a team"; the owner never does.
- TEAM-4 · *(rev 2)* The office filter (each office, and Unplaced) highlights agents placed there with a ring and a tag, dims everyone else including people, and hides no one.
- TEAM-5 · *(rev 2)* New team supports a "Part of" team, members (agents and people) and an optional lead chosen from those members; the owner is never offered; a duplicate sibling name shows the inline error.
- TEAM-6 · Edit teams adds or removes a member in several teams at once, warns when it clears a lead, and reports any single failure without losing the others.
- TEAM-7 · Moving a team works by drag and by **Move to…**; a team and its sub-teams are never offered as targets; the confirmation names what moves; Undo is offered for 8 s.
- TEAM-8 · *(rev 2)* A project's teams can be added and removed from the project and from the team; adding a team needs no confirmation, and removing a project's last team confirms that it becomes unassigned.
- TEAM-9 · *(rev 2)* Deleting a team that owns teams is refused before any request, listing them; deleting any other team confirms first, naming the members who move to Not in a team and the projects that become unassigned.
- TEAM-10 · *(rev 2)* All three views are fully usable from the keyboard (tree navigation, menus, Move to…, Change manager…), and each has a List view.
- TEAM-11 · *(new)* The org chart has three tabs, Teams, People and Reporting lines, over the same data; switching keeps the search and the selected member, and editing is available from each.
- TEAM-12 · *(new)* Agents and people are told apart without color: agents have a square tile with the robot glyph, people a round avatar with initials, and screen readers say "agent" or "person" after the name.
- TEAM-13 · *(new)* A project worked on by several teams shows on each of their cards with a "Shared" badge whose popover lists the other teams.
- TEAM-14 · *(new)* Projects with no team show under "Unassigned projects" with Assign team…, and a note above the chart counts them; both are hidden when there are none.
- TEAM-15 · *(new)* The lead picker says the lead also leads the team's projects, and a project shows its lead per team ("No lead" when none).

**Org chart: People (PEOPLE)**
- PEOPLE-1 · The People view shows the same team tree with only people on the cards; teams without people stay as compact "No people" cards; agents appear only when naming a team's lead, marked "(agent)".
- PEOPLE-2 · The People view always shows that agents can only reach the people listed in the organization.
- PEOPLE-3 · Add person (from the toolbar, a team's menu, or "Add a new person" in a member picker) requires a name and takes an optional contact, teams and manager (the owner by default); the person then appears in their teams, or in Not in a team.
- PEOPLE-4 · *(rev 4)* A name that matches any member's handle ignoring case (a person's name, an agent's messaging name, or `owner`) is refused inline with a suggestion, and Add person stays disabled. An empty or invalid name shows the inline error.
- PEOPLE-5 · Removing a person confirms first, naming the teams they leave and stop leading, who will report to whom instead (the person's own manager, or the owner if they had none), and that their waiting messages won't be delivered and their open tasks come back for reassignment.
- PEOPLE-6 · People never show an office, and the office filter never highlights them.
- PEOPLE-7 · When an agent's message or task fails because the recipient isn't in the organization (or was removed), the agent's activity says so in plain words, with Add a person… for an unknown recipient.
- PEOPLE-8 · *(new, rev 4)* Renaming a person checks the same handle rule, and the rename dialog says agents will address them by the new name from now on.

**Org chart: Reporting lines (REPORT)**
- REPORT-1 · The Reporting lines view draws who reports to whom with the owner at the root; anyone without a manager hangs directly under the owner, and everyone appears exactly once.
- REPORT-2 · Each node shows the name, Agent or Person (styled as in TEAM-12), their teams and their number of reports.
- REPORT-3 · Change manager… (and drag, as a shortcut) lists the owner first, never lists the member, and disables everyone who reports to them, directly or through others, with the reason.
- REPORT-4 · A changed manager shows a toast with Undo for 8 s; a rejection from HQ says nothing changed and shows the current lines.
- REPORT-5 · The view says reporting lines are separate from teams, and no change to a team or its lead changes anyone's manager.

**The owner (OWNER)**
- OWNER-1 · The owner appears once, above the chart in every view, with "(you)", "Owner", full access and that any agent can message, ask or assign them; never on a team card, in Not in a team, or in team and organization counts.
- OWNER-2 · The owner is never offered as a team member or lead, and their quick view has no Remove, Add to team or Change manager action.
- OWNER-3 · The owner is first in every "Reports to" picker and in every recipient and assignee picker agents' work uses, and is the root of Reporting lines.

**Retired:** GUILD-1, GUILD-2 (rev 2); MENU-1 through MENU-6 from v1 (rev 12). IDs are not reused.

## 9. Open questions

**For the user**
- ~~U5~~ · *Resolved (the user, 2026-11-08): as recommended.* *My recommendation:* keep **Lumen Labs** for designs and mock data. It's neutral and can't be confused with the app or with our own organization.
- ~~U6~~ · *Resolved (the user, 2026-11-08): as recommended.* *My recommendation:* keep "HQ" and "office". They're short and plain, and the layout doesn't depend on them.

**For ada**
- ~~A50~~ · *Resolved (ada):* a capability list in `hello` (`capabilities: ["office.status_changed", "office.restart", …]`, additive; absent means none). Without `office.status_changed` the app shows "Status unknown"; without `office.restart` it hides the restart button (as for `unknown_method`). `HelloResult.capabilities` is on v1 (`crates/org-proto/src/calls.rs`, with `capability::OFFICE_RESTART`, `OFFICE_STATUS_CHANGED` and `HelloResult::supports`; ada, merged at `b9f1198`).
- ~~A38~~ · *Resolved (ada):* no rename. "Open organization" stays for an organization already on this Mac, and "Join organization" is reserved for the invitation link (phase 3).
- A7 · Remove office: I've made it HQ-only, like Add office and approval. Confirm, or should it work from any office while HQ is reachable?
- A8 · Is there a limit on how many people an organization can add, or on a person's name length? I've assumed the team-name rules (1–64 characters).
- A9 · Does the agent's `delivery_failed` notice carry the name the agent used, so PEOPLE-7 can say "Couldn't reach "Sam"" and prefill Add person?
- A91 · *(rev 14)* The name rules follow `org-proto` (trimmed, 64 characters, no control, invisible or direction-changing characters, duplicates compared case-insensitively on the trimmed name). Open: is the duplicate check per HQ instance or per account?
- A95 · *(rev 14)* Which error does the gateway return when an account without the admin grant calls Create organization, and does a key-store failure during create surface as `storage_error` or something else?
- A92 · *(rev 14)* How long can a create attempt be looked up after a lost response, and what does HQ answer once it's too old?
- ~~A101~~ · *Resolved (Rowan, 2026-11-10; X-02 Q9):* a waiting create or Add office stays `accepted`, and its lookup and the instance status feed report `waiting_for_idle { sessions: n }`, so the copy gives the number. Cancel uses the original operation ID, as for Create organization. A waiting office isn't in the Offices list yet: it's a pending row driven by the operation, like a waiting organization in the switcher. A satellite join on an instance with running agents waits the same way, and setup shows "Waiting for N running agents to stop…" with "Press Ctrl-C to cancel". There's no longest wait: the person can always cancel, and `agentic-org doctor` shows the wait (§5a, §5c, §6.2; OFC-33 to OFC-38, JOIN-15, JOIN-16).
- ~~A102~~ · *Resolved (Rowan, 2026-11-10; X-02 Q9):* the status read adds `join_phase: awaiting_approval | initial_sync` under `join_pending`, so the joining instance shows Waiting for approval, then Starting · copying records (§3.1, §5c, OFC-32). `join_cancelled` also covers an invitation the owner cancelled or replaced after it was used (HQ refuses the join). An HQ identity mismatch ends the join as `join_rejected`, a security failure that isn't retried. An unsupported version keeps the join pending until the invitation expires (§5c; JOIN-7, JOIN-13, JOIN-14).
- A105 · *(rev 15)* A decline at HQ and an HQ identity mismatch both end as `join_rejected` (A102). Setup and `agentic-org status` need to tell them apart to show the right copy and repair (§5c): a decline asks for a new invitation, a mismatch asks to check HQ's address first. Does the instance's status say which one it was, so setup, `status` and the §3.3 row can say so? Until it does, they show one combined row for both (§3.3, §5c).
- A106 · *(rev 15, reviews of 02316bb and 0802754)* The wait for running agents names the agents it waits for in organizations the reader owns, and counts the rest by organization (§5a, §5c, §6.2). `waiting_for_idle { sessions: n }` gives only a count. Can a read made with a session (the create's and Add office's lookup, HQ's status feed, and the joining instance's status read by setup with an instance admin signed in) give each running agent's member ID, name, organization, run mode and status for the organizations that account owns, and a count for each other organization, so the browser can link and pause them and setup can name them? And can a read without a session (`agentic-org status`, `agentic-org doctor`, setup on an instance with no accounts, noninteractive setup) give only a count for each organization, with no names? Until they do, the wait shows its total-only copy, without names, counts by organization or Pause.
- A89 and A93 in `server-installation-and-access-ux.md` §4a.9 also affect §5a and §5b: whether a failed recovery package fails the create, and the HQ machine's name in copy. *(rev 15)* Its A86 is resolved: the admin grant alone opens no organization.

**Resolved in revision 2:** U1 (wording kept for now), U2 (reporting lines stay as their own view), U3 (no departments, no labels), U4 (one optional lead, agent or person; owner in no team; shared projects allowed), A1 (§5.2 warning), A2 (stable code, §5.3), A3 (three wrong codes cancel, §6.3), A4 (unplaced agents, §6.4), A5 (Add office and approval on HQ's Mac, §6.1), A6 (8 Crockford base32 characters in two groups of four).

## 10. Out of scope here (follow-ups)

The placement flow itself (Place… opens the agent profile's flow, specced separately); hiring and offboarding (the HR flow); messaging and task screens beyond the PEOPLE-7 notice; promotion (phase 4, "Make this HQ…" reserved); the recovery-code screens, which are in `server-installation-and-access-ux.md` §4a.6 *(rev 14)*; the design-system glossary and the v1 spec revision 5, which fold this spec in.
