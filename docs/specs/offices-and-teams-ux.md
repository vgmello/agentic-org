---
kind: spec
title: "Offices, organizations and teams (UX)"
status: for-review
revision: 6
owner: ac-product-designer-eden
created: "2026-11-08"
updated: "2026-11-08"
requested_by: ac-staff-engineer-ada
reviewers: [ac-staff-engineer-ada, owner]
builds_on: []
---

# Spec · Offices, organizations and teams (UX)

Requested by ada for the user.

**Sources:** ada's `docs/design/offices-hq-and-satellites.md` (approved), `docs/design/teams-and-org-chart.md` (**approved by the user 2026-11-08**) and `docs/design/create-organization.md`. Look and feel: `prototype-agency` and Tailwind Plus application UI (dark sidebar, gray-900 ground, indigo-500 accent), as the app's onboarding screens already use.

**What it changes in my earlier work:** it replaces, for the office model, `desktop-app-v1.md` (in Eden's notes, not in this repository) §2.5 (workspace switcher), §3.2 (department and reporting-line chart) and §7.1 (Machines). Revision 5 of that spec folds this in. Acceptance-criterion IDs here are stable: changed criteria are rewritten under the same ID, retired IDs are never reused.

## Changelog

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

1. **One list, two places.** The menu-bar dropdown and the in-app organization switcher show the same thing: organizations first, each with its offices underneath. The menu bar only informs and switches; repairs happen in the app window.
2. **Switching organization never asks for confirmation.** Offices run under the login service, so nothing stops when you switch. The only prompt is for unsaved input ("Discard the new team?").
3. **The UI says "HQ" and "office", never "satellite".** HQ gets a badge; every other office is just an office. (`satellite` stays the engineering term.) The user is OK with this for now and may revisit it.
4. **"This Mac"** replaces "This computer" everywhere (macOS only, and the create screen already says it).
5. **The menu bar works without the window.** A join request waiting at HQ and agents that need you appear at the top of the menu, so the owner never has to open the app to notice them.
6. **Joining is two screens, then a wait:** paste → confirm (nothing is sent before Confirm) → waiting with the check code → copying data → done.
7. **The invitation link is shown once.** HQ keeps only the public half of the invitation key, so the app can't show the link again. The screen says so, and "New link" replaces it (cancelling the old one).
8. **The check code is never copyable on the joining Mac,** and the approve dialog at HQ says "only type a code you can see on that computer's screen yourself". That keeps the code doing its job: proving the owner sees the joining Mac. The code is stable (it comes from the invitation and the joining Mac's key), so it never changes under the owner's eyes.
9. **Check-code input is one text field**, not one box per character (screen readers, paste, password managers). Case, spaces and dashes are ignored.
10. **Org chart** is one nav item with three pill tabs: **Teams | People | Reporting lines**. Offices has its own nav item (bottom group, above Settings).
11. **There are only teams.** No guilds, no divisions, no labels. A team that owns teams simply shows "3 teams".
12. **Every edit has a non-drag path** (menu, dialog, keyboard). Drag-to-move and drag-to-change-manager are shortcuts, never the only way.
13. **Changes made on an office that can't reach HQ show as Pending** (dashed outline, "Pending" badge) and **Blocked** if HQ rejects an earlier one, with Retry or Discard. Nothing pretends to be saved before HQ commits it. This holds in all three chart views.
14. **Agents and people never look alike**, and the difference is never color only (§7.0).
15. **The owner is shown once, above the chart**, never inside it, except as the root of Reporting lines, where everyone ultimately reports to them.
16. **Reporting lines and teams are independent.** Leading a team doesn't make you anyone's manager, and the UI says so where people would assume it.

## 1. Words (proposed additions to the design-system glossary)

| Term | Means | Don't say |
| --- | --- | --- |
| Agentic Org | The app | agentic.org (in copy), Agency |
| Office | Where an organization's agents run and its data is kept. One organization has one or more. | Server, node, satellite, machine |
| HQ | The organization's first office. It keeps the records and approves new offices. | Primary, master, main server |
| This Mac | The Mac the app is running on | This computer, localhost |
| Invitation link | The one-time link that lets another computer join as an office | Token, enrollment, invite code |
| Check code | The short code a joining computer shows, typed at HQ to approve it | PIN, OTP, verification token |
| Team | A group of agents and people. A team can be part of another team. | Department, division, guild, squad |
| Member | An agent or a person in the organization | User, employee (in the UI) |
| Agent | An AI employee. It's placed in an office. | Bot, assistant, subagent |
| Person | A human added to the organization. Agents can reach them. | User, contact, human (in labels) |
| Owner | You: the organization's special user, with full access | Admin, root, super user |
| Lead | A team's optional lead, agent or person. Also leads the team's projects. | Manager, head |
| Reports to / Manager | Who someone reports to. Separate from teams. | Lead, boss |
| Shared | A project worked on by more than one team | Cross-team, joint |
| Unassigned | A project with no team, or an agent with no office ("Unplaced") | Orphan |

"Department" (from the role catalog) is not a grouping in the app, and there are no labels.

## 2. Visual direction (Tailwind Plus application UI)

| Need | Component |
| --- | --- |
| Menu-bar dropdown | **Native macOS menu** (tray menu), not a web popover: platform look, keyboard and VoiceOver for free |
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

### 3.1 Office status (one at a time, from the viewing Mac's point of view)

| Label | Icon (Lucide) | Color | When | Secondary text |
| --- | --- | --- | --- | --- |
| Online | `circle` filled | green | The office runs and is in sync with HQ (HQ itself: it runs) | — |
| Starting | `loader` | blue | An office on this Mac is starting | — |
| Offline | `circle` outline | gray | Another computer's office isn't connected to HQ | "last seen 2 h ago" |
| Unreachable | `wifi-off` | red | Seen from an office that can't reach HQ: the HQ row | "last synced 10 min ago" |
| Stopped | `circle-x` | red | An office on this Mac isn't running (crashed, failed to start) | the reason, in plain words |
| Update needed | `circle-arrow-up` | amber | The office's app is too old for the organization's level | "needs Agentic Org 1.4 or later" |
| Invited | `mail` | gray | An invitation link exists and hasn't been used | "expires in 21 h" |
| Wants to join | `bell-ring` | amber (needs you) | A computer used the link and waits for its check code at HQ | "from 192.168.1.44 · 2 min ago" |
| Waiting for approval | `hourglass` | amber | This Mac's own pending join | "type the code at HQ" |
| Unknown | `circle-help` | gray | The app has no live status for it (never shown as healthy) *(rev 6)* | "Status unknown" |

### 3.2 Organization availability (menu and Open organization screen)

An organization is **unavailable** only when the app can't read it on this Mac at all. It's greyed, with the reason and, in the window, a fix.

| Reason (exact copy) | Fix offered in the window |
| --- | --- |
| "Its office on this Mac stopped", followed by the reason's copy from §3.3 | The action for that reason from §3.3 (for example no Restart for `in_use`) · Copy details |
| "Offices aren't running. Allow Agentic Org in Login Items" (all organizations) | **Open Login Items settings** |
| "Needs a newer version of Agentic Org" | **Check for updates** |
| "Can't open its data", followed by §3.3's copy for `key_missing` or `storage_error` | The action from §3.3 (**Try again** for `key_missing`, **Restart office** for `storage_error`) · Copy details |
| "This Mac was removed from the organization" | **Delete its copy from this Mac** |
| "Waiting for approval at HQ" | **Show check code** · Cancel join |
| "Joining… copying data (40%)" | (progress only) |

**Not unavailable:** a Mac whose office can't reach HQ (it opens with a banner and queues changes, §6.5), and an office too old to sync (it opens read-only with "Update needed").

### 3.3 Why an office stopped (rev 6)

Shown as the Stopped row's secondary text and at the top of its details, with the fix as a button. The office's status updates live (`office.status_changed`), and a status that isn't live is never shown: the row reads "Status unknown" instead.

| `status_reason` | Copy (secondary text) | Action |
| --- | --- | --- |
| `crashed` | "It stopped unexpectedly. Restarting in 20 s" (from `next_restart_at`), or "It stopped unexpectedly" | **Restart now** · Copy details |
| `start_timeout` | "It didn't start in time" | **Restart office** · Copy details |
| `restarts_exhausted` | "It kept stopping, so it won't restart on its own" | **Restart office** · Copy details |
| `storage_error` | "It can't read or write its data on this Mac" (also used when the Keychain doesn't allow access, `office-host-boundary.md`) | **Restart office** · Reveal data folder · Copy details |
| `in_use` | "Another copy of this office is already running. Quit it, and this office starts again." | Copy details (it restarts by itself with backoff, so there's no Restart button) |
| `key_missing` | "Its data key is missing from this Mac's Keychain, so its data can't be opened" (the databases exist, the key doesn't) | **Try again** (`office.restart`, which asks the Keychain again and returns straight to this reason if the key is still missing) · Copy details. The button says Try again, never Restart. |
| `stopped_by_owner` | "You stopped it" | **Start office** (`office.restart`). Phase 1 has no stop call, so this appears only once one exists. |
| `unsupported_version` | "Needs a newer version of Agentic Org" | **Check for updates** |
| `invalid_request` | "It couldn't start because of an internal error" | Copy details (to send to support) |
| `unknown`, or anything else | "It stopped for a reason this version doesn't know" | **Restart office** · Copy details |

- **Restart office** calls `office.restart`. While it runs: "Restarting…" with a spinner, then the live status. `office.restart` covers Restart, Try again and Start office (ada). It refuses only `unsupported_version` ("Needs a newer version of Agentic Org" · **Check for updates**) and `invalid_request` (shown as an error with Copy details).
- **The host is still starting** (`office.restart` answers `not_ready`): the button shows "Starting…" and retries by itself for up to 30 s, then "The office host isn't ready yet" with **Try again**.
- **Copy details** copies the office name, id, status, reason, the time and the app version. It never includes keys or tokens.

## 4. Flow 1 · Menu-bar dropdown

The app lives in the menu bar (ADR-0001). The icon is a monochrome template image. It gains a small dot when anything needs the owner (a join request, an agent that needs you, unplaced agents), so state is never color-only: the menu's first rows say what it is.

```
┌────────────────────────────────────────────────────┐
│ Studio Mac wants to join Lumen Labs…               │  ← needs-you rows, only when present
│ 2 agents need you in Lumen Labs…                   │
├────────────────────────────────────────────────────┤
│ ✓ Lumen Labs                                   ⌘1  │  ← current organization (checkmark)
│      HQ · This Mac                  ● Online       │
│      Studio Mac                     ● Online       │
│      Build box                      ○ Offline · 2h │
│   Side Project                                 ⌘2  │
│      Office · This Mac              ● Online       │
│      HQ · hq-mac.local              ⚠ Unreachable  │
│   Old Org                                          │  ← disabled (greyed)
│      Needs a newer version of Agentic Org          │  ← disabled reason line
├────────────────────────────────────────────────────┤
│ Open Agentic Org                               ⌘O  │
│ Open or join organization…                         │
│ Create organization…                               │
├────────────────────────────────────────────────────┤
│ Quit Agentic Org                               ⌘Q  │
└────────────────────────────────────────────────────┘
```

**Rules**

- Organizations are sorted A–Z (stable, for muscle memory); the current one has a checkmark. ⌘1–⌘9 follow that order.
- Offices are indented under their organization: HQ first, then this Mac's other offices, then the rest A–Z. Each shows "· This Mac" when it runs here. More than 5 offices: the first 5, then "4 more offices…".
- **Click an organization** → the window opens (or comes forward) on it. No confirmation (decision 2).
- **Click an office** → the window opens that organization's Offices screen with that office's details open.
- **Click a needs-you row** → the window opens on the item (the approve dialog, or the Needs-you inbox).
- An unavailable organization is disabled with its reason on the line below; its offices aren't listed. The fix is in **Open or join organization…** (decision 1).
- **No organizations yet:** the list area shows one disabled line, "No organizations on this Mac yet", above Create and Open.
- **Host not running** (the login service is off): one disabled line at the top, "Offices aren't running", and "Open Login Items settings…" below it.
- **Quit Agentic Org:** the first time, a one-time notice in the window: "Your offices and agents keep running after you quit. Open Agentic Org from the menu bar to see them."

**In-app switcher** (sidebar footer): organization initials tile · name · current office ("HQ · This Mac") · chevron. It opens a Tailwind dropdown with the same list as the menu, plus **Manage offices** and **Open or join organization…**. Keyboard: ↑/↓, Enter, Esc.

## 5. Flow 2 · Open organization and join

**When it shows:** first launch; when the last-opened organization is unavailable; from "Open or join organization…". Otherwise the app opens straight into the last organization.

### 5.1 Open organization

```
 Open organization
 Pick an organization on this Mac, or join one with an invitation link.

 ON THIS MAC
 ┌──────────────────────────────────────────────────────────────────────────┐
 │ [LL] Lumen Labs               HQ on this Mac          ● Online    Open › │
 │      4 agents · 3 offices · opened today                                 │
 ├──────────────────────────────────────────────────────────────────────────┤
 │ [SP] Side Project             Office · HQ at hq-mac   ⚠ Unreachable Open › │
 │      Changes wait on this Mac until HQ is back                           │
 ├──────────────────────────────────────────────────────────────────────────┤
 │ [OO] Old Org (greyed)   Needs a newer version of Agentic Org  [Check for updates] │
 └──────────────────────────────────────────────────────────────────────────┘

 JOIN AN ORGANIZATION
 Paste the invitation link from HQ (Offices › Add office › Another computer).
 ┌──────────────────────────────────────────────────┐ ┌───────┐ ┌──────────┐
 │ agenticorg://join#…                              │ │ Paste │ │ Continue │
 └──────────────────────────────────────────────────┘ └───────┘ └──────────┘

 [+ Create organization]
```

- Rows are a stacked list; the whole row opens the organization. Unavailable rows show their fix button instead of "Open".
- **Empty (first launch):** two cards, as built today, **Create organization** and **Open organization** (ADR-0001, `docs/design/create-organization.md`), both enabled. Open organization lists the organizations on this Mac and offers **Join organization** ("Join an organization you already run, with an invitation link from its HQ"), which opens the paste field (the join flow, phase 3).
- The app **never reads the clipboard on its own.** "Paste" (and ⌘V in the field) does.
- Opening an `agenticorg://join#…` link brings the window forward on this screen with the field filled and goes straight to Confirm (§5.2). It never joins by itself.

**Paste validation** (local, instant, nothing is sent):

| Case | Inline message under the field |
| --- | --- |
| Not a link or token | "This isn't an Agentic Org invitation link. It starts with agenticorg://join#." |
| Damaged (cut off, edited) | "This link is incomplete. Copy it again from HQ." |
| Expired | "This invitation expired at 14:02 today. Ask HQ for a new link." |
| From a newer app | "This link needs a newer version of Agentic Org." **Check for updates** |
| For an organization this Mac already belongs to (same id) | "This Mac is already in Lumen Labs." **Open Lumen Labs** |

### 5.2 Confirm (nothing has been sent yet)

```
 ← Back
 Join Lumen Labs?

 ┌ Organization ─────────────────────────────────────────────┐
 │ Lumen Labs                                id …7K2Q9F      │
 │ This Mac joins as   Studio Mac                            │
 │ HQ                  HQ · hq-mac.local:7443, 192.168.1.20  │
 │ Invitation          expires in 23 h                       │
 └───────────────────────────────────────────────────────────┘

 ┃ ⚠ Joining lets Lumen Labs run its agents on this Mac.
 ┃   In this version, agents run as you: they can read and change your
 ┃   files, use your apps and network, and act on your behalf. This Mac
 ┃   keeps an encrypted copy of the organization's records and stays in
 ┃   sync with HQ. Only join organizations you run yourself.

 ┃ ⚠ You already have a different organization called Lumen Labs    ← only on a name match
 ┃   on this Mac (id …A3M8, HQ on this Mac, created 7 Nov). This link is for
 ┃   another one (id …7K2Q9F). If you meant to add an office to yours,
 ┃   don't continue: create the link from yours.
 ┃   ☐ I understand this is a different organization

                                     [Cancel]  [Join Lumen Labs]
```

- The organization's name is the title. The id's last 6 characters show beside it, and in the name-match warning both ids are shown, so the two are tellable apart.
- The office name comes from the link and isn't editable here (rename later in Offices).
- **Name match:** the primary button stays disabled until the checkbox is ticked. Matching ignores case and surrounding spaces.
- The first warning always shows. Its wording is ada's (A1, resolved): agents run as the user until v2. The copy says "In this version" rather than naming v2, so it stays true without promising a date; when v2 changes it, this warning changes with it.

### 5.3 Waiting for approval

```
 Joining Lumen Labs
 ✓ Connected to HQ at hq-mac.local
 ✓ Checked HQ's identity
 ◌ Waiting for approval at HQ

 ┌───────────────────────────────────┐
 │      Check code                   │
 │        K7QP  4MZT                 │   (mono, 32 px, letter-spaced, not selectable)
 └───────────────────────────────────┘
 On HQ's Mac, open Agentic Org. It shows "Studio Mac wants to join".
 Type this code there to approve.
 This Mac: Studio Mac · 192.168.1.44  (HQ shows the same address)

 The invitation expires in 23 h.               [Cancel join]
```

- **The code:** 8 characters in Crockford base32 (digits and uppercase letters without I, L, O and U), shown in two groups of four. It isn't selectable or copyable (decision 8). VoiceOver reads it character by character ("K, 7, Q, P, 4, M, Z, T").
- **The code is stable:** it comes from the invitation and this Mac's key, so a reconnect, a closed window or a restart shows the same code. There is no "code changed" state.
- The window can be closed. The organization then shows in the menu and Open screen as "Waiting for approval at HQ", with **Show check code**.

**Then:** "Approved. Copying Lumen Labs' records… 12 of 40 MB" (a progress bar) → "Studio Mac is now an office of Lumen Labs." **Open Lumen Labs**.

**Errors** (each replaces the step it failed at, with the next action):

| Case | Copy | Actions |
| --- | --- | --- |
| HQ unreachable at every address | "Can't reach HQ at hq-mac.local:7443 or 192.168.1.20. HQ's Mac must be awake and on a network this Mac can reach." | **Try again** · **Enter HQ's address…** (safe: HQ is checked by identity, not address) |
| HQ's identity doesn't match (red) | "The computer at hq-mac.local isn't Lumen Labs' HQ. Nothing was sent. Check the address with HQ before trying again." | Try again · Cancel |
| Declined at HQ | "HQ declined this join, and the invitation is cancelled." | Close |
| Three wrong codes at HQ | "HQ cancelled this invitation after 3 wrong codes. Ask HQ for a new link." | Close |
| Invitation used, cancelled or expired (HQ says) | "This invitation was already used." / "…was cancelled at HQ." / "…expired." + "Ask HQ for a new link." | Close |
| Too many attempts | "Too many attempts from this Mac. Try again in 5 minutes." | (countdown) |
| HQ needs a newer app here | "Lumen Labs' HQ runs a newer version. Update Agentic Org to join." | **Check for updates** |
| Copy interrupted | "Copying stopped at 60%. It continues when HQ is reachable." | Try now |

## 6. Flow 3 · Offices and Add office

### 6.1 Offices screen (nav: Offices)

```
 Offices                                                        [+ Add office]
 Where Lumen Labs' agents run. HQ keeps the records and approves new offices;
 every other office keeps a copy.

 ┌───────────────────────────────────────────────────────────────────────────────┐
 │ ! Laptop wants to join · from 192.168.1.44 · 2 min ago      [Review and approve] │  ← highlighted row
 ├───────────────────────────────────────────────────────────────────────────────┤
 │ [HQ] Main office        This Mac (you're here)       ● Online     4 agents   ⋯ │
 │      Studio Mac         from 192.168.1.31            ● Online     2 agents   ⋯ │
 │      Build box          last seen 2 h ago            ○ Offline    1 agent    ⋯ │
 │      Office 2           This Mac                     ● Online     0 agents   ⋯ │
 │      Laptop             invitation · expires in 21 h ✉ Invited     [Cancel]  ⋯ │
 ├───────────────────────────────────────────────────────────────────────────────┤
 │ ! 2 unplaced agents · from a removed office                   [Place or offboard] │  ← only when any
 └───────────────────────────────────────────────────────────────────────────────┘
 Shown from this Mac. Statuses update live.
```

- Order: join requests, HQ, this Mac's offices, other offices A–Z, invitations, then unplaced agents.
- **A row opens a slide-over** with a description list: name (**Rename**), role (HQ / Office), where (This Mac, or where it last connected from), status, last seen, sync ("Up to date" or "Catching up: 34 changes"), Agentic Org version, agents placed here, projects it follows. Actions: **Rename** · **Remove office…** (not on HQ) · later, **Make this HQ…** (phase 4, hidden until then).
- **Version holding back a feature:** when HQ can't raise the organization level, a quiet note above the list: "Build box runs an older version of Agentic Org, so new features wait until it updates." It links to the row.
- **Empty is impossible** (HQ always exists). With HQ alone, a one-line hint under the list: "Add an office to run agents on another Mac, or to keep a copy of the records there."
- **Loading:** skeleton rows. **Live:** statuses update in place; a row that changes status doesn't move.
- **Row label for screen readers** *(rev 6, OFC-13)*: "Main office, HQ, This Mac, Online, 6 agents. Opens details." (with the reason for Stopped: "…, Stopped: it stopped unexpectedly, restarting in 20 s, …").
- **Phase 1** *(rev 6, from the build)*: **Add office** is shown disabled with "Available in a later release." While the data is invented, a **Sample data** badge says so. A status the app can't confirm shows as **Unknown** (§3.1), never as healthy.
- **HQ-only actions** (A5, resolved): **Add office**, **Review and approve** and **Remove office…** happen in the app on HQ's Mac. On any other office, Add office is disabled with "Add offices from HQ's Mac (Main office)", and a join request shows read-only with "Approve on HQ's Mac (Main office)".

### 6.2 Add office (slide-over, HQ's Mac only)

**Step 1 · Where**

```
 Add office
 Name     [ Studio Mac                ]
 Where will it run?
 (•) Another computer
     You'll get a one-time link to open on that computer. You approve it here.
 ( ) This Mac
     A separate office on this Mac, with its own copy and agents. Ready in seconds.
                                                    [Cancel]  [Continue]
```

- Name: required, 1–64 characters, unique among the organization's offices ("There's already an office called Studio Mac.").
- **This Mac** → "Creating Office 2…" (inline steps: Creating · Copying records · Starting) → the slide-over closes, the row appears Online and is briefly highlighted. Failure: "Office 2 couldn't start: <reason>." **Try again** · Copy details. The registry entry stays as Stopped, so nothing half-made is hidden.

**Step 2 · Another computer: the link**

```
 Invite Studio Mac
 Link expires in  [ 24 hours ▾ ]   (1 hour · 4 hours · 24 hours)
                                         [Create invitation link]
 ─────────────────────────────────────────────
 ┌──────────────────────────────────────────────┐
 │ agenticorg://join#AQEBgK3…f9Q                 │ [Copy link]
 └──────────────────────────────────────────────┘
 ┃ ⚠ This link is shown only once. Treat it like a password: send it
 ┃   privately (AirDrop, a direct message), not in a shared channel.
 ┃   Anyone who has it can ask to join until it's used, cancelled or expires.

 Next, on Studio Mac:
 1. Install Agentic Org.
 2. Choose Join organization and paste the link.
 3. Studio Mac shows a check code. You'll type it here to approve.
                                                    [Done]
```

- **Copy link** → the button reads "Copied" for 2 s. The link field is read-only and fully selectable.
- **Done or closing** → the row shows **Invited · expires in 24 h**. Its menu has **Cancel invitation** and **New link** ("New link cancels the current one. Continue?").
- Expiry → the row reads "Invitation expired" with **New link** and **Remove**.

### 6.3 Approve a join (on HQ's Mac only)

Triggers: a native notification ("Studio Mac wants to join Lumen Labs. Type its check code to approve."), the menu's needs-you row, the Needs-you inbox, and the highlighted Offices row. Every one opens this dialog.

```
 ┌──────────────────────────────────────────────────────────┐
 │ (!) Approve Studio Mac?                                   │
 │ A computer at 192.168.1.44 used the invitation for        │
 │ Studio Mac (created today at 14:02) 2 minutes ago.        │
 │                                                           │
 │ Type the check code shown on Studio Mac                   │
 │ [ K7QP 4MZT                 ]                             │
 │ Only type a code you can see on that computer's screen    │
 │ yourself. If someone sent you a code, decline.            │
 │                                                           │
 │                       [Decline]   [Approve Studio Mac]    │
 └──────────────────────────────────────────────────────────┘
```

- **Approve** is enabled once the code has 8 characters. Case, spaces and dashes are ignored. As in Crockford base32, a typed O counts as 0 and I or L as 1, so look-alikes never cause a mismatch. A U, or any other character that can't be in a code, is rejected as typed with the hint "Codes use digits and letters, without U".
- **Wrong code:** "That code doesn't match Studio Mac's. Check its screen. 2 tries left." After 3 wrong codes the invitation is cancelled (A3, resolved): "Join declined after 3 wrong codes. The invitation is cancelled; create a new link to try again." The row becomes **Invitation cancelled** with **New link** and **Remove**.
- **Decline** asks once: "Decline Studio Mac? Its invitation is cancelled." · **Decline**.
- **Approved:** the row moves to the office list as "Starting · copying records", then Online. Toast: "Studio Mac joined Lumen Labs."
- **Two requests on one link** (shouldn't happen, but HQ may see a retry from a new address): both are listed. Approving one declines the rest.

### 6.4 Remove office (on HQ's Mac only)

Modal, red icon. "Remove Studio Mac from Lumen Labs? It can't sync again. Its 2 agents stop running there and become unplaced: you choose a new office for each, or offboard them. HQ takes back their 5 tasks. If Studio Mac is offline, it keeps its copy until it next connects, then deletes it. To add it again, send a new invitation." Primary: **Remove Studio Mac** (red).

**After removal** (A4, resolved): the agents are **Unplaced**. They keep their teams, reporting lines and history, but don't run. They show:
- as a needs-you item and menu row: "2 agents are unplaced in Lumen Labs…", until each is placed or offboarded;
- in the Offices list's last row, **Place or offboard**, which opens a list of them, each with **Place…** (office picker, the agent profile's placement flow) and **Offboard…** (the HR flow);
- in the org chart with an **Unplaced** tag, and under **Office: Unplaced** in the office filter.

### 6.5 Working from an office that can't reach HQ

- Banner (yellow) on every screen: "Can't reach HQ (Main office) · last synced 10 min ago. You can keep working. Changes to the organization wait on this Mac and are sent when HQ is back. **3 waiting**". "3 waiting" opens **Changes waiting for HQ** (a slide-over list: what, when, status).
- Pending changes show on their objects with a dashed outline and a **Pending** badge. They aren't in the copy yet.
- **Blocked** (HQ rejected one, so later ones are held): the banner turns red: "HQ rejected a change: Move Checkout under Growth (Growth is under Checkout now). 2 later changes are held." Each blocked change has **Retry** (as a new change) and **Discard**.

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
 │ Checkout               ⋯ │ │ Growth                 ⋯ │   │ [H] HR            │
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
 │ [A] ada · Agent ⋯ │ │ (S) Sam Ortiz · Person⋯│ │ [H] HR · Agent  ⋯ │
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
- MENU-1 · The dropdown lists organizations A–Z, each followed by its offices, indented, with HQ first and a status label on every office.
- MENU-2 · The current organization has a checkmark. Clicking another organization switches the window to it without a confirmation.
- MENU-3 · An unavailable organization is disabled, its reason shows on the next line, and its offices aren't listed.
- MENU-4 · A pending join request and agents that need you appear as the first rows, and the menu-bar icon shows a dot while any exist.
- MENU-5 · Clicking an office opens that organization's Offices screen with the office's details open.
- MENU-6 · The in-app switcher lists the same organizations, offices, statuses and reasons as the menu.

**Open organization and join (OPEN, JOIN)**
- OPEN-1 · On launch, the app opens the last-opened organization, or this screen if it's unavailable or there is none.
- OPEN-2 · Each organization on this Mac shows its offices on this Mac and their status; an unavailable one shows its reason and its fix action (§3.2).
- OPEN-3 · The app never reads the clipboard without Paste or ⌘V.
- JOIN-1 · A pasted or opened link is checked locally; each invalid case shows its message from §5.1, and nothing is sent.
- JOIN-2 · Nothing is sent to HQ before **Join <organization>** on the confirm screen. Opening a link only fills the screen.
- JOIN-3 · *(rev 2)* The confirm screen shows the organization's name and short id, this office's name, HQ's addresses, the expiry and the warning that agents run as you and can read and change your files, use your apps and network, and act on your behalf.
- JOIN-4 · If the name matches an organization on this Mac with another id, the warning shows both ids, and Join stays disabled until the checkbox is ticked.
- JOIN-5 · While waiting, the check code shows in groups of four, can't be selected or copied, and is read character by character by VoiceOver.
- JOIN-6 · Closing the window while waiting keeps the join; the organization shows "Waiting for approval at HQ" with **Show check code**.
- JOIN-7 · Every error in §5.3 shows its copy and actions; an identity mismatch says nothing was sent.
- JOIN-8 · *(new, rev 2)* The check code is 8 Crockford base32 characters, and the same code shows after a reconnect, a closed window or a restart.

**Offices (OFC)**
- OFC-1 · The Offices screen lists join requests, HQ, this Mac's offices, other offices and invitations, in that order, with status and secondary text from §3.1.
- OFC-2 · Add office › This Mac creates and starts the office with no link or approval; a failure leaves the row as Stopped with its reason.
- OFC-3 · Add office › Another computer shows the link once, with Copy link and the "shown only once" warning; closing leaves an **Invited** row with Cancel invitation and New link.
- OFC-4 · New link cancels the previous link after a confirmation.
- OFC-5 · A join request raises a native notification, a menu row, a Needs-you item and a highlighted row, and each opens the approve dialog.
- OFC-6 · *(rev 2)* Approve is enabled only when the typed code has 8 characters; case, spaces and dashes are ignored, and O, I and L are read as 0, 1 and 1; a wrong code shows the mismatch message with the tries left, and the third wrong code cancels the invitation.
- OFC-7 · *(rev 2)* Add office, the approve dialog and Remove office are available only in the app on HQ's Mac; other offices show Add office disabled with the reason and the request read-only with "Approve on HQ's Mac".
- OFC-8 · *(rev 2)* Remove office names the office, its agents (which become unplaced), their tasks and the offline case before removing.
- OFC-9 · On an office that can't reach HQ, the banner shows last sync and the waiting count; changes show Pending; a rejection shows Blocked with Retry and Discard; HQ-only actions are disabled with the reason.
- OFC-10 · *(new, rev 2)* After an office is removed, its agents show as Unplaced (Offices list, org chart tag, office filter), with Place… and Offboard… for each, and a needs-you item until every one is handled.
- OFC-11 · *(new, rev 6)* A Stopped office shows its reason in plain words (§3.3) as secondary text and in its details, with the action for that reason, and Restart is never offered for `in_use` (for `key_missing` the action is Try again).
- OFC-12 · *(new, rev 6)* Office status updates live from the host. When the app has no live status, the row says "Status unknown" and never shows a stale Online.
- OFC-13 · *(new, rev 6)* Each status has its icon and word from §3.1, and a row's accessible label includes the name, HQ, where it runs, the status (and reason) and the agent count, and says it opens details.

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

**Retired:** GUILD-1, GUILD-2 (rev 2: there are no guilds). Not reused.

## 9. Open questions

**For the user**
- U5 · *My recommendation:* keep **Lumen Labs** for designs and mock data. It's neutral and can't be confused with the app or with our own organization.
- U6 · *My recommendation:* keep "HQ" and "office". They're short and plain, and the layout doesn't depend on them.

**For ada**
- ~~A38~~ · *Resolved (ada):* no rename. "Open organization" stays for an organization already on this Mac, and "Join organization" is reserved for the invitation link (phase 3).
- A7 · Remove office: I've made it HQ-only, like Add office and approval. Confirm, or should it work from any office while HQ is reachable?
- A8 · Is there a limit on how many people an organization can add, or on a person's name length? I've assumed the team-name rules (1–64 characters).
- A9 · Does the agent's `delivery_failed` notice carry the name the agent used, so PEOPLE-7 can say "Couldn't reach "Sam"" and prefill Add person?

**Resolved in revision 2:** U1 (wording kept for now), U2 (reporting lines stay as their own view), U3 (no departments, no labels), U4 (one optional lead, agent or person; owner in no team; shared projects allowed), A1 (§5.2 warning), A2 (stable code, §5.3), A3 (three wrong codes cancel, §6.3), A4 (unplaced agents, §6.4), A5 (Add office and approval on HQ's Mac, §6.1), A6 (8 Crockford base32 characters in two groups of four).

## 10. Out of scope here (follow-ups)

The placement flow itself (Place… opens the agent profile's flow, specced separately); hiring and offboarding (the HR flow); messaging and task screens beyond the PEOPLE-7 notice; promotion (phase 4, "Make this HQ…" reserved); the recovery-code screens (phase 3); the design-system glossary and the v1 spec revision 5, which fold this spec in.
