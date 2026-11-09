---
kind: spec
title: "Capture requests and route them to whoever should scope them (UX)"
status: for-review
revision: 3
owner: ac-product-designer-eden
created: "2026-11-08"
updated: "2026-11-09"
requested_by: owner
reviewers: [ac-staff-engineer-ada, owner]
builds_on: [work-items-ux, decisions-and-tech-debt-ux, agent-run-modes-ux]
---

# Spec · Capture requests and route them to whoever should scope them (UX)

Extends `work-items-ux.md` (Work, tasks), `decisions-and-tech-debt-ux.md` (specs, designs, guidance), `agent-run-modes-ux.md` (waking agents) and the smart-features candidates.

## The request (the user, 2026-11-08)

> I want to capture new requirements or features exactly the way I'm telling you now. I press a shortcut, anywhere, and start typing. It becomes a **feature request**, not a task yet, in a queue ("I need to design this later"). The right agent picks it up and scopes it, and then it becomes tasks we implement. A smart feature can **suggest** who should scope it: a feature goes to the designer, something technical to the SDM or the staff engineer, something very technical maybe straight to a dev. It's only a suggestion: I see the options, and I can change it. Or I can just talk to the agent directly.

**Revision 3 (2026-11-09): the details app-frontend-dev needs next (from a spec audit).** Capture (§1.1): where the panel opens and for which organization, @name routing (only at the start, completion, the handle removed, unknown names), Message instead (recipient and confirmation), the bug hint (copilot or words, placement, Not a bug), similar requests (not yet scoped, Add to closes the panel), attachments hidden until they can be stored, the confirmation banner, saving while the office can't be reached (kept on this Mac, sent later), and Settings › Shortcuts with a menu-bar item. Queue (§2): filters, search and states; the project tile opens the filtered queue. Page (§3): Merge and Decline only from open states (ada's design); "From REQ-n" on the task page. **Criteria rewritten (same ID):** RQS-1, RQS-4, RQS-6, RQS-13 *(rev 3)*. **Added:** RQS-15 to RQS-18.

**Revision 2 (2026-11-08): aligned with ada's W7 model.** A request has a new state, **Routed**, between New and With <agent>: it reads "Sent to Eden · not accepted yet" until the agent accepts it (`request.accept`, `org-agent request accept`), then "Eden is scoping" (decisions 3 and 6, §2, §3). The With agents group holds both. Approve needs a project: if the request has none, Approve asks for one first (decision 7, §3). **Criteria rewritten (same ID):** RQS-7, RQS-9. **Added:** RQS-14.

## 0. Decisions

1. **One shortcut, anywhere: ⌥⌘N, "New request".** It works system-wide (from the menu bar too), even with the app's window closed, and opens a small capture panel. The shortcut can be changed in Settings. Inside the app, ⌘K › "New request" does the same.
2. **Capturing asks for nothing but the text.** Type (or dictate: macOS dictation works in the field) and press ⌘Enter. Everything else (project, kind, who scopes it) is suggested and can be changed, or left for later.
3. **A request is not a task.** Its lifecycle: **New → Routed → With <agent> (scoping) → Scoped → Approved** (it becomes tasks), or **Declined · Merged into REQ-n**. **Routed** means sent to an agent that hasn't accepted it yet: "Sent to Eden · not accepted yet". When the agent accepts it (`request.accept`), it reads "Eden is scoping". *(rev 2)* Numbered org-wide, `REQ-12`, because a request may not belong to a project yet.
4. **Routing is a suggestion with reasons, never a silent decision.** The panel shows the suggested agent and two alternatives, each with one line of why ("Eden, product designer: a new user-facing flow"). The owner picks one, types `@name`, or chooses **Just save** to queue it unrouted.
5. **Talking to an agent directly stays a first-class path.** Starting the text with `@` and a handle (an agent's messaging name such as `@ac-product-designer-eden`, completed from display names as you type: "Eden") chooses that agent instead of the suggestion; **Send** then routes it there *(rev 3, §1.1)*. **Message instead** turns the capture into an ordinary message.
6. **The scoping agent is woken** (on call, `agent-run-modes-ux.md`), gets the request in its inbox, **accepts** it (`org-agent request accept`), and from then owns it until it's scoped, rerouted, declined or merged. It can ask the owner questions inside the request.
7. **Scoped means a feature the owner can approve** *(rev 1a: features are first class, `features-ux.md`)*: the request materializes as a **Proposed feature** (`FEAT-n`, in the repository) or extends an existing one, with links to the specs, designs or ADRs the agent wrote, the proposed tasks (drafts, not yet real), and an estimate (effort, and an API-price cost range when the agent gives one). **Approve** sets the feature to Planned and creates its tasks. Approve needs a project: if the request has none yet, it asks for one first *(rev 2)*. Nothing is built before that.
8. **Requests are working data, and their history lands in the repository:** a request lives in the office database like tasks. When it's approved, the **feature** it produced (or extended) lists it in its front matter (`requests: [REQ-n]`, `features-ux.md`) and quotes the original words in its Notes, so the "why" of a feature survives after the request is closed. *(rev 1a: on the feature, not on specs.)*
9. **Agents can raise requests too** (`org-agent request add`), for example a dev who finds a missing capability. They arrive in the same queue, marked with who raised them.

## 1. Flow 1 · Capture

```
 ┌ New request ──────────────────────────────────────────────── ⌥⌘N ─┐
 │ Let me capture feature requests with a shortcut and route them to   │
 │ whoever should scope them…                                           │
 │                                                                      │
 │ Project  [Agentic Org ▾]  (suggested)     Kind  [Feature ▾] (suggested)│
 │ Scope it ◉ [E] Eden · product designer: a new user-facing flow       │
 │          ○ [A] ada · staff engineer: touches the office and CLI      │
 │          ○ [C] codex-dev · owns the work-items library               │
 │          ○ Someone else… (or type @name)                             │
 │                                                                      │
 │ [Just save]  [Message instead]                    [Send to Eden ⌘↵]  │
 └──────────────────────────────────────────────────────────────────────┘
```

- **The text field** is focused at once, grows to 12 lines and then scrolls. Pasting an image attaches it (screenshots are common) once attachments can be stored; until then a pasted image is ignored, and only text is pasted *(rev 3, §1.1)*.
- **Suggestions appear after a pause in typing** (about 1 s) and update as the text changes, unless the owner has already picked something. A small "Suggesting…" shows while the copilot works. The send button names the current choice.
- **Kinds:** **Feature** (new capability) · **Change** (to something that exists) · **Technical** (architecture, performance, infrastructure) · **Question**. If the text reads like a bug, a line offers "This sounds like a bug. **Report a bug instead**", which carries the text over.
- **Without the copilot** (off, or no runtime): the suggestion comes from simple rules: Feature or Change → the product designer, Technical → the staff engineer (or the project's lead), Question → the project's lead. It's marked "Suggested by rule" so it's clear why it's less specific.
- **Esc** with text asks "Keep as a draft?" (**Keep** saves it as an unrouted request, **Discard**, **Back**).
- **Sent:** the panel closes, and a quiet confirmation says "REQ-12 sent to Eden" (§1.1). Nothing else interrupts.

### 1.1 Capture details *(rev 3)*

- **The panel** is a small floating window at the top third of the screen the pointer is on, above other apps. Pressing the shortcut again while it's open brings it forward with the text kept. It captures into the organization selected in the menu bar; with more than one, the panel's title reads "New request · <organization> ▾" and can be switched before sending. Every control has a label, Tab moves through them in order, and Esc follows the draft rule above (§1).
- **@name routing:** only at the very start of the text. Typing `@` there opens a completion list of agents (display name, handle, role and status; only agents scope requests), filtered as you type; ↑↓ move, Enter or Tab picks, Esc closes the list. Picking chooses who scopes it, as choosing them in the list would; **Send** then routes it to them, and the `@handle` is removed from the saved text. An `@` anywhere else is just text. An unknown handle shows "No one called @<handle>. Pick someone from the list or remove it." and Send is disabled until it's fixed.
- **Message instead** sends the text as an ordinary message (not a request; `chat-ux.md`) to the member chosen above or named with `@`. With nobody chosen it's disabled with "Choose who to message." While the office can't be reached it's disabled with "Messages can't be sent until the office is back. Send it as a request instead (it's kept on this Mac)." The confirmation reads "Message sent to Eden · Open in Chat".
- **The bug hint** sits under the text field and appears when the text reads like a bug: from the copilot when it's on, otherwise from words such as "broken", "crash", "error", "doesn't work", "fails", "bug". It reads "This sounds like a bug. **Report a bug instead** · Not a bug". **Report a bug instead** opens Report a bug (`work-items-ux.md` §4) with the text in What happened and the chosen project (when none is chosen, Report a bug asks for the project first; the area starts on General). **Not a bug** hides the hint for this capture.
- **Similar requests** appear under the text after a pause in typing: up to 2 requests not yet scoped (New, Routed or being scoped) whose words match ("Looks like REQ-9: Dark mode for the docs reader"), each with **Add to REQ-9**, which adds the text to that request as a comment, closes the panel and confirms "Added to REQ-9". They match the title and the request's own text (the text once `work.list` has a text filter, A76; titles only until then).
- **Attachments** (pasted images) are hidden until their storage is designed (deferred, A76); the panel then accepts a pasted image as an attachment.
- **The confirmation** after Send, Just save or Add to: a small banner where the panel was, for about 3 s, "REQ-12 sent to Eden · Open" (or "REQ-12 saved", "Added to REQ-9"), then it fades. It never takes focus.
- **When the office can't be reached** (stopped, or the Mac is offline): the panel still opens, and Send or Just save keeps the request on this Mac with "Saved on this Mac. It gets a number and is sent when the office is back." It's sent, and routed as chosen, when the office is back; the menu-bar menu shows "1 request waiting to send" until then. Nothing typed is ever lost. (This is the one exception to "writes are off while the office is stopped", `work-items-ux.md` §12.)
- **Settings › Shortcuts:** a row "New request ⌥⌘N" with **Change…** (press the new keys; when another app or the system already uses them: "⌥⌘N is used by another app. Choose another shortcut."), **Reset** and **Turn off**. The menu-bar menu has **New request… ⌥⌘N** (`offices-and-teams-ux.md` §4).

## 2. Flow 2 · The queue (Work › Requests)

Work gets a fourth pill: **Tasks | Bugs | Debt | Requests**. Each project's Overview has a **Requests** tile (n open), which opens Work › Requests filtered to that project *(rev 3)*.

```
 Requests          [Open ▾] [Anyone ▾] [All projects ▾]  [Search]        [New request ⌥⌘N]
 NEEDS YOU · 2
   REQ-13  Costs per project                     Scoped by Eden · FEAT-0009 · ≈ $40–70  [Review]
   REQ-15  Weekly digest by email                Eden asks: "Daily or weekly?"       [Answer]
 WITH AGENTS · 3
   REQ-17  Export a feature as PDF               Sent to Eden · not accepted yet · 5 min
   REQ-16  Keyboard shortcuts cheat sheet        Eden is scoping · 20 min
   REQ-10  Checklists on tasks                   codex-dev is scoping · 1 h
 NOT ROUTED · 1
   REQ-9   Dark mode for the docs reader         saved 2 d ago                         [Route]
 APPROVED · 1 (collapsed)
   REQ-14  Quick capture of feature requests     → FEAT-0007 · 4 of 6 tasks done
```

- **Groups:** *(rev 3)* Waiting to send (captures kept on this Mac while the office couldn't be reached, §1.1, shown first and only while there are any: the first line of the text, "Saved on this Mac · will go to Eden", and **Discard**; they become real requests, with numbers, once sent) · Needs you (scoped and waiting for approval, or a question for the owner) · With agents (Routed, not accepted yet, and being scoped) · Not routed · Approved (collapsed) · Declined and merged (collapsed).
- **Needs you** items also appear in the owner's Home › Needs you.
- **Row:** number · title (the first line of the text, or a title the agent gave it) · state in words · who · age · the next action.
- **Filters and search** *(rev 3)*: **State ▾** (Open, the default: New, Routed, being scoped or Scoped · Needs you · With agents · Not routed · Approved · Declined and merged · All) · **People ▾** (Anyone · Raised by me · Raised by <member> · Scoped by <member>) · **Project ▾** (All projects · No project · each project). **Search** matches the number (`REQ-12`, matched by the app on the number itself), the title and the text (the text once ada adds a text filter to `work.list`, A76; until then, titles only). The filters are remembered.
- **States** *(rev 3)*: loading (skeleton rows) · empty ("No requests yet. Capture one when something comes to mind: write what you need and choose who scopes it.") · no match ("No requests match. **Clear filters**") · can't load ("Can't load requests." with **Retry**) · the office stopped (the last list stays readable; actions are off with the reason, and capture still works, §1.1).

## 3. Flow 3 · A request's page

- **Main column:** the original text (as typed, with attachments once they can be stored, §1.1), then the conversation: the scoping agent's questions and the owner's answers, notes and changes of route ("You moved this from ada to Eden").
- **Side column:** state · kind · project · who scopes it (**Change…**: the same suggestion list) · raised by and when · **Outcome** (once scoped: spec, design or ADR links, proposed tasks, estimate).
- **Owner's actions by state:** New → **Route…** · Routed or With agent → **Change…**, **Message <agent>** · Scoped → **Approve and create 6 tasks** (when the request has no project, it first asks "Which project?" with a picker of projects that aren't archived; then it opens the task drafts, each editable or removable, then **Create tasks**), **Ask for changes…**, **Decline…** · any open state (New, Routed, With agent, Scoped) → **Merge into…** (another open request; disabled with "No other open requests to merge into." when there's none), **Decline…** (reason required) *(rev 3: open states only, ada's design)*.
- **Agent side (CLI):** `org-agent request show|accept|ask|reroute --to <member> --reason|scope --doc <id> --task "<title>"…|decline --reason|merge --into REQ-n`, plus `request add` to raise one. The words match the app's.
- **Approved:** the tasks appear in the project (To do) linked to the request: the task page's side column shows **From** "REQ-12 · Costs per project" as a link *(rev 3)*. The request shows their progress ("4 of 6 done") and the **feature** tracks the build from there (`features-ux.md`). The request itself ends at Approved.

## 4. States and edge cases

| Case | Behavior |
| --- | --- |
| The suggested agent is asleep (on call) | It's woken by the request (run modes RUN-3). The panel says nothing extra. The request shows "Eden is waking up". |
| The chosen agent is Offline or paused (Riley) | Shown in the list with its status, and choosing it warns: "Riley is offline and won't pick this up. Choose someone else or Just save." |
| The agent reroutes it | The owner sees "Eden sent REQ-12 to ada: 'mostly the office protocol'" in the request and Activity. No approval is needed for a reroute. |
| No answer from the scoping agent (on call, failed wakes) | After the run-mode failure rules, Needs you shows "REQ-12 is stuck with Eden", with **Change…**. |
| Duplicate of a request not yet scoped | While typing, up to 2 similar requests not yet scoped show ("Looks like REQ-9: Dark mode…"), with **Add to REQ-9** (appends the text as a comment). |
| Owner offline / app closed | The shortcut still opens the capture panel (the app runs in the menu bar). Saving keeps it on this Mac until the office is back, then sends and routes it (§1.1) *(rev 3)*. |

## 5. UX acceptance criteria (RQS)

Criterion ids use **RQS-** so they never collide with request records (`REQ-n`) *(renamed in rev 1a, before any test used them)*.

- RQS-1 · *(rev 3)* ⌥⌘N opens the capture panel from anywhere, with the app window closed, with the text field focused, at the top third of the pointer's screen, for the organization selected in the menu bar (switchable in the title when there are several). Pressing it again brings the open panel forward with its text. Settings › Shortcuts can change it (Change…, Reset, Turn off), and when another app already uses it, says so and asks for another. The menu-bar menu has New request… ⌥⌘N.
- RQS-2 · ⌘Enter with only text saves a request. Project, kind and route are suggested and editable, and none is required.
- RQS-3 · The panel shows a suggested agent and two alternatives, each with a one-line reason, and the owner can pick another, type @name or Just save.
- RQS-4 · *(rev 3)* Typing @ at the start of the text opens an agent completion (keyboard: ↑↓, Enter or Tab, Esc); picking chooses who scopes it (Send routes it to them) and removes the handle from the text, and an unknown handle disables Send with the reason. Message instead sends the text as an ordinary message to the chosen member, is disabled with "Choose who to message." when nobody is chosen, and with the reason while the office can't be reached, and confirms "Message sent to <name> · Open in Chat".
- RQS-5 · Without the copilot, the suggestion follows the kind rules and is marked "Suggested by rule".
- RQS-6 · *(rev 3)* Text that reads like a bug (the copilot, or words like crash, error, doesn't work) shows the hint under the text with Report a bug instead (the text and project carried over; no project asks first) and Not a bug.
- RQS-7 · *(rev 2)* Work has a Requests pill grouped as Needs you, With agents (Routed and being scoped), Not routed, Approved, and Declined and merged. Scoped requests and agents' questions appear in Home › Needs you.
- RQS-8 · A routed request wakes an on-call agent and appears in its inbox. Choosing an offline or paused agent warns first.
- RQS-9 · *(rev 2)* A scoped request shows the linked docs, the draft tasks and the estimate. Approve creates the tasks (each editable or removable first), linked "from REQ-n". When the request has no project, Approve asks for one first, and nothing is created without it.
- RQS-10 · Reroutes, questions, answers, declines and merges are recorded on the request and in Activity, with who and why.
- RQS-11 · The feature a request produced or extended lists it in its front matter (`requests:`) and quotes its original text.
- RQS-12 · Agents can raise, accept, ask, reroute, scope, decline and merge requests with `org-agent request …`, with the same words as the app.
- RQS-13 · *(rev 3)* Up to 2 similar requests not yet scoped (New, Routed, being scoped) show under the text while typing, with Add to REQ-n, which adds the text as a comment, closes the panel and confirms "Added to REQ-n".
- RQS-14 · *(new, rev 2)* A routed request shows "Sent to <agent> · not accepted yet" until that agent accepts it, then "<agent> is scoping". Rerouting it makes it Routed again for the new agent.
- RQS-15 · *(new, rev 3)* The queue filters by state (Open by default), people (raised by, scoped by) and project, searches the number, the title and the text (the text once `work.list` has a text filter, A76; titles only until then), remembers the filters, and shows loading, empty, no match (Clear filters), can't load (Retry) and office-stopped states. The project's Requests tile opens the queue filtered to that project.
- RQS-16 · *(new, rev 3)* A task created from a request shows From "REQ-n · <title>" in its side column, linking to the request. Merge into… and Decline… are offered only in open states.
- RQS-17 · *(new, rev 3)* With the office unreachable, the capture panel still saves: the request is kept on this Mac ("Saved on this Mac. It gets a number and is sent when the office is back."), the menu-bar menu shows how many are waiting, the queue shows them first under Waiting to send (with Discard), and each is sent and routed as chosen when the office is back.
- RQS-18 · *(new, rev 3)* After Send, Just save or Add to, a banner confirms for about 3 s ("REQ-12 sent to Eden · Open") without taking focus. Attachments are hidden until the office can store them.

## 6. Open questions

**For ada and codex-dev**
- ~~A29~~ · *Resolved (ada):* a request is a work-item kind on the shared core (REQ-n, a state machine, a thread of messages). Approved creates the linked tasks in one command. codex-dev's W7 model covers it. Plan row W16: codex-dev (model), Remy (CLI), app-frontend-dev (UI).
- ~~A30~~ · *Resolved (ada):* the Tauri global-shortcut plugin in the menu-bar app. It's configurable, and if another app owns the shortcut, Settings shows "⌥⌘N is used by another app. Choose another shortcut." (RQS-1).
- ~~C22~~ · *Resolved (ada):* smart feature #16, with the rule fallback when no runtime is set up.

- A76 · *Resolved in part (ada, 2026-11-09):* similar requests match the title and the request's own text (no outcomes for now): a simple word match on the mock, a text filter on `work.list`, which ada adds to the contract (not there yet: today `work.list`'s `query` matches titles only). Attachments stay hidden until their storage is designed (deferred). Still open: the app keeping a capture on this Mac while the office can't be reached, then sending it (§1.1, RQS-17).

**For the user**
- ~~U21~~ · *Resolved (the user, 2026-11-08): as recommended.* ⌥⌘N as the default shortcut (⌘N is New in many apps, and ⌥⌘N is rarely taken)? And is an approval step before tasks are created what you want (my recommendation), or should a scoped request become tasks straight away?
