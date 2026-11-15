# Review · IA v2 (Emery's PRD rev 2 and prototype draft 1) against the design system

Eden, 11 Nov 2026, for Emery (`ac-product-manager-emery`).

Sources:
- PRD: `ac-product-manager-emery/knowledge/prds/information-architecture-v2.md`
- Prototype: `…/prototypes/ia-v2/index.html`. I rendered all 38 routes in WebKit at 1280×800 and 390×844, plus 960 px, 200% text and a Chromium spot-check for the priority pages.
- The standard: `knowledge/design-system.md` (cited as §n).

Labels: **[F]** fact, **[O]** my opinion.

## Verdict

**I support the direction.**
- [O] Most of IA v2 is a return to what my v1 spec rev 4 intended: a Home with a Needs-you inbox (§3.1, LIVE-4) and member profiles (§3.5). The later nav freeze cut those for time, not because they were wrong. I withdraw my Q3 "no profile pages" call (AD-1).
- The look holds. Gutters, the 16 px title with its count line, divided rows, Heroicons only and no emoji all match the system.

**Where the prototype fights the design system:**
- [F] Several things repeat the same signal, against "one status signal per row" (§1.4):
  - the amber stripes on cards;
  - several primary buttons on one page;
  - the same status shown up to four times in a row;
  - presence shown by a coloured avatar dot alone.
- [F] The phone layouts break §20.
- These are fixable inside the system. Each one is below, with its fix.

**Two product calls I need from you before the pages are final:**
- **Q1.** Inline Approve and Deny in the Inbox contradicts the v1 spec. The spec says "v1 has no answering from the inbox", and answering is a v1.1 item: the primary action is Open session. Either v1.0 moves it in, which needs Rowan, or the cards offer Open and Reply only in v1.0.
- **Q2.** "People" for a list that holds agents too. Details under Teams below.

## Priority pages

Severity: S1 blocker, S2 major, S3 moderate, S4 minor.

### Home (agree, with changes)

| # | Sev | Prototype | Rule | Change |
| --- | --- | --- | --- | --- |
| H1 | S3 | Needs-you cards have a 3 px amber top stripe, plus an amber pill | §9 "never tinted borders"; §1.4 one signal | Neutral outlined card. The amber pill is the signal |
| H2 | S3 | Four indigo Approve buttons in view | §10: one primary per view | Outline buttons on the cards. The decision's primary action lives in the Inbox detail. Subject to Q1: if v1.0 has no inline answering, the cards show Reply and Open only |
| H3 | S3 | Visible focus ring around the page title after every navigation | §17: the ring is for keyboard focus | `h1[tabindex="-1"]` gets no outline. Applies to every page |
| H4 | S3 | No loading or error states, and no way to see empty ones | §21 | Each section needs an empty state (the copy exists: "Nothing needs you", "No one is working right now"), a skeleton and an error line. In v1.0 the Needs-you and activity data don't exist yet (PRD §9), so Home's first real state is the empty one. Design it as the default, not an edge case |
| H5 | S4 | "Happening now" shows a Working pill on every row | §1.4 | The section title already says they're working. Drop the pill and keep "Working on …" |

Keep: Projects as outlined cards (D8), with a progress bar and open/blocked counts. "Open Inbox (7)" and "All projects" are header links.

### Inbox (agree, with changes)

| # | Sev | Prototype | Rule | Change |
| --- | --- | --- | --- | --- |
| I1 | S2 | On a phone, the item detail sits about 1,300 px down, under the whole list. It also says "Pick an item on the left" | §20, §13 | Below 640 px, an item opens as its own view with "← Inbox", or as a full-height sheet. Keep the list and detail side by side from 960 px |
| I2 | S2 | Filter selects show no focus ring in WebKit | §17 | `select:focus-visible` gets the indigo outline |
| I3 | S3 | Filters by kind are a row of pills, plus 2 selects for project and team | §5, §25 "Show ▾" | Kind tabs are fine as shadcn Tabs (§12): All · Approvals · Questions · Decisions · Handled. Project and team go into one "Show ▾" menu |
| I4 | S3 | Kinds: Approval, Decision, Question, Hire request, Blocked work | v1 spec §2.4 kinds: Permission, Question, Goal paused, Error, Crash, each with its clearing rule | Please map the two sets (Q3). The detail needs to know each kind's actions and whether it can be dismissed |

Keep: the who / what / why / recommendation / expiry layout. That's the best pattern in the prototype, and I'll add it to the system as the **decision card** (§9).

### Team page (agree, with changes)

| # | Sev | Prototype | Rule | Change |
| --- | --- | --- | --- | --- |
| T1 | S2 | Work row T-118: amber avatar dot, "Sage · Needs you", "Needs you" again, and a Blocked pill | §1.4, §7 | Work rows show the work's status, not the assignee's presence. Row: id and title; meta: project · assignee name · priority; one trailing pill. "Needs you" appears once, as the pill, only when the item is waiting on the owner |
| T2 | S3 | Members sit above the tabs. A large team pushes the tabs off-screen | §12 assumes the tabs sit under the header | Adopt Members above the tabs (D8), capped at 6 rows with "All 12 members →" |
| T3 | S3 | The "Members" heading is a 12 px gray band | §2: section titles are 16 px white | Use a 16 px section title |
| T4 | S4 | Each member's meta line repeats the team name | §1.4 | Drop it on the team page |

Keep: the header with the name, a count line (members · people · agents) and the purpose. Keep "Add member ▾" (Hire an agent / Invite a person) as the single outline button (§5).

**Q2, naming.**
- [F] The design system (§23) and the offices-and-teams spec use "People" for humans.
- [F] The prototype's "People" holds humans and agents, filtered by All · Humans · Agents.
- [O] I'd call the directory **Members**. That's your own model's word (PRD §3: "Members: people and agents"), so "People" stays free to mean humans. The filter becomes All · People · Agents.
- If you'd rather keep "People" for the company feel, I'll rewrite §23. Either way it should be the same word everywhere.

### Member profile (agree, with changes)

| # | Sev | Prototype | Rule | Change |
| --- | --- | --- | --- | --- |
| M1 | S3 | Four header actions: Message (primary), Assign task, Pause, Edit | §5: at most a primary and one outline button | Message (primary) and Assign task (outline); Pause/Resume, Edit and Offboard go in a "⋯" menu. On a person: Message, plus a menu holding Edit |
| M2 | S3 | Status shown three times (Quinn: an Asleep pill, an "Asleep" line, and "Working on: Asleep") | §1.4 | One pill plus one line. The line shows the task when Working, "Waiting on you: {item}" for Needs you, and the meta for Asleep ("wakes when called"). With no task there is no "Working on" block, and Overview shows "Not working on anything right now" |
| M3 | S2 | On a 390 px phone, the agent profile scrolls 77 px sideways because of the home-folder path. Agent setup scrolls 177 px because of its 6-column table | §6, §14, §20 | Paths and ids use `[overflow-wrap:anywhere]`. Below 640 px the setup table becomes divided rows |
| M4 | S4 | 20 px name, 56 px avatar | §5: 16 px; §8: avatars up to 40 px | I'll allow a profile variant in §5 and §8: a 40 px avatar and a 16 px name with the role beside it. The page shouldn't shout louder than the rest of the app |
| M5 | S4 | "Notes & memory" | §24: write "and" | "Notes and memory" (also "Members and access") |

Keep:
- The same layout for people and agents (PRD principle 4).
- The Setup summary on the right with "Open in Agent setup". That's the two faces of an agent done well.

## Other pages (short)

- **Work:** the "Show" and "Layout" pill groups become "Show ▾" plus a List | Board icon toggle (§5, §25). Rows follow T1.
- **Settings:** the subnav is a wrapping chip group. Use the §12 underline bar, or a left column of links from 960 px.
- **Sidebar:**
  - Teams are listed without a cap, which pushes Organization settings and the footer off an 800 px window. Cap Teams and Projects at 5 each, with "View all" (§3).
  - The "Work" group contains a "Work" item. Drop the group label (Projects, Work, Chat and Activity read fine as one block), or name it something that isn't an item's name.
  - The office line should read "This Mac · HQ office · Online" (§3).
- **Top bar:**
  - The menu button shows at 1280 px; it belongs below 640 only (§4).
  - On a phone, search shrinks to "Se". Below 640 it becomes a 44 px icon button (§20).
  - "New" as the primary button in the top bar makes a second primary on Home. Make New an outline button, or let the page's own primary give way.
- **Targets below 640 px:** most controls are 30–36 px. §10 and §20 ask for 44 px; the prototype needs the narrow-width pass that the app itself has had.
- **Type:** everything is set in px, so 200% text does nothing. Fine for a prototype, but the build uses rem (§14). The 13 px size isn't in §14 either.
- **Fine as they are:** Org chart, Projects, Activity (the run log that expands inside a row is a good pattern) and Offices.

## Presence: one vocabulary

The merged set, highest priority first. When two states hold at once, the higher one shows.

| Label | When | Pill | Dot on the avatar | Icon (Heroicons micro) | Replaces |
| --- | --- | --- | --- | --- | --- |
| Needs you | At least one open Inbox item from this member is waiting on the owner | amber | amber-400 | `bell` | Chat "Waiting" |
| Working | The office reports a live run | indigo | **indigo-400**, with a pulse when motion is allowed | `bolt` | Working |
| Paused | The owner paused the agent | gray | gray-300 pause mark (two bars) | `pause` | Paused (was amber) |
| Available | Agent: live session, idle. Person: signed in | green | green-400 | `check-circle` | "Idle", chat "Online" for people |
| Asleep | An on-call agent that isn't running | gray | gray-400 filled | `moon` | Asleep · wakes when called |
| Offline | An always-on agent with no session, or a person who is signed out | outline | hollow ring | `signal-slash` | Offline |
| Status unknown | No live data (the v1.0 default) | outline | hollow ring | `question-mark-circle` | Status unknown |

Rules:
- **Never colour alone** (§1.5, §7).
  - [F] In the prototype, Paused and Offline are the same hollow gray dot (index.html:174).
  - [F] Working and Available are the same green and differ only by the pulse, which reduced motion removes.
  - Fixes: Working's dot becomes indigo, Paused gets its own mark, and every avatar dot has its word next to it. In dense places (feed, chat, board), the word goes in the accessible name and a tooltip instead; [F] there it's missing today.
- **Not presence:**
  - Former is a membership state.
  - "Started by a person" becomes meta text.
  - Online, Checking…, Update needed and the other office statuses stay office words (§7).
- **Goal paused stays amber,** because it creates a Needs-you item; it's an Inbox kind, not a presence state.
- **Truth (§1.6):** with v1.0 data, Working and Available may not be knowable. Show "Status unknown" rather than guess. Please confirm with Rowan which states v1.0 can report (Q4).
- **"Paused by you"** claims who paused the agent. Say "Paused" unless the data records who did it.

## D8, item by item

| Item | Today in the system | Position |
| --- | --- | --- |
| Paused is gray; amber means only "Needs you" | §7 and §8 make Paused amber, and the product decisions already say amber means waiting on you, so the system contradicts itself | **Adopt.** The paused avatar mark goes gray too |
| "At risk" is red | §7 has no project-health row | **Adopt, with a change.** On track green; At risk red with `exclamation-triangle`; Off track red with `x-circle`. The icon tells the two reds apart. Use the icon in the Projects list too, not only on Home |
| Outlined cards on Home and the Teams index | §1.3 and §25: lists are rows, not cards | **Home: adopt** (few items, mixed data), without the amber stripe. **Teams index: don't.** It's a list, so use divided rows, the same as the People directory |
| Team page: Members above the tabs | §12: tabs right under the header | **Adopt, capped at 6 rows** (T2) |
| The org chart is a vertical tree | §12: Org chart has Teams / People / Reporting lines views | **Adopt, with a change:** the tree replaces the Reporting lines view. Teams and the members list cover the other two |
| Chat has a "Between agents" group | Silent | **Adopt, with a change.** [F] The prototype says "if you reply, both see it" and shows a reply box, but LIVE-3 makes these threads read-only unless the owner is a recipient. Keep it read-only with a note, or rewrite LIVE-3 deliberately (Q5) |
| Sidebar pins up to 5 projects | §3: every project A–Z, capped at 8, plus "View all" | **Adopt, with a change.** The 5 with the most recent activity, called "Recent" rather than "Pinned" (the owner didn't pin them), plus "View all" |

## Questions for you (product)

1. **Q1.** Inline Approve and Deny in the Inbox in v1.0, or Open and Reply only? This overrides a v1 spec rule.
2. **Q2.** "Members" or "People" for the mixed directory?
3. **Q3.** The Inbox kinds mapped to the spec's waiting kinds, with each kind's actions and whether it can be dismissed.
4. **Q4.** Which presence states v1.0 can actually report (with Rowan), and what a person's presence comes from while sign-in is out of scope.
5. **Q5.** Agent-to-agent threads: read-only (LIVE-3), or can the owner reply?
6. **Q6.** "Hire" or "Add"? [O] I'd go with **Hire** everywhere: it matches the hiring flow and the company model. §23 changes from "Add / Offboard" to "Hire / Offboard".
7. **Q7.** Where project progress, due date and health come from in v1.0. If there's no source, cards show status only (§1.6).
8. **Q8.** What search returns, and on which page.

## What I'll change in the system once we agree

When we've agreed Q1–Q8, I'll make design-system v2 a single amendment:
- **§2–§4:** frame, sidebar and top bar for the new nav.
- **§7:** the presence table above, project health, and Paused in gray.
- **§8:** the profile avatar size and the gray paused mark.
- **§9:** the decision card.
- **§12:** remove the Agents | Org chart block; add Teams tabs, profile tabs and settings navigation.
- **§21:** Home, Inbox and profile states.
- **§22:** the new icons.
- **§23:** Member, Inbox, Run, Agent setup, Hire.
- **Product decisions:** lines 581, 582 and 589.

At the same time I'll list the v1 spec IDs that IA v2 rewrites: SHELL-1, SHELL-2, SHELL-5; HOME-1 to HOME-14; ORG-1 to ORG-8; AGT-1 to AGT-10; §3.5; SESS; SET; CHAT-1; LIVE-3 and LIVE-4; PROF-1 and PROF-3; RUN-1. I'll also list the bug-70 rows (SH-1 to SH-6, SH-10, SH-12, SH-13, AG, AD-1, OC-1, OC-3, OF-6, UN-2), so ada and Rowan can re-sequence.

The merged S2 (the Agents | Org chart link bar, v1 673e1cb7) becomes the Teams view switch: the same component with new labels and routes.

Evidence: `scratchpad/ia-v2/render/shots/` (home, teams-platform, members-ada desktop; inbox phone opened), `render/measure.json`, `spec/written-conflicts-and-presence.md` (session scratch).

## Addendum after Rowan's technical read (11 Nov, late)

1. **"With you" joins the presence set.** The wire enum's `WithYou` means a person has taken over the agent's session.
   - Label: **"With you"** when it's the owner, "With {name}" for another person.
   - Pill: indigo, with the `user` icon. Dot: indigo-400, no pulse.
   - Precedence: Needs you > **With you** > Working > Paused > Available > Asleep > Offline > Status unknown.
   - It replaces the v1 spec's "Started by a person" status (RUN-1).
   - Wire mapping (no protocol change): Needs you = NeedsYou, With you = WithYou, Working = Busy, Available = Idle/Online, Asleep = Asleep, Paused = member status, Offline = Offline, Status unknown = Unknown.
2. **Truth until runtime reporting exists.**
   - [F] Rowan: nothing reports presence yet, so the server sends Offline for every member.
   - In v1.0 as it stands, the UI shows **Paused** when the member status says so, and **Status unknown** for everything else. That means no dots, and no "Offline" or "Working", until the office actually reports runs (§1.6).
   - Home's "Happening now" shows its empty state ("No one is working right now" becomes "Live status isn't available yet").
   - "Working on {task}" can show from `current_task_id` once C2a lands, because that is real data, without a presence pill.
3. **The owner on teams (Emery's D4).**
   - [F] The org model doesn't allow the owner to be a member of any team (org-office structure.rs:144).
   - My design doesn't need that rule to change. Team pages list members only, and the owner never appears as a team member row.
   - The owner appears once: the owner card on the Teams index and Org chart (the existing OwnerBar), "Reports to You" on members' meta lines, and the sidebar footer.
   - A "You" row inside a team would suggest a membership the data doesn't have.
   - Changing the org rule would be a one-way door; that's for Emery and ada to decide, not something to design around.
