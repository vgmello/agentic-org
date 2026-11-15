# Product vision and UX lens (v1.0)

Emery (`ac-product-manager-emery`) · 2026-11-12 · The one-page summary of the product we're building and the questions to ask of every screen. It's written for whoever designs, builds or validates a feature.
- **PRD** = `information-architecture-v2.md`.
- **Stories** = `ia-v2-stories.md`.
- **DS** = Eden's `knowledge/design-system.md` (v2.4).

Where this page and those sources differ, the sources win; tell me, and I'll fix this page.
Labels: **[F]** fact · **[O]** my call (the owner delegated product decisions to me on 2026-11-11).

## 1. The vision

**[F] The owner, 2026-11-11 (PRD §1):**
- The app is organized the way a company works: an organization, its offices, the projects it works on, and teams of people and agents.
- Projects, their tasks and their status are what everyone looks at most.
- Agents stay, as a more advanced, configuration-like area in less technical terms.
- Offices are server configuration, under organization or settings.
- Lead with what's used most and matters most, and with how humans step into the loop of autonomous agents.
- The look and feel stays. Icons are welcome, and emoji aren't allowed anywhere in the system.

**[O] How I turned that into the product** (PRD §1-§2):
- **The owner's questions come first:**
  - What needs me?
  - How are my projects doing?
  - Who's doing what right now?
- **The human loop is one place** (the Inbox), with enough context to act there.
- **An agent has two faces.**
  - The colleague face: name, role, team, presence and a record of what they did.
  - The setup face, which lives in Organization settings.

**[F]** IA v2 (PRD rev 4.1, milestones IA-M1 to IA-M3 in the stories, plus runtime presence IA-15 per D34) is the v1.0 shape. ada adopted it on 2026-11-12 with no re-baseline (my journal, 2026-11-12). Some stories ship in v1.0 as an honest placeholder; for example, an agent's Notes and memory tab reads "Not available yet: notes and memory arrive in a later release." (AC-IA14-2, D18; DS §21). Check each story's v1.0 scope before raising a finding.

## 2. Why we redid the design

**[F]** The earlier build met its specs, but the product had the wrong shape:

1. **It was organized around the machinery** (PRD §1). The landing page was Agents. The sidebar had 10 items, including Sessions, Offices and Processes. [O] The first screen answered "what agents exist and how are they configured?"
2. **The human loop was buried.** "Needs you" existed only inside Chat (PRD §1). A Tauri-era rule stopped the owner answering from the inbox, so every yes cost an extra screen (D9).
3. **Agents weren't colleagues.** The profile page had been dropped, so there was nowhere to see what an agent is working on, with whom, and what it did (PRD §1).
4. **Presence used inconsistent words.** Chat had six presence states, and the roster used five different words (PRD §7).
5. **There were emoji, and two icon sets.** Chat had 14 emoji, and Lucide was mixed in with Heroicons (PRD §8, E2).

**[O] The lesson for validation:** checking a screen against its criteria isn't enough. Every review should also ask whether the screen fits the vision (§3). A screen can pass every criterion and still lead with the machinery, or hide the human loop.

## 3. The lens: questions to ask of every screen and feature

1. **Company, not runtime.** Does the screen use the organization's words: organization, project, work, team, member, person, agent? [O] Machinery words (session, runtime, model, office, process, home folder, gateway) appear only where the machinery is the subject. Examples:
   - Organization settings;
   - the sidebar's office status line;
   - an agent's Setup summary on their profile;
   - a run's log in Activity;
   - Inbox items about setup or offices (an unplaced agent, a missing model, an office join request).

   The DS's pinned copy is always correct, even when it uses one of these words (for example "Couldn't reach the office. Your data is safe."). Raise a finding only for machinery words in everyday work screens that aren't pinned.
2. **Urgent and frequent first.** The order is: what needs the owner, then what's happening, then projects and work, then people and structure, then configuration (PRD principle 2). On Home, the order is Needs you, then Projects (D1), then Happening now and Recent activity (AC-IA04-1). Once Following ships (TK-1), its card, hidden when there's nothing to follow, sits between Needs you and Projects (D38). Once the away digest (AW-3) ships, after an away of at least the owner's threshold the "While you were away" card leads Home, above Needs you, and never replaces it (D41, AC-IA04-1).
3. **One human loop.** Every Needs-you source reaches the Inbox as exactly one kind (D11, D26). Sources whose feature isn't in v1.0 arrive when that feature ships (D26). Check each kind's scope in the stories. A missing runtime is the exception: it shows in Agent setup and raises no Inbox item (DS). Each item shows who, what, why, the actions and any expiry (PRD §6). The owner acts right there wherever the operation exists; otherwise the card offers Open (D9, AC-IA11-1).
4. **Two faces of an agent.** The colleague face (with the team) and the setup face (Agent setup) are separate, and each links to the other. Agent setup shows Health, not presence (D20).
5. **People and agents are equals.** Both use the same patterns, and the kind is always said in words, never only a shape or colour (PRD principle 4).
6. **Truth over decoration** (D12, D15, D22). Every status, presence, health or progress value comes from real data. If the data isn't there:
   - leave the value out: no pill in lists (AC-IA08-2), nothing for an unset health, due date or lead;
   - or say so honestly: presence "Status unknown" on an agent's profile header only (a person's profile shows no presence in v1.0; office statuses may also read "Status unknown"; DS), or "Live status isn't available yet." (AC-IA04-2; DS §21).

   Until runtime presence (IA-15, in v1.0 per D34) lands, presence shows only Paused, Needs you (once the Inbox, IA-09, exists), "Working on {task}" and Status unknown (D12; AC-IA08-2). After that, the full DS presence set applies to agents, including Working and Available. People other than the owner have no presence in v1.0 (D12).
7. **Amber means "waiting on you".** The DS's amber statuses (such as Update needed, Goal paused and Needs setup) mean that too. So do the Needs-you bell and the Inbox badge (DS §7). The only other amber is the warning icon in callouts, toasts and banners (DS), and, once AW-3 ships, the "While you were away" card's single "N things need you" line (D41, AC-AW3-2). There's no amber Scoped pill (D23).
8. **No emoji; one icon set** (D7, E1-E3; DS).
   - Icons are Heroicons: outline 24 for navigation, empty states and toasts; solid mini 20 in buttons, menus and controls; solid micro 16 in pills and meta.
   - The chrome (sidebar, top bar, headers, pills, buttons, menus) is Heroicons only. Lucide may remain only inside the content of screens not yet restyled (BUG-70), and the two sets never mix on one screen (DS).
   - Decorative icons are hidden from screen readers (`aria-hidden`). An icon-only control has an `aria-label` and a tooltip (DS §22).

## 4. Invariants a validator can test

- **Work blocked on the owner always has an open Inbox item,** from IA-10c. Work blocked on someone else doesn't need one (D26, AC-IA18-1). The item is the open Approval or Question. If there's none, or after that item is refused, it's a "Blocked on you" Problem. Exception: work blocked by its budget ("Budget reached") has only its Budget reached item, never a "Blocked on you" item; work already blocked on the owner before its budget was reached keeps its "Blocked on you" item and gets the Budget reached item too (D26, D42; `tasks-v1.md` AC-TK4-19). A refusal (Deny, or "Not now" in Chat) leaves the work blocked, and visibly so. A "Blocked on you" item clears to Handled when the work is unblocked. It's withdrawn, with no Handled entry, when an Approval or Question about the work opens (D26; DS §9).
- **Work → Needs me lists exactly the five cases where work waits on the owner** (AC-IA18-1, `tasks-v1.md` §4.1): blocked on the owner; the owner is the reviewer; an open Hand-off; an open question to the owner; a reached budget (a Budget reached block or hold). A chat hand-off is listed only when it has a work item; a task Hand-off is listed while it's open. Sequencing:
  - Until IA-10, Needs me uses work status, and the Inbox holds only chat asks (IA-18 Route/data; AC-IA09-3).
  - From IA-10c (Blocked on you), every Needs me row has a matching open Inbox item, except a subtask stopped by its parent's budget ("Parent budget reached", blocked or held), whose row links to its parent's item (`tasks-v1.md` §4.1, AC-TK4-19). For tasks, the Blocked on you and Hand-off rows ship with task statuses and reassignment (D38, `tasks-v1.md` §6.13, TK-4, TK-6).
  - Chat hand-offs arrive with IA-10b; the review case shows once In review ships (TK-5); the budget case once costs ship (D42).

  The Needs me count isn't the Inbox count, because the Inbox also holds hire requests and other kinds.
- **The Inbox badge equals Home's "Open Inbox (N)"** (AC-IA09-4).
- **Every Inbox item clears when it's resolved anywhere,** including in a terminal, in another client, or because the agent carried on. Only run error and crash items can be dismissed; Budget reached items and the routine Problems ("missed twice", "needs a lead") can't (D11, D26, D42, D44).
- **A Critical bug item offers "Assign…"** and clears when the bug is assigned or closed. Triage alone doesn't clear it (D26; DS §9).
- **Missing model** (D26, MOD-7). The item shows only when an exact model is set and the runtime doesn't offer it. It doesn't show:
  - when no model is set;
  - when the runtime's model list can't be read;
  - when the runtime is missing, or the office is unreachable. Those are other states.
- **The owner isn't a team member.** They appear once: the owner card, "Reports to You", and the footer. There's no `/members/you`. The account menu shows Sign out only once owner sign-in exists (D17, D21).
- **Names** (D10, D14, D33):
  - "Members" is the mixed directory, with the filter All · People · Agents, and "People" means humans only.
  - The actions are "Hire an agent" and "Add a person". There's no "Invite" in v1.0.
- **Agent-to-agent threads are read-only,** with the note "Reading as the owner. To weigh in, message {agent}", as pinned in the DS (D13, D27).
- **Buttons:** on Home, card buttons are outline only, and Home has no page primary. Every view has at most one primary action (D9, AC-IA04-4; DS).
- **A gated route in production shows the root Unavailable page,** "{Title} isn't available yet". By design, that page has no nav list and no "Back to Home" button (AC-IA03-3; DS).
- **In-shell copy for things that aren't there yet** (AC-IA03-3; DS). There are two kinds, and they never mix on one page. Never use "Coming soon". Each kind uses the DS's full pinned copy, both sentences (DS §21):
  - **A section out of v1.0:** "Not in this release", plus that section's own second sentence from DS §21 (for example "Roles will define what each agent does.").
  - **A v1.0 section that isn't built yet:** "{Title} isn't built yet", plus the fixed second sentence "It's coming in this release."

  The DS pins two more cases, and both are correct too:
  - the Notes and memory tab placeholder, "Not available yet: notes and memory arrive in a later release." (DS §21);
  - a disabled control's reason, "Available in a later release" (DS). Menu items whose flow doesn't exist are left out, not disabled (AC-IA19-1, D21).
- **Every section has an honest empty state,** such as "Nothing needs you" (PRD §6).
- **URLs use IDs, and old URLs redirect,** keeping their query string (D19, AC-IA01-3).
- **Every story passes the UI preflight** (Stories, header): four widths, 200% text, the keyboard focus path, and no emoji.
- **Keyboard** (AC-IA19-4; DS):
  - The single-key shortcuts switch (WCAG 2.1.4) governs N and `/`.
  - [O] After Cancel, Escape or an action, focus returns to the control that opened it or to the next sensible one, never to the page body.
- **No emoji in UI copy, mock or seeded data, or agent-written text.** Agent text is stripped at render, never in storage (AC-IA16-1, D7, D19). Text a person types is shown as typed. Once IA-16 ships, the UI suite's emoji check is green (E3, AC-IA16-2). IA-16 is the first story cut if scope slips (Stories), so check its state before raising a finding on agent text or on the emoji check.

## 5. Owner jobs to walk end to end

Walk these jobs, not single screens. Walk each one as far as the current build's scope allows; for example, Critical bug and Missing model items arrive with their own slices.

1. **Open the app.** Home answers what needs me, how the projects are doing, and who's working. After a long enough away (once AW-3 ships), it first says what happened while I was away (D41).
2. **Act in the Inbox.** Approve, deny or reply where the operation exists, and open the item otherwise. After a denial on blocked work, the work stays blocked and gets a "Blocked on you" item (unless the block is Budget reached, D26).
3. **Accept a hand-off, and assign a critical bug** with "Assign…".
4. **Open a project.** Check its progress and due date (when set), its open and blocked work, its team, and its conversation.
5. **Find a colleague.** See what they're working on and what they did, then message them or assign them work.
6. **Hire an agent, then give it a model its runtime doesn't offer.** A Needs you item appears; clearing the model removes it.
7. **Configure an office.** This is rare, and it lives in Organization settings.

## 6. The prototype

The prototype, draft 4.1 (https://claude.ai/artifact/JX8BmbmJqkc7PAv47uAshh; source in `../prototypes/ia-v2/index.html`) is the reference for structure, flows and behaviour.

- Its mock data and exact pixels are illustrative. On the look, DS v2.4 wins.
- It has two data modes, switched with the "Data" control in the prototype banner. "Full demo" shows everything. "v1.0 today" shows presence and runs as they are before IA-15 lands, and notes and memory as they are before IA-14. The Inbox mock data is the same in both modes and includes kinds that aren't in v1.0. Check scope in the stories, not the prototype.

## Sources

- PRD: the problem, principles, IA and decisions D1-D45 (D45: v1 runs in the user's own Chrome or Chromium, Firefox or Safari, with no desktop shell).
- Stories: IA-01 to IA-19, with stable AC IDs.
- `ia-v2-backlog-triage.md`: the joint KEEP / RE-POINT / RETIRE list, which ada accepted.
- DS v2.4: the look, tokens, pinned copy and Inbox kinds (§9).
