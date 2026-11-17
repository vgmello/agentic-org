---
kind: spec
title: "Offices, organizations and teams (UX)"
status: for-review
revision: 19
owner: ac-product-designer-eden
created: "2026-11-08"
updated: "2026-11-15"
requested_by: ac-staff-engineer-ada
reviewers: [ac-staff-engineer-ada, owner]
builds_on: []
---

# Spec · Offices, organizations and teams (UX)

Selected chart data follows [organization](../design/organization-data/01-organization-structure.md), [projects](../design/organization-data/02-project-structure.md), [employees](../design/organization-data/04-agent-structure.md) and [task assignment](../design/organization-data/12-task-assignment-and-history.md). The body and criteria below use those target relationships. Detailed office/enrollment UX remains a requirement; target records do not prove handlers or migration are implemented. [Authority map](../design/documentation-authority.md).

**Revision 12 (2026-11-09): standalone server pivot.**
[server-installation-and-access-ux.md](server-installation-and-access-ux.md) supplies
the v1 installation/access requirements. The browser's machine is not an office.
MENU-1 through MENU-6 are retired from v1 (native shell deferred to v2), OPEN/JOIN
and OFC criteria below are revised for HQ HTTP access and CLI enrollment. The later selected chart semantics below supersede the old membership/owner exceptions. Old changelog entries record
history and do not reintroduce the desktop flows.

Requested by ada for the user.

**Sources:** ada's `docs/design/offices-hq-and-satellites.md` (approved), `docs/design/teams-and-org-chart.md` (**approved by the user 2026-11-08**) and `docs/design/create-organization.md`. Look and feel follows the [shared shell standards](app-shell-ux.md#6a-shared-visual-and-interaction-rules): gray-800 surfaces, flat rows and the accessible indigo accent.

**What it changes in my earlier work:** it replaces, for the office model, `desktop-app-v1.md` (in Eden's notes, not in this repository) §2.5 (workspace switcher), §3.2 (department and reporting-line chart) and §7.1 (Machines). Revision 5 of that spec folds this in. Acceptance-criterion IDs here are stable: changed criteria are rewritten under the same ID, retired IDs are never reused.

## Changelog

**Revision 19 (2026-11-15):** Teams index, mixed Members directory and one reporting tree replace the three canvases; retain selected home-team/accountability/editing rules and move Offices under Organization settings.

**Revision 18 (2026-11-15):** rewrites chart forms, diagrams, model wording and TEAM/PEOPLE/REPORT/OWNER criteria for one home team, subtree leads, independent reporting, required project owner and optional contributors. Owner/HR employee exemptions are removed. Existing wire adapters need an explicit migration; no new commands are introduced. Layout-changing operations wait for verified turn/process quiescence and cleanup, without depending on deferred run modes or automatic restart. Undo remains deferred.

The entries below are historical revision notes, not active relationship or identifier requirements. Pre-selection statements about multiple memberships, teamless owners, name-as-handle and ownerless projects are superseded individually by §§7.0–7.10 and their revised criteria.

**Revision 17 (2026-11-15):** initial agentic-org setup creates HQ only and registers its service automatically. Satellite onboarding is a separate HQ UI/issued-command flow. Destination enrollment repair copy no longer tells users to run initial setup. Enrollment approval, consent and retry rules remain unchanged.

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
- **The countdown** never overstates the time left: it rounds down to a 10 s
  step, updates on those boundaries, and reads "Restarting shortly" under 10 s
  (§3.3, §6.1).
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

- *(Historical review of 07a8a4b)* Clarified that the old session-based wait covered idle sessions too and did not stop/relaunch agents. Revision 18 replaces sleep/pause labels as completion evidence with verified active-turn/process cleanup.
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
- *(Historical review of 02316bb)* Added authorized profile links/Pause controls, pending-operation cancellation and office-ID CLI repair details. Its automatic run-mode/sleep explanation is superseded by revision 18's explicit turn/process/drain boundary; the controls/privacy/cancel behavior is retained in the flows below.
- *(Review of 0802754)* The CLI follows the browser's rule for agents of
  organizations the reader doesn't own: counted, not named. Setup with an
  instance admin signed in names the agents of organizations that account owns,
  each with "Pause it in the app", and shows "<n> agents in <organization> · ask
  its owner to pause them" for each other organization. Without a session
  (`agentic-org service status`, `doctor`, setup on an instance with no accounts, and
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

**Revisions 1–5 (2026-11-08), historical chart work:** introduced the three views, distinct agents/people, reporting and owner presentation. Revision 18 replaces the earlier multi-membership, teamless-owner, display-name-as-handle and symmetric project-link relationships throughout the body/criteria. [The earlier spec](https://github.com/vgmello/agentic-org/blob/c6e520a36c4dc7c738c68915fac5e7d5d1cbe804/docs/specs/offices-and-teams-ux.md) preserves those obsolete forms and decisions. Retained criteria IDs include TEAM-1–15, PEOPLE-1–8, REPORT-1–5 and OWNER-1–3. GUILD-1/2 remain retired and are not reused.

## 0. Decisions in this spec

1. **HQ organization navigation.** The in-page switcher lists authorized organizations and their offices. The native menu is deferred to v2.
2. **Switching organization never asks for confirmation.** Offices run under the login service, so nothing stops when you switch. The only prompt is for unsaved input ("Discard the new team?").
3. **The UI says "HQ" and "office", never "satellite".** HQ gets a badge; every other office is just an office. (`satellite` stays the engineering term.) The user is OK with this for now and may revisit it.
4. **Locations identify the service machine or instance.** Support macOS and Linux; never label the browser machine as the executing office.
5. **Services run without a browser.** Pending joins and agents needing attention appear in HQ Needs you; native background notifications are v2.
6. **Enrollment uses a separate HQ-issued destination flow, never initial setup:** validate invitation, confirm destination/execution consent, await HQ approval, synchronize and report readiness.
7. **The invitation is shown once.** HQ retains only its verifier/public material. The screen uses **Copy invitation** and **New invitation**; creating a replacement cancels the old invitation. Destination setup accepts the secret through protected input, not a desktop URL handler.
8. **The check code identifies the joining request/key.** The CLI displays it on the joining machine; the owner verifies it through a trusted session to that machine. Do not treat terminal copy prevention as a security guarantee.
9. **Check-code input is one text field**, not one box per character (screen readers, paste, password managers). Case, spaces and dashes are ignored.
10. **Teams** contains the Teams index, Members directory and one Org chart reporting tree. Offices lives under Organization settings; the old three canvases and top-level Offices item are replaced (§7).
11. **There are only teams.** No guilds, no divisions, no labels. A team that owns teams simply shows "3 teams".
12. **Every edit has a non-drag path** (menu, dialog, keyboard). Drag-to-move and drag-to-change-manager are shortcuts, never the only way.
13. **Office outboxes report Pending and Blocked.** HQ shows their known outcomes. A disconnected browser marks its view stale and its own unsubmitted text unsaved; it does not become a satellite outbox.
14. **Agents and people never look alike**, and the difference is never color only (§7.0).
15. **The owner account bar is above the chart.** A linked owner employee retains its required home team and is counted/displayed there; account authority is separate from employee identity.
16. **Reporting references and team hierarchy are distinct.** A lead manages the entire team subtree; an ordinary ancestor-team employee does not. Explicit reporting remains visible independently.
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
| Lead | Optional direct home-team employee appointed to manage its whole subtree | Inferred project co-lead |
| Reports to / Manager | Explicit employee reporting reference, separate from subtree lead responsibility | Inferred ancestor-team manager |
| Contributor | Optional participating team; the project still has one accountable owner | Co-owner |
| Unplaced | An agent without current execution placement; home team is still required | Teamless |
| Unassigned | A task with no individual and/or assigned team, independent of required accountable team | Ownerless project |

"Department" (from the role catalog) is not a grouping in the app, and there are no labels.

The owner and the instance admin are often the same account, but they are different words for different rights: the owner has full access to one organization; the instance admin can create organizations (§5b) *(rev 14)*.

## 2. Visual direction (Tailwind Plus application UI)

Apply [shared shell standards](app-shell-ux.md#6a-shared-visual-and-interaction-rules)
to these flows. Lists use flat divided rows; forms use the shared controls and
right sheets, confirmations use dialogs, and switchers use modal menus. An owner
account card uses a neutral white/3 surface and white/10 outline, not an indigo
fill. A selected chart/profile entity never gains authority from its styling.
Navigation tabs use links/underline when they have distinct URLs.

Office pills use green for Online, sky for Starting/Checking, gray for Invited,
an outline for Offline/Unknown, red for Stopped/Unreachable, and amber only when
an owner action is required (for example approving a join or an update). Words,
icons and freshness distinguish states; color does not prove readiness.

## 3. Status vocabulary

### 3.1 Office status (one at a time, from HQ, with reported location and freshness)

| Label | Icon (Heroicons) | Color | When | Secondary text |
| --- | --- | --- | --- | --- |
| Online | `check-circle` | green | The office runs and is in sync with HQ (HQ itself: it runs) | — |
| Starting | `arrow-path` | sky | Its owning instance reports the office starting | —; for a joining office HQ has approved, "copying records" until its first copy of records completes *(rev 15)* |
| Offline | `signal-slash` | gray | The office is disconnected from HQ; its current execution state is not confirmed | "last seen 2 h ago" |
| Unreachable | `signal-slash` | red | HQ cannot be reached by the browser or the joining CLI | "last synced 10 min ago" |
| Stopped | `x-circle` | red | Its owning instance confirms the office is stopped/unavailable | the reason from §3.3, in plain words |
| Update needed | `arrow-up-circle` | amber | The office's server is too old for the organization's level | "needs Agentic Org 1.4 or later" |
| Invited | `envelope` | gray | An invitation link exists and hasn't been used | "expires in 21 h" |
| Wants to join | `bell` | amber (needs you) | A computer used the link and waits for its check code at HQ | "from 192.168.1.44 · 2 min ago" |
| Waiting for approval | `clock` | amber | The selected joining office awaits HQ approval. Only in the joining instance's own view, never in HQ's Offices list *(rev 15)* | "type the code at HQ" |
| Unknown | `question-mark-circle` | gray | The app has no live status for it, or the status is `unknown` or missing (never shown as healthy) *(rev 6, rev 8)* | "Status unknown" |

**From the wire to the label** *(rev 8)*. `OfficeInfo.status` shows as one label:

- `active` → **Online**. `starting` → **Starting**. `update_needed` → **Update needed**. `pending` (invited, not joined yet) → **Invited**.
- **Two views of a joining office** *(rev 15)*. The joining instance's own view is its `agentic-org service status` (and setup) and, if it has one, its local status page. HQ's Offices list is HQ's view. Each shows only its own states, as below (§5c).
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

**Countdown** *(rev 8)*: whenever `next_restart_at` is present, whatever the reason, the copy ends with "Restarting in N s" (N from `next_restart_at`; for `in_use` the row's own "Trying again in N s"). The host sends it only while the office is `offline` with a restart pending. The screen-reader label doesn't tick (§6.1). *(rev 16)* The visible countdown never overstates the time left: it shows the remaining time rounded **down** to a 10 s step ("Restarting in 40 s" while 40 to 49 s remain), and updates on those 10 s boundaries. Under 10 s it reads "Restarting shortly" (for `in_use`, "Trying again shortly"). It's correct from its first frame, including right after a status push.

| `status_reason` | Copy (secondary text) | Action |
| --- | --- | --- |
| `crashed` | "It stopped unexpectedly", then "Restarting in 20 s" when a restart is pending | **Restart office** · Copy details *(rev 8)* |
| `start_timeout` | "It didn't start in time" | **Restart office** · Copy details |
| `restarts_exhausted` | "It kept stopping, so it won't restart on its own" | **Restart office** · Copy details |
| `storage_error` | "It can't read or write its data on <office machine / instance>" (also used when the Keychain doesn't allow access, `office-host-boundary.md`) | **Restart office** · Copy details *(rev 8)* |
| `in_use` | "Its data is still in use by another Agentic Org process. Trying again in N s" (N from `next_restart_at`) *(rev 8)* | **Restart office** · Copy details (the host also retries by itself) *(rev 8)* |
| `key_missing` *(this state goes away when [ADR-0016](../adr/0016-no-application-level-data-encryption.md) is implemented)* | "This office's key isn't available on <office machine / instance>, so its data can't be opened" *(rev 10)* (the databases exist, and no key that opens them is available to the owning instance: missing, or not the right one) | **Try again** (`office.restart`, which asks for the key again and returns straight to this reason if it still isn't available) · Copy details. The button says Try again, never Restart. |
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
 Slug        [ side-project                        ]
 Org Git     [ https://example.com/org/state.git   ] remote repository
 State branch[ main                                ]
 Bootstrap   [ Choose valid home team(s)…          ] owner/HR employees if created
 ┃ ⓘ It will be stored on HQ instance default (hq-mac), and you'll own it.
 ┃   Its recovery code is then collected on that machine.
                                    [Cancel]  [Create organization]
```

- **Name:** required, trimmed. No browser-machine folder picker is offered. The selected target additionally requires explicit organization slug, a remote organization repository registration, state branch and valid chosen home teams for any owner/HR employees created during bootstrap. These may be separate setup steps, but must be resolved before admitting those records; never guess a team name or create teamless employees. Code repositories may be local; organization Git cannot. Current name-only create APIs require an explicit target adapter, not invented new request fields on that wire.
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
| Waiting for running agents *(rev 18)* | “Waiting for 3 agents to stop: Kai, Remy, Riley. Their active work and process cleanup must finish first.” Spinner, authorized agent list with supported **Pause** control (below), then “Side Project is created once the instance confirms it is safe. No agent is automatically stopped or restarted for this.” **Cancel** remains available and fields are read-only. One agent: “Waiting for 1 agent to stop: Remy.” No run-mode badges or sleep prediction. See **Waiting for running agents** below. Cancel cancels this operation: “Side Project wasn't created”, returning to the retained form. If creation already began, show “Too late to cancel: Side Project is being created now” and Creating…. Closing the panel does not cancel it. |
| Creating | **Creating…**, disabled. Name is read-only. **Cancel** becomes **Close**, with "Closing this panel doesn't stop it." A second click sends nothing. |
| Created, starting | The new organization opens. Its HQ shows **Starting** (§3.1), and the page says "Side Project is starting. This usually takes a few seconds." Toast: "Created Side Project. You own it." The switcher lists it with "HQ · Starting". |
| Ready | The organization's pages load as usual. The recovery-code notice shows for its owner once the code is waiting to be collected (a new organization starts as Being prepared, `server-installation-and-access-ux.md` §4a.6). |
| Created, then stopped | The organization exists: its HQ shows **Stopped** with §3.3's reason. This is not a failed create. |
| Failed | A red alert in the panel, with the reason, **Try again** and **Copy details**. Name keeps its text. Try again is a new attempt. |
| Checking (the response was lost) | "Checking whether Side Project was created…" with a spinner. The browser looks up the same attempt; it never sends a new one while the outcome is unknown. |
| Session ended | The sign-in-again dialog (`server-installation-and-access-ux.md` §4a.4). The form keeps its text. After signing in, press **Create organization** again. |

**Waiting for running agents** *(rev 18; A101's existing operation tracking retained)*. A new organization changes the layout/policy context available to processes on this instance. The operation waits for the affected work to drain and for verified safe process cleanup before changing that context. [Execution document 11](../design/organization-data/11-model-profiles-and-agent-execution.md) owns turn/attempt/process evidence; automatic schedules, Always on/On call modes and restart-on-idle behavior are deferred and are not prerequisites for this flow.

- Count affected employees with active/open turns, live main processes or unresolved stopping/cleanup evidence, including a warm idle process when it holds the old layout context. A status label such as Idle, Paused, Offline or Asleep does not prove quiescence. Admit the layout change only when the owning process controller confirms no affected active work/live process remains and cleanup is complete. Stale/unavailable evidence keeps the operation waiting with “Can't confirm that work has stopped on <instance>.” No cache deletion, elapsed timeout or zero presence count substitutes for this proof.
- Waiting does not automatically stop, cancel, restart or schedule agent work. Any necessary drain/admission control and safe stop must use explicitly supported, authorized lifecycle actions and durable control state; do not imply a newly implemented drain service or invent a command. Later explicit work still requires current execution readiness, placement generation and permits.
- The create remains accepted. Preserve existing operation lookup/status and the current-contract `waiting_for_idle { sessions: n }` field, but map that legacy session count to verified target turn/process blockers through an explicit adapter. A count alone cannot prove target quiescence. Counts update in place as blockers clear. Without a trustworthy number show “Waiting for active work and process cleanup to finish”; if verification is unavailable say so instead of reporting zero or ready.
- **Authorized list:** one row per affected employee, with a profile link, observed turn/process state, observation freshness and any supported Pause action:

  ```text
  Kai     Lumen Labs · Turn running                             [Pause]
  Remy    Lumen Labs · Cleanup required, last observed idle             [Pause]
  Riley   Lumen Labs · Stop pending at office
  ```

  - Profile navigation does not cancel the operation; the switcher's waiting entry reopens it.
  - **Pause** uses the usual authorized dialog (`our-org-in-the-app.md` §9, DEAL-3), when that capability is supported. Show **Pause requested · stop pending at office** until confirmed. Pause acceptance alone does not remove the blocker: current work and any retained process/descendants still need verified cleanup. Explain an unavailable action or required explicit safe stop/drain; never predict that a mode will send the employee to sleep.
  - Agents in organizations this account does not own are counted, not named. Show “1 agent in Other Org · Ask its owner to finish or safely stop the work”, with no link/Pause; admin status alone grants no organization access. Main copy can read “Waiting for 3 agents to stop: Kai, Remy, 1 agent in Other Org.”
  - Until HQ provides authorized per-employee lifecycle evidence (A106), retain the count-only fallback, with no fabricated rows, mode labels or controls.
- No time limit: wait for verified quiescence or Cancel. `agentic-org doctor` without a session shows supported counts by organization and no names: “Creating Side Project: waiting for 3 agents to stop (2 in Lumen Labs, 1 in Other Org), since 14:05.” Unknown lifecycle evidence stays explicit.
- **Cancel** uses this same operation ID from this panel, its waiting switcher entry or another authorized browser tab. It cancels only the layout operation, never resumes or restarts agents or clears independent manual/security/budget holds.
- **While waiting, it is not an organization yet.** Closing the panel leaves an operation-driven switcher entry: hourglass, “Side Project · Waiting for 3 agents to stop”. Selecting it reopens this panel with Cancel. Once creation starts, the entry becomes “HQ · Starting”; Cancel removes the waiting entry.

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

**Owner scope amendment (2026-11-15):** setup in the legacy enrollment copy below means the destination flow initiated by HQ's Add office UI or an HQ-issued command, not agentic-org setup. Initial setup creates HQ only and has no satellite option. This separate flow reuses installation/service internals and existing approval/repair semantics; the public HQ-issued command is supplied by HQ, not a new initial-setup flag.

The HQ-issued onboarding flow on the joining machine hands the invitation to its instance, then watches
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
  with the same check code. See it with agentic-org service status." The result is
  awaiting approval, not success and not an error (JOIN-6).
- **Noninteractive setup** waits for its bounded time, then ends with the
  `awaiting_approval` result and the same Check code and Status lines once.
- `agentic-org service status` shows the same Check code, Status and Last error lines for
  a pending join.

**Waiting for running agents** *(rev 18)*. Joining this instance changes its process layout/access context, so the join waits for the same verified turn/process drain and cleanup boundary as §5a. A warm idle process or unresolved cleanup can block; a Paused/Offline label alone cannot clear it. No mode engine, automatic sleep or automatic restart is assumed. With an instance-admin session (alex owns Lumen Labs but not Other Org):

```text
Joining Lumen Labs as "Studio Mac" (instance default)
Waiting for 3 running agents to stop…
  Kai · Lumen Labs · Turn running · Pause or safely stop it in the app
  Remy · Lumen Labs · Process cleanup pending
  1 agent in Other Org · ask its owner to finish or safely stop the work
Active work and process cleanup must finish first.
No agent is automatically stopped or restarted for this.
Press Ctrl-C to cancel.
```

- Numbers/names/counts update in place as verified blockers clear; one agent uses singular copy. Nonterminal output writes a new line only when state changes. Continue only after the owning controller verifies quiescence; missing/stale cleanup evidence keeps waiting with its reason.
- **Who's named** follows §5a. Name only employees of organizations the session owns, with observed turn/process state and supported authorized controls. Setup can point to Pause/safe stop in the app but must not invent a CLI endpoint or promise Pause alone finishes cleanup. Other organizations show counts and the instruction to ask their owner. No Always on/On call/Scheduled badge or sleep prediction appears.
- **Without a session**, show supported counts per organization and no employee names. This includes noninteractive output, `agentic-org service status`, `agentic-org doctor` and setup without accounts. For example:

  ```text
  Waiting for 3 running agents to stop (3 in Side Project)…
  Active work and process cleanup must finish first.
  No agent is automatically stopped or restarted for this.
  Press Ctrl-C to cancel.
  ```

  If lifecycle evidence/counts are unavailable, show “Waiting for active work and process cleanup to finish; verification unavailable” rather than inventing zero blockers or a run-mode explanation. Until A106's organization counts exist, retain only the supported total.
- No time limit. Noninteractive enrollment returns its existing `waiting_for_idle` result without blocking; the legacy wire result needs the verified lifecycle adapter described in §5a. It prints:

  ```text
  Waiting for 3 agents to stop before joining (2 in Lumen Labs, 1 in Side Project).
  This instance finishes the join after verified work/process cleanup; see agentic-org service status.
  No agent is automatically stopped or restarted for this.
  To cancel the join: agentic-org office remove-pending 01JBX7…
  ```

  The last line uses this join's full office ID. Service status preserves the same supported counts/reason until completion or cancellation. Doctor lists the operation, counts and waiting-since timestamp without names. Unsupported target lifecycle evidence must be called out rather than claiming the existing count contract already supplies it.
- **Ctrl-C here cancels the join** (unlike Ctrl-C while waiting for approval,
  which only stops watching): "Cancelled. Studio Mac wasn't added to instance
  default. To join, rerun the HQ-issued onboarding command with the invitation." Exit
  status Error.

**When the join ends, or can't go on** *(JOIN-7, JOIN-13, JOIN-14)*. Exit status
Error for the ones that end it:

| What happened | The join | Setup and `status` say | Repair (printed under it) |
| --- | --- | --- | --- |
| The invitation expired before approval (`join_expired`) | Ends | "The invitation for Studio Mac expired before the join was approved. Nothing was added to Lumen Labs." | "Remove this request (agentic-org office remove-pending 01JBX7…), then ask for a new invitation at HQ (Offices › Studio Mac › New invitation) and rerun the HQ-issued onboarding command." |
| The invitation was cancelled or replaced at HQ after it was used, or got 3 wrong check codes, which cancel it (`join_cancelled`; HQ refuses the join) | Ends | "This invitation was cancelled or replaced at HQ, so Studio Mac can't join with it. (3 wrong check codes at HQ also cancel it.) Nothing was added to Lumen Labs." | "Remove this request (agentic-org office remove-pending 01JBX7…), then ask the owner for a new invitation and rerun the HQ-issued onboarding command. When approving, type the code exactly as setup shows it." |
| Declined at HQ (`join_rejected`) | Ends | "The join for Studio Mac was declined at HQ. Nothing was added to Lumen Labs." | "Remove this request (agentic-org office remove-pending 01JBX7…). If it was declined by mistake, ask for a new invitation and rerun the HQ-issued onboarding command." |
| HQ's identity doesn't match the invitation: the server at HQ's address doesn't hold the certificate authority the invitation pins (`join_rejected`, a security failure) | Ends, and setup doesn't retry | "Stopped joining: the server at hq.example.com:8443 couldn't prove it's Lumen Labs' HQ. Its identity doesn't match the invitation. Studio Mac didn't join it, and setup won't try again." | "Check HQ's address with the owner of Lumen Labs. This can mean the address now points to another server, or that someone is intercepting the connection. Remove this request (agentic-org office remove-pending 01JBX7…), then ask for a new invitation and rerun the HQ-issued onboarding command." |
| HQ doesn't support this instance's version (unsupported version) | Stays waiting, until the invitation expires | Status: "This instance needs an update to join this HQ. Update it, then setup continues." | "Update Agentic Org on this machine with agentic-org upgrade. Setup checks again on its own. The request stays open until the invitation expires (tomorrow at 14:02)." |
| HQ can't be reached | Stays waiting | The Last error line above, with the reason and its time. | "Check that HQ is running and that this machine can reach hq.example.com:8443. Nothing to remove: setup keeps trying." |

- *(A102, resolved: Rowan, 2026-11-10; X-02 Q9)* An HQ identity mismatch ends
  the join as `join_rejected`, and is never retried. An unsupported version
  keeps it waiting; if the invitation expires first, the join ends as
  `join_expired`, as above. A familiar name with a different identity is still
  flagged before the join starts (JOIN-4).
- Both a decline and an identity mismatch end as `join_rejected`. Setup and
  `agentic-org service status` show the matching row when the instance's status says
  which it was (A105). When it doesn't, they show one row for both: "The join
  for Studio Mac was refused: it was declined at HQ, or HQ's identity didn't
  match the invitation. Nothing was added to Lumen Labs." with the repair "Check
  HQ's address with the owner of Lumen Labs. Then remove this request
  (agentic-org office remove-pending 01JBX7…), ask for a new invitation and run
  the HQ-issued onboarding command again."

- A join that ended is never reported as an office that joined, and it isn't
  restarted.

**Two views of a joining office** *(rev 15)*. The joining instance and HQ each
show only what they decide or see:
- **The joining instance's own view**, its `agentic-org service status` (and setup) and,
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
  `agentic-org service status`.

**Removing a join** (on the joining machine, through its instance): `agentic-org
office remove-pending <office id>`. On an instance that has accounts, it asks an
instance admin to sign in first, as setup does when it joins ("Signing in first", above). Its sign-in failures use
the account commands' words (`server-installation-and-access-ux.md` §4a.2,
SIGN-30), in the table below.

```
$ agentic-org office remove-pending 01JBX7…
Removed the ended join for Studio Mac (Lumen Labs) from instance default.
To join again, ask for a new invitation and run the HQ-issued onboarding command.
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
| Unknown ID | "There's no join request 01JBX7… on instance default. agentic-org service status lists them." | Error |
| Wrong username or password at sign-in (an instance with accounts) | "That username and password don't match. Try again." Asks again. Too many tries: "Too many sign-in attempts. Try again in a few minutes." Nothing removed. | — / Error |
| The account that signed in isn't an instance admin | "sam isn't an instance admin, so it can't remove join requests. Sign in as an instance admin of instance default." Nothing removed. | Error |
| Instance not running | "Instance default isn't running. Start it with agentic-org service start, then run this command again." | Error |

## 6. Flow 3 · Offices and Add office

### 6.1 Offices screen (Organization settings › Offices)

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
- **Row label for screen readers** *(rev 6, rev 8, OFC-13)*: "Main office, HQ, HQ instance, Online, 6 agents. Opens details." For Stopped it adds the reason, with a restart in a form that doesn't tick: "…, Stopped: it stopped unexpectedly, restarting shortly, …", or "restarting at 10:42" when the restart is a minute or more away. The visible countdown shows the remaining time rounded down to a 10 s step and updates on those boundaries, with "Restarting shortly" under 10 s (§3.3) *(rev 16)*. A status change confirmed by its owning instance is announced through a polite live region ("Office 2 stopped: it stopped unexpectedly", "Office 2 is online"). *(rev 16)* The label doesn't claim "(you're here)" unless the host says so.
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

**Waiting for running agents** *(rev 18)*. Adding an office on this instance uses §5a's verified active-turn/process/drain boundary. An idle retained process or unresolved cleanup still blocks; an employment, presence or mode label never proves work is stopped. The form shows “Waiting for 3 agents to stop: Kai, Remy, Riley. Their active work and process cleanup must finish first”, spinner, the authorized profile/state list with supported Pause controls, then “Office 2 is added once the instance confirms it is safe. No agent is automatically stopped or restarted for this.” **Cancel** stays available and fields are read-only.

Existing `waiting_for_idle { sessions: n }` lookup/count behavior is retained as current-contract evidence; the target needs an explicit lifecycle adapter and confirmed cleanup before admission. Unknown evidence keeps waiting. The authorized controls, per-organization privacy, singular/count-only forms, independent holds and absence of automatic mode restart follow §5a. Pause requests alone never remove a blocker before verified cleanup.
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

**After removal** (A4, resolved): agents become **Unplaced** while retaining their required home teams, reporting and history. Show stop/cleanup as pending until confirmed; durable placement generations, fencing and process reconciliation must prevent duplicate execution. Office removal does not itself prove processes stopped or authorize deleting task workspaces. Safe transfer preserves task worktrees until closure or explicit migration. They show:
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

## 7. Flow 4 · Teams, Members and Org chart

`/teams`, `/teams/members` and `/teams/org-chart` share the Teams · Members · Org chart view switch. Selecting a view focuses its heading. Entity routes use stable IDs and old routes redirect with query strings preserved (app-shell-ux.md §0).

### 7.0 Agents and people

Agents and people share the initial-avatar pattern; kind is written as a word,
not inferred from a robot glyph, color or avatar shape:

| | Agent | Person |
| --- | --- | --- |
| Avatar | Circle with initials | Circle with initials |
| Kind label | Agent | Person |
| Secondary text | Executing office and freshness, or **Unplaced** | Contact if any; never execution office |
| Screen reader | “Riley, agent” | “Sam Ortiz, person” |
| Home-team card | Agents row | People row |

Every employee has one required home team and appears once in that tree. Display `@handle` separately from a freely editable display name; names need not be unique. People and agents share the organization-wide handle namespace. Employment (`active | inactive`), legacy operating state and observed process status are separate.

### 7.1 The owner account and linked employee

```text
┌──────────────────────────────────────────────────────────────────────────┐
│ (AK) Alex Kim (you) · Owner of Lumen Labs                                 │
│ Account authority: full access to this organization. [Employee profile]  │
└──────────────────────────────────────────────────────────────────────────┘
```

- The owner bar presents authenticated account authority; the account is not counted as an employee merely because it signs in. An optional linked owner employee is a normal home-team member and is counted once there. Its `is_owner` marker neither grants authentication nor waives required membership.
- A linked owner employee can be shown on its home-team card and in eligible member/lead pickers subject to ordinary direct-membership rules. Account-removal protections are separate from employee editing. Changing an employee name, handle, home team or reporting line cannot transfer ownership of the account.
- **Message/Ask** can retain the current owner security-principal recipient route. Task assignment stores an employee ID: list a linked active owner employee if present; do not manufacture one from a sign-in account. Current wire `owner` addressing stays unchanged until explicitly migrated.
- The built-in HR shortcut may sit beside the bar. Its employee also has a valid home team and appears in the team and reporting views; the shortcut does not hide it from employee counts. Earlier built-in lifecycle/manager restrictions remain a separate UX policy to implement; home membership itself never appoints HR as a lead or manager.

### 7.2 Teams index, Members directory and team pages

Teams is an indented divided-row index of the recursive team tree, showing TEAM- reference/name, optional direct lead, direct people/agent counts and active projects with accountable Owner/Contributor roles. Keep New team, Move team, home-team moves, Change manager and project owner/contributor editing under their existing revision/authorization/safety rules. Project owner cannot be removed without a replacement.

Members is the mixed directory, filtered All · People · Agents. Each divided row shows avatar/name/handle/role, kind in words, truthful presence where available and Working on {task} when known. People means human employees. Show the authenticated owner account as an account card, not a manufactured employee. If an owner has a linked employee, that employee remains a normal member in its required home team and appears/counts once. HR's employee obeys the same home-team/count rule.

Org chart is one vertical reporting tree, replacing the earlier Teams/People/Reporting lines canvases, their View menu, Fit/zoom and chart-specific search. Put the owner account card above the tree as presentation; records without an explicit employee manager are grouped there as No explicit manager without inventing a manager ID. A linked owner employee keeps its actual reporting position. Team hierarchy lives in Teams rows and employee discovery in Members/top-bar search. Preserve keyboard tree navigation, collapse/expand, employee menus, Change manager and a profile link on every employee node. Reporting and subtree-lead authority remain separate and must not silently rewrite one another.

A team page `/teams/:teamId` shows name/reference, optional lead, real counts, purpose and Hire or add (Hire an agent / Add a person). Above its tabs show at most six member rows and All N members. Tabs are Work, Projects, Activity, Conversations; Work is default. Work rows show reference/title, project · assignee · priority metadata and one trailing work-state pill. Team members do not repeat their team in each row. Missing tab data uses honest empty states. Routines is absent until its engine is selected and served.

Delete/archive on the Teams index moves focus to the next surviving row, else the previous row or index heading; it never returns to a removed control. Project-team removal retains the corresponding next/previous/remove-region focus rule. A view-switch navigation focuses its destination heading.



### 7.4 Add, edit and remove a person

Entry points: toolbar, team menu (prefills home team), last member-picker option **Add a new person “Sam”…**.

```text
Add person                                               (slide-over)
Name         [Sam Ortiz                  ] required
Handle       [sam-ortiz                   ] required, shown as @sam-ortiz
Contact      [sam@example.com             ] optional
Home team    [Product · TEAM-1          ▾ ] required, single choice
Reports to   [No explicit manager       ▾ ] optional employee reference
Once added, agents in Lumen Labs can reach Sam as a participant.
                                            [Cancel] [Add person]
```

- Name is trimmed, required and has no control characters. Display-name duplicates are allowed. Handle is a separate explicit lowercase identifier using permitted letters/digits/dots/underscores/hyphens; check uniqueness across people and agents, not by renaming a display name. Preserve current reserved recipient addresses/ID disambiguation at the compatibility boundary. Inline errors name the unavailable handle and suggest another; do not silently overwrite it when the name changes.
- Contact remains optional free text (trimmed); clearing it is allowed. Home team cannot be cleared. If none exists, **Create team…** returns to this form with the chosen valid team before submission.
- Added: show once at the home team, highlight for two seconds and focus it. Toast “Added Sam Ortiz. Agents can now reach Sam.” Pending on the receiving office's outbox is distinct from an accepted committed record.
- Quick view: name/handle, Person, contact, Home team, Reports to and reports. Actions: Edit · Move home team… · Change manager… · Deactivate…. Employment and account access remain separate.
- Deactivation preserves profile, ID, home team and handle. Explain affected lead appointments, reports and current assignments and require their explicit resolution in the supported operation. Clear or reassign the individual assignment without changing task accountable team or status merely because the person leaves. Report histories remain immutable. Destructive deletion is only available for an unreferenced mistake with no history, after backend validation; do not promise a cascading delete.
- Unknown/removed messaging participants show a plain failure with **Add a person…** where appropriate; task assignment validates active employees independently. Existing queued-message delivery/dead-letter behavior is governed by the messaging contract, not by the chart mutation.

### 7.5 Create a team (under another team)

```text
New team
Name        [Checkout                   ]
Part of     [Product · TEAM-1          ▾ ] “None: top level” first
Lead        [No lead                    ] optional, direct member only
                                            [Cancel] [Create team]
```

- A new empty team needs no lead. To appoint one, explicitly select/create an employee whose home team is this team; moving an existing employee requires the same reviewed home-team move as §7.6. Never silently add another membership. Explain that the lead manages the whole subtree, including child-team leads.
- New team numbers come from organization-wide allocation, never depth/name. Client-side name checks preserve compatible length/control-character constraints and backend conflict feedback. Do not invent a target uniqueness constraint from a display name alone; existing sibling-name restrictions remain in the current wire adapter.
- Created: card appears under its parent, highlights for two seconds and receives focus. Show Pending/Blocked accurately. Empty organization: “Teams group your agents and people. A team can contain other teams.” **Create your first team**. Create a valid team before any employee or owned project can be accepted, including owner/HR employee bootstrap.

<a id="76-add-a-member-to-several-teams"></a>

### 7.6 Move an employee’s home team

The former heading is retained only as an explicit link anchor. The selected action is **Move home team…**, with one required destination and optional home-team title. Additional memberships are not offered.

```text
Move Riley's home team
From       App · TEAM-3
To         [Office · TEAM-5           ▾] required
Title      [frontend lead             ] optional
Riley leads App. Resolve or clear that appointment before the move.
Current task assignments and project participation do not change automatically.
                                             [Cancel] [Move Riley]
```

The move uses the expected employee revision and updates profile `home_team_id` plus stable-ID path together, resolving any old-team lead appointment in the same recoverable change. No transient teamless record. Preserve ID, handle, reporting and history. File/workspace changes require an explicit controller plan; no recursive workspace deletion. On stale revision retain input and show current home/lead state. If unsupported by the current service, explain that the target move is unavailable rather than submitting unrelated membership calls as if atomic.

### 7.7 Move a team

- Drag onto another card or root; self and descendants are disabled with a reason. Menu **Move to…** offers the identical tree dialog and keyboard path.
- Confirm: “Move Checkout (with 1 child team and 2 projects) from Product to Platform? Its employees and owned projects keep their identities; subtree lead responsibility follows the new hierarchy.” Both canonical `parent_team_id` and team path change together.
- Done: “Moved Checkout to Platform.” **Undo is deferred**; no automatic eight-second reverse mutation is claimed. A later reverse move is a new authorized revision-checked operation.
- A concurrent cycle/revision conflict changes nothing and displays current state: “Platform is now under Checkout. Choose another parent.” Preserve focus/input.

### 7.8 Org chart

Draw `reports_to_employee_id` separately from team hierarchy. The owner bar can act as a visual grouping root for records with no explicit employee manager; label that absence **No explicit manager**, not an inferred foreign key to the owner. A linked owner employee appears exactly once in its recorded reporting position, retaining its home team. Built-in HR is an employee in this view too.

Nodes show shape/name/handle, Agent or Person, one Home team and number of explicit reports; agents show office/Unplaced and freshness. Always explain: “Reporting lines are explicit. Team leads also manage their team's subtree; ordinary ancestor-team members do not.”

**Change manager…** (node menu, quick view or drag shortcut) offers active eligible employees plus **No explicit manager**. The employee itself is absent and descendants in the reporting graph are disabled with “Reports to Riley”. Save uses expected revision; toast names the selected manager, node moves with focus retained. A concurrent loop/conflict refreshes current lines and preserves input. Undo is deferred. Editing one graph does not silently rewrite the other; show both sources of responsibility when relevant. Deactivation/lead moves resolve live references explicitly rather than fabricating a new reporting line.

### 7.9 Give a project to a team, or share it

- Project settings: **Accountable owner team** (required single combobox), **Contributing teams** (optional multiple choice). A team menu can open the project with the intended owner/contributor action made explicit. Show reference and full path for every team.
- New project requires an owner before submission, prefilled from the launching team when applicable. No unassigned project is created and no follow-up dialog repairs a deliberately omitted owner.
- Adding/removing a contributor does not change accountability or current assignments. Each participating card links to the project's owner. Do not label every participating lead as the project's lead.
- Changing owner previews affected tasks. The service updates required project owner and all project tasks' accountable `team_id` with durable history in the same recoverable operation. Assigned teams/individuals remain unchanged, even if outside the new owner team.
- Owner cannot be removed/cleared. Pick a replacement; its availability and revision are checked at admission. Project records retain required integration branches per repository association, independent of team participation and the office's shared repository cache.

### 7.10 Delete a team, and other states

- Prefer **Archive** to preserve team IDs/history. Explain how current employees, owned projects/tasks and live assignments/lead appointments will be resolved before admitting an incompatible lifecycle change.
- Delete is blocked while child teams, home employees, accountable projects/tasks or other durable references remain. List blockers with links and explicit move/reassign/archive actions. No cascade into teamless employees or ownerless projects; no silent history deletion. “Checkout still has 1 child team and 2 home employees. Resolve them first.”
- Loading: owner bar/root then skeleton cards/nodes. Large organization (>30 teams or >50 reporting nodes): collapse below level two; keep expand/collapse and keyboard navigation, without Fit/zoom. Live updates apply in place, retaining selection; removed/archived card focus goes to its parent or a stable ancestor.
- Revision conflict: “Checkout changed while you were editing. Here's the latest.” Retain typed fields and mark them. Backend errors name the actual constraint (“Riley's home team is Office, so Riley cannot lead Checkout”; “Choose an accountable owner team”). Current-wire `owner_protected` remains a compatibility error, not a target rule forbidding an owner employee from having a team.

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
- JOIN-7 · *(rev 15)* Setup and `agentic-org service status` show an expired invitation, an invitation cancelled or replaced at HQ (3 wrong check codes included) and a declined join as ended, each with its §5c copy and `agentic-org office remove-pending <office id>` as the repair; HQ that can't be reached shows as the last error while the request stays waiting. No failure is reported as an active office.
- JOIN-8 · *(rev 12)* The check code identifies the same persisted invitation/request key across reconnects and setup restarts; a second setup cannot create a duplicate office.
- JOIN-9 · *(new, rev 15)* While a join waits, setup shows its status, the check code and the last error when there is one, and clears the error once a check works.
- JOIN-10 · *(new, rev 15)* `agentic-org office remove-pending <office id>` removes a join that ended without asking, and prints how to join again.
- JOIN-11 · *(new, rev 15; rewritten, review of 02316bb)* `agentic-org office remove-pending` on a join that's still waiting for approval says it stops the join and to remove the office at HQ if HQ approves it later, and removes it only after y in a terminal.
- JOIN-12 · *(new, rev 15)* `agentic-org office remove-pending` on an office that has joined removes nothing and points to Remove office at HQ.
- JOIN-13 · *(new, rev 15, A102)* When the server at HQ's address doesn't match the certificate authority the invitation pins, the join ends as `join_rejected` and setup makes no further attempt. Setup shows §5c's "couldn't prove it's <organization>'s HQ" row, or, while the status can't say which `join_rejected` it was (A105), the combined row; never the decline row alone.
- JOIN-14 · *(new, rev 15, A102)* When HQ doesn't support the joining instance's version, the join stays pending and setup shows "This instance needs an update to join this HQ. Update it, then setup continues." until the invitation expires.
- JOIN-15 · *(rev 18)* A join waits for verified affected turn/process drain and cleanup. Setup retains the existing waiting_for_idle tracking/count contract through an explicit target adapter and updates supported counts as blockers clear; Ctrl-C cancels the same join. Stale/unknown evidence cannot prove readiness, and no agent is automatically stopped or relaunched.
- JOIN-16 · *(new, rev 15, A101)* Ctrl-C while a join waits for running agents cancels the join: setup prints "Cancelled. <office> wasn't added to instance <name>…", and the instance has no office or pending join for it.
- JOIN-17 · *(new, rev 15)* Each row of §5c's `office remove-pending` table shows its copy and exit status and removes nothing; signed in as an account without the instance-admin grant it's refused with "<username> isn't an instance admin, so it can't remove join requests…".
- JOIN-18 · *(rev 18)* With an authorized instance-admin session, setup names only agents in organizations that account owns, with observed lifecycle state and supported Pause/safe-stop guidance. Other organizations and unauthenticated/noninteractive outputs show counts only. No run-mode labels, automatic-sleep or automatic-restart promises appear. Noninteractive waiting_for_idle output names pending verification, service status and the exact remove-pending command/office ID; status/doctor preserve supported counts and privacy.
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
- OFC-15 · *(rev 18)* Selected organization creation resolves name, explicit slug, remote organization repository/state branch and valid chosen bootstrap employee home teams before admission, with no browser folder picker. These may be separate setup steps; current name-only APIs require an explicit adapter. The note names the HQ instance/machine and creator ownership.
- OFC-16 · An empty or too long name, or one with control, invisible or direction-changing characters, shows its inline message and sends no request.
- OFC-17 · A name HQ refuses as already used shows "There's already an organization called <name> on this HQ. Choose another name." and no organization is created.
- OFC-18 · While creating, the button reads **Creating…** and is disabled; double-clicking sends one request.
- OFC-19 · A created organization opens showing Starting, with the toast "Created <name>. You own it.", and then loads normally. Its owner is the account that created it.
- OFC-20 · A confirmed failure shows its reason's copy, keeps the name, and offers Try again and Copy details. Copy details contains no key, token or recovery code.
- OFC-21 · When the create response is lost, the panel shows "Checking whether <name> was created…" and looks up the same attempt. Each lookup outcome shows its copy (including "still being created", which keeps checking), and repeating the check never creates a second organization.
- OFC-22 · HQ's refusal for a missing instance-admin grant shows "Only an instance admin can create organizations." with no Try again.
- OFC-23 · Closing the panel while creating doesn't stop it: the new organization then appears in the switcher with "HQ · Starting".
- OFC-24 · *(rev 18)* The form meets §5a's accessibility: labelled fields with linked rules/errors, `aria-invalid` on an invalid name, announced failures with focus on the alert, polite announcements of progress, focus on the new organization's heading, and focus returned to the switcher on close.
- OFC-25 · Add person and the person quick view offer no invite, password or sign-in option.

**Joins that end, waiting for running agents, and offices on this instance (OFC)** *(new, rev 15, §3.3, §5a, §6.2)*
- OFC-26 · In the joining instance's own view (its status and, if it has one, its local status page), a satellite whose join ended (`join_expired`, `join_cancelled`, `join_rejected`) shows its §3.3 copy and Remove…, which shows `agentic-org office remove-pending <office id>` with Copy command, and never offers Restart office, Try again or Start office. HQ's Offices list never shows this row: after 3 wrong check codes, a decline or an expired invitation it shows §6.3's row with New invitation and Remove.
- OFC-27 · On the joining instance's own status (CLI or local status page), when `office.restart` answers `unavailable` because the office's join ended, the office shows the join-ended row, with no restart action. HQ's Offices list is unaffected and keeps §6.3's rows.
- OFC-28 · *(rev 18)* Create organization waits for verified affected turns/processes to finish and cleanup to complete; a live idle process or unresolved cleanup blocks, and Paused/Offline/presence alone cannot clear it. Keep Cancel, authorized profile links and supported Pause controls. Show stop pending until confirmed cleanup; name only owned-organization agents, count others. Unknown evidence keeps waiting. No mode engine or automatic stop/restart is assumed.
- OFC-29 · *(rev 18)* Add office on this instance uses the same verified turn/process/drain boundary and controls/privacy/freshness as OFC-28, with spinner, retained read-only fields and Cancel. Existing waiting_for_idle tracking needs an explicit lifecycle adapter; do not infer readiness from a zero legacy count or an agent mode/status label.
- OFC-30 · Cancel while waiting for running agents creates nothing and keeps the name; once creating has started it shows "Too late to cancel…" and the create continues.
- OFC-31 · HQ's refusal of Add office › On this instance for an account without the grant shows "Only an instance admin can add an office on this instance. You can still invite another machine." with Invite another machine and no Try again.
- OFC-32 · *(rewritten in rev 15, A102)* In the joining instance's own view, a satellite whose join is pending (`join_pending`) shows Waiting for approval while its `join_phase` is `awaiting_approval`, then Starting · copying records while it's `initial_sync`, and never Online before its first copy of records completes. HQ's Offices list never shows Waiting for approval: it shows Wants to join before approval, then Starting · copying records until the first copy completes, and never Online before then, also for an office on HQ's own instance.
- OFC-33 · *(rev 18)* A warm idle process holding the affected layout context, open turn or unresolved cleanup keeps create/Add office/join waiting. Only verified quiescence clears the blocker; Paused, Offline or a deferred Asleep label is not proof.
- OFC-34 · *(rev 18)* Waiting counts update without reload as verified blockers clear. Existing waiting_for_idle.sessions is a legacy session count requiring target lifecycle mapping; unavailable counts/evidence show explicit waiting/verification copy rather than fabricated zero or readiness.
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
- OFC-46 · *(new, rev 16)* The visible restart countdown shows the remaining time rounded down to a 10 s step ("Restarting in 40 s" while 40 to 49 s remain) and updates on those 10 s boundaries, so it never overstates the time left; under 10 s it reads "Restarting shortly". For `in_use`, the same steps apply to "Trying again in N s", which reads "Trying again shortly" under 10 s. It's correct from its first frame, including right after a status push, and the screen-reader label doesn't tick.
- OFC-47 · *(new, rev 16)* The office details sheet has one Close control, and closing it returns focus to whatever opened it.
- OFC-48 · *(new, rev 16)* The Offices list's footer reads "Shown from HQ. Statuses include last-seen information."

**Org chart: Teams (TEAM)**
- TEAM-1 · Teams index uses indented divided rows for the recursive parent_team_id tree, showing TEAM- reference/name, optional direct lead, direct People/Agents counts and projects marked Owner or Contributor.
- TEAM-2 · Every employee is counted once at the required home team and listed once in Members. No multiple-membership badge or checklist is offered; name, handle and home-team appointment title remain distinct.
- TEAM-3 · *(rev 18)* Required home-team failures appear as validation or unresolved import repair, never an ordinary Not in a team tray. Owner and HR employee records obey the same constraint.
- TEAM-4 · Agent rows may show real executing office/Unplaced with freshness; office placement neither owns teams/projects nor changes directory membership. The earlier canvas-highlighting filter is retired with those canvases.
- TEAM-5 · *(rev 18)* New team supports optional parent and optional direct-member lead. Explicitly review home-team moves before appointment; explain subtree responsibility. Create a valid team before employee/bootstrap admission; preserve current-wire name validation only in its adapter.
- TEAM-6 · *(rev 18)* Move home team is one employee-revision-checked recoverable operation updating field/path and resolving departed leadership; preserve ID, handle, reporting, history and current task assignments. Stale state keeps input and explains the conflict.
- TEAM-7 · *(rev 18)* Team move works by drag and Move to…, excludes self/descendants, previews subtree effects and preserves identity. Undo is deferred; concurrent cycle/revision failure refreshes current state without applying the edit.
- TEAM-8 · *(rev 18)* Projects have a required single accountable owner and optional contributors, editable from project/team entry points. Owner removal requires a replacement; changing owner updates task accountability/history together and leaves assignments unchanged.
- TEAM-9 · *(rev 18)* Team deletion is blocked by child teams, home employees, owned projects/tasks and durable references, listing repair actions. Archival preserves historical identity. No cascade produces teamless employees or ownerless projects.
- TEAM-10 · Teams, Members and Org chart are fully usable from the keyboard, including view-switch heading focus, tree navigation/expand/collapse, menus, Move to… and Change manager…. Removing a row/control focuses a surviving neighbor or stable heading.
- TEAM-11 · Teams · Members · Org chart switch between the index, mixed directory and one vertical reporting tree. The old three canvases, View menu, Fit/zoom and chart search are removed; editing survives in index/directory/tree menus.
- TEAM-12 · Agents and people use initial avatars and explicit Agent/Person words, including the accessible name; kind is never conveyed by color, shape or robot glyph.
- TEAM-13 · A participating project appears in owner/contributor team rows with distinct words/badges and links to its accountable owner and other contributors; optional participation never creates additional owners.
- TEAM-14 · *(rev 18)* New projects cannot omit their owner team. Unresolved legacy ownerless records stay in explicit repair/staging rather than an ordinary Unassigned projects tray.
- TEAM-15 · *(rev 18)* The lead picker accepts only direct home-team employees and states subtree management, including child leads. Project cards identify accountable ownership without inferring project co-leads.

**Org chart: People (PEOPLE)**
- PEOPLE-1 · Members lists people and agents as divided rows, filtered All · People · Agents, with name/handle/role, kind in words, real presence when available and current task when known.
- PEOPLE-2 · The directory explains that agents can reach people registered in the organization, with Add a person; registration does not grant sign-in or authority.
- PEOPLE-3 · *(rev 18)* Add person requires name, independent handle and one valid home team, with optional contact and explicit employee manager. Team entry points prefill the home team; accepted people appear once there.
- PEOPLE-4 · *(rev 18)* Duplicate display names are allowed; handles are explicit lowercase identifiers unique across agents/people. Invalid or unavailable handles show inline errors/suggestions and block submission; current reserved routing/ID namespaces remain preserved by the adapter.
- PEOPLE-5 · *(rev 18)* Deactivation preserves ID, profile, required home team and handle; preview/resolve live leadership, reporting and assignments. History is retained; hard delete requires backend proof of no history/references. Assignment cleanup never erases accountability or automatically changes task status.
- PEOPLE-6 · People never show execution placement or invented runtime presence.
- PEOPLE-7 · When an agent's message or task fails because the recipient isn't in the organization (or was removed), the agent's activity says so in plain words, with Add a person… for an unknown recipient.
- PEOPLE-8 · *(rev 18)* Renaming a person does not change their handle. Explicit handle changes require separate uniqueness/compatibility validation and show the new address before acceptance.

**Org chart: Reporting lines (REPORT)**
- REPORT-1 · *(rev 18)* Reporting view draws explicit reports_to_employee_id; group absent managers under the owner presentation labelled No explicit manager. Every employee, including HR and linked owner employees, appears once without inventing a manager foreign key.
- REPORT-2 · *(rev 18)* Nodes show name/handle, Agent or Person shape/words, one Home team and explicit-report count; placement/freshness is agent-only.
- REPORT-3 · *(rev 18)* Change manager and drag offer eligible employees plus No explicit manager, omit self and disable reporting descendants with a reason. A security owner account is not fabricated as an employee reference.
- REPORT-4 · *(rev 18)* A changed manager keeps focus and shows a toast; conflict/cycle failures retain input and refresh current lines. Undo is deferred rather than a claimed automatic reverse mutation.
- REPORT-5 · *(rev 18)* The view distinguishes explicit reporting from subtree lead responsibility; changing either graph does not silently rewrite the other. Ordinary ancestor-team membership grants no management authority.

**The owner (OWNER)**
- OWNER-1 · *(rev 18)* The owner account bar presents organization access above each chart. A linked owner employee has a required home team and is counted/displayed there; a security account without a profile is not an employee.
- OWNER-2 · *(rev 18)* Owner employee home-team/lead eligibility follows ordinary employee constraints; profile edits cannot transfer account ownership or derive privileges from is_owner. No blanket teamless-owner exception appears.
- OWNER-3 · *(rev 18)* Messaging retains the authenticated owner routing contract. Task assignees and employee managers use eligible employee IDs, including a linked owner employee where valid; a sign-in account alone supplies neither reference.

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
- A105 · *(rev 15)* A decline at HQ and an HQ identity mismatch both end as `join_rejected` (A102). Setup and `agentic-org service status` need to tell them apart to show the right copy and repair (§5c): a decline asks for a new invitation, a mismatch asks to check HQ's address first. Does the instance's status say which one it was, so setup, `status` and the §3.3 row can say so? Until it does, they show one combined row for both (§3.3, §5c).
- A106 · *(rev 15, reviews of 02316bb and 0802754)* The wait for running agents names the agents it waits for in organizations the reader owns, and counts the rest by organization (§5a, §5c, §6.2). `waiting_for_idle { sessions: n }` gives only a count. Can a read made with a session (the create's and Add office's lookup, HQ's status feed, and the joining instance's status read by setup with an instance admin signed in) give each affected agent's employee ID, name, organization, active turn/process state, cleanup/drain evidence and observation freshness for the organizations that account owns, and a count for each other organization, so the browser can link and pause them and setup can name them? And can a read without a session (`agentic-org service status`, `agentic-org doctor`, setup on an instance with no accounts, noninteractive setup) give only a count for each organization, with no names? Until they do, the wait shows its total-only copy, without names, counts by organization or Pause.
- A89 and A93 in `server-installation-and-access-ux.md` §4a.9 also affect §5a and §5b: whether a failed recovery package fails the create, and the HQ machine's name in copy. *(rev 15)* Its A86 is resolved: the admin grant alone opens no organization.

**Historical resolutions in revision 2:** U1 (wording), U2 (separate reporting view), U3 (only recursive teams), U4 (old relationship choices superseded by the selected home-team/subtree-lead/project-owner model), A1 (§5.2 warning), A2 (stable code, §5.3), A3 (three wrong codes cancel, §6.3), A4 (unplaced agents, §6.4), A5 (Add office and approval on HQ's Mac, §6.1), A6 (8 Crockford base32 characters in two groups of four).

## 10. Out of scope here (follow-ups)

The placement flow itself (Place… opens the agent profile's flow, specced separately); hiring and offboarding (the HR flow); messaging and task screens beyond the PEOPLE-7 notice; promotion (phase 4, "Make this HQ…" reserved); the recovery-code screens, which are in `server-installation-and-access-ux.md` §4a.6 *(rev 14)*; the design-system glossary and the v1 spec revision 5, which fold this spec in.
