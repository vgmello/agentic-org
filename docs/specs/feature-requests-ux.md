---
kind: spec
title: "Capture requests and route them to whoever should scope them (UX)"
status: for-review
revision: 1
owner: ac-product-designer-eden
created: 2026-11-08
updated: 2026-11-08
requested_by: owner
reviewers: [ac-staff-engineer-ada, owner]
builds_on: [work-items-ux, decisions-and-tech-debt-ux, agent-run-modes-ux]
---

# Spec · Capture requests and route them to whoever should scope them (UX)

Extends `work-items-ux.md` (Work, tasks), `decisions-and-tech-debt-ux.md` (specs, designs, guidance), `agent-run-modes-ux.md` (waking agents) and the smart-features candidates.

## The request (the user, 2026-11-08)

> I want to capture new requirements or features exactly the way I'm telling you now. I press a shortcut, anywhere, and start typing. It becomes a **feature request**, not a task yet, in a queue ("I need to design this later"). The right agent picks it up and scopes it, and then it becomes tasks we implement. A smart feature can **suggest** who should scope it: a feature goes to the designer, something technical to the SDM or the staff engineer, something very technical maybe straight to a dev. It's only a suggestion: I see the options, and I can change it. Or I can just talk to the agent directly.

## 0. Decisions

1. **One shortcut, anywhere: ⌥⌘N, "New request".** It works system-wide (from the menu bar too), even with the app's window closed, and opens a small capture panel. The shortcut can be changed in Settings. Inside the app, ⌘K › "New request" does the same.
2. **Capturing asks for nothing but the text.** Type (or dictate: macOS dictation works in the field) and press ⌘Enter. Everything else (project, kind, who scopes it) is suggested and can be changed, or left for later.
3. **A request is not a task.** Its lifecycle: **New → With <agent> (scoping) → Scoped → Approved** (it becomes tasks), or **Declined · Merged into REQ-n**. Numbered org-wide, `REQ-12`, because a request may not belong to a project yet.
4. **Routing is a suggestion with reasons, never a silent decision.** The panel shows the suggested agent and two alternatives, each with one line of why ("Eden, product designer: a new user-facing flow"). The owner picks one, types `@name`, or chooses **Just save** to queue it unrouted.
5. **Talking to an agent directly stays a first-class path.** Starting the text with `@` and a handle (an agent's messaging name such as `@ac-product-designer-eden`, completed from display names as you type: "Eden") skips the suggestion and sends it there. **Message instead** turns the capture into an ordinary message.
6. **The scoping agent is woken** (on call, `agent-run-modes-ux.md`), gets the request in its inbox, and owns it until it's scoped, rerouted, declined or merged. It can ask the owner questions inside the request.
7. **Scoped means a feature the owner can approve** *(rev 1a: features are first class, `features-ux.md`)*: the request materializes as a **Proposed feature** (`FEAT-n`, in the repository) or extends an existing one, with links to the specs, designs or ADRs the agent wrote, the proposed tasks (drafts, not yet real), and an estimate (effort, and an API-price cost range when the agent gives one). **Approve** sets the feature to Planned and creates its tasks. Nothing is built before that.
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

- **The text field** is focused at once, grows to 12 lines and then scrolls. Pasting an image attaches it (screenshots are common).
- **Suggestions appear after a pause in typing** (about 1 s) and update as the text changes, unless the owner has already picked something. A small "Suggesting…" shows while the copilot works. The send button names the current choice.
- **Kinds:** **Feature** (new capability) · **Change** (to something that exists) · **Technical** (architecture, performance, infrastructure) · **Question**. If the text reads like a bug, a line offers "This sounds like a bug. **Report a bug instead**", which carries the text over.
- **Without the copilot** (off, or no runtime): the suggestion comes from simple rules: Feature or Change → the product designer, Technical → the staff engineer (or the project's lead), Question → the project's lead. It's marked "Suggested by rule" so it's clear why it's less specific.
- **Esc** with text asks "Keep as a draft?" (**Keep** saves it as an unrouted request, **Discard**, **Back**).
- **Sent:** the panel closes, and a quiet confirmation in the menu bar says "REQ-12 sent to Eden". Nothing else interrupts.

## 2. Flow 2 · The queue (Work › Requests)

Work gets a fourth pill: **Tasks | Bugs | Debt | Requests**. Each project's Overview has a **Requests** tile (n open).

```
 Requests          [Open ▾] [Anyone ▾] [All projects ▾]  [Search]        [New request ⌥⌘N]
 NEEDS YOU · 2
   REQ-13  Costs per project                     Scoped by Eden · FEAT-0009 · ≈ $40–70  [Review]
   REQ-15  Weekly digest by email                Eden asks: "Daily or weekly?"       [Answer]
 WITH AGENTS · 2
   REQ-16  Keyboard shortcuts cheat sheet        Eden · scoping · 20 min
   REQ-10  Checklists on tasks                   codex-dev · scoping · 1 h
 NOT ROUTED · 1
   REQ-9   Dark mode for the docs reader         saved 2 d ago                         [Route]
 APPROVED · 1 (collapsed)
   REQ-14  Quick capture of feature requests     → FEAT-0007 · 4 of 6 tasks done
```

- **Groups:** Needs you (scoped and waiting for approval, or a question for the owner) · With agents · Not routed · Approved (collapsed) · Declined and merged (collapsed).
- **Needs you** items also appear in the owner's Home › Needs you.
- **Row:** number · title (the first line of the text, or a title the agent gave it) · state in words · who · age · the next action.

## 3. Flow 3 · A request's page

- **Main column:** the original text (as typed, with attachments), then the conversation: the scoping agent's questions and the owner's answers, notes and changes of route ("You moved this from ada to Eden").
- **Side column:** state · kind · project · who scopes it (**Change…**: the same suggestion list) · raised by and when · **Outcome** (once scoped: spec, design or ADR links, proposed tasks, estimate).
- **Owner's actions by state:** New → **Route…** · With agent → **Change…**, **Message <agent>** · Scoped → **Approve and create 6 tasks** (opens the task drafts, each editable or removable, then **Create tasks**), **Ask for changes…**, **Decline…** · any state → **Merge into…** (another request), **Decline…** (reason required).
- **Agent side (CLI):** `org-agent request show|accept|ask|reroute --to <member> --reason|scope --doc <id> --task "<title>"…|decline --reason|merge --into REQ-n`, plus `request add` to raise one. The words match the app's.
- **Approved:** the tasks appear in the project (To do) linked to the request ("from REQ-12"). The request shows their progress ("4 of 6 done") and the **feature** tracks the build from there (`features-ux.md`). The request itself ends at Approved.

## 4. States and edge cases

| Case | Behavior |
| --- | --- |
| The suggested agent is asleep (on call) | It's woken by the request (run modes RUN-3). The panel says nothing extra. The request shows "Eden is waking up". |
| The chosen agent is Offline or paused (Riley) | Shown in the list with its status, and choosing it warns: "Riley is offline and won't pick this up. Choose someone else or Just save." |
| The agent reroutes it | The owner sees "Eden sent REQ-12 to ada: 'mostly the office protocol'" in the request and Activity. No approval is needed for a reroute. |
| No answer from the scoping agent (on call, failed wakes) | After the run-mode failure rules, Needs you shows "REQ-12 is stuck with Eden", with **Change…**. |
| Duplicate of an open request | While typing, up to 2 similar open requests show ("Looks like REQ-9: Dark mode…"), with **Add to REQ-9** (appends the text as a comment). |
| Owner offline / app closed | The shortcut still opens the capture panel (the app runs in the menu bar). Saving works locally, and the office routes it. |

## 5. UX acceptance criteria (RQS)

Criterion ids use **RQS-** so they never collide with request records (`REQ-n`) *(renamed in rev 1a, before any test used them)*.

- RQS-1 · ⌥⌘N opens the capture panel from anywhere, with the app window closed, with the text field focused. The shortcut can be changed in Settings, and when another app already uses it, Settings says so and asks for another.
- RQS-2 · ⌘Enter with only text saves a request. Project, kind and route are suggested and editable, and none is required.
- RQS-3 · The panel shows a suggested agent and two alternatives, each with a one-line reason, and the owner can pick another, type @name or Just save.
- RQS-4 · Starting the text with @name sends it to that member without a suggestion. Message instead sends it as an ordinary message.
- RQS-5 · Without the copilot, the suggestion follows the kind rules and is marked "Suggested by rule".
- RQS-6 · Text that reads like a bug offers Report a bug instead, carrying the text over.
- RQS-7 · Work has a Requests pill grouped as Needs you, With agents, Not routed, Approved, and Declined and merged. Scoped requests and agents' questions appear in Home › Needs you.
- RQS-8 · A routed request wakes an on-call agent and appears in its inbox. Choosing an offline or paused agent warns first.
- RQS-9 · A scoped request shows the linked docs, the draft tasks and the estimate. Approve creates the tasks (each editable or removable first), linked "from REQ-n".
- RQS-10 · Reroutes, questions, answers, declines and merges are recorded on the request and in Activity, with who and why.
- RQS-11 · The feature a request produced or extended lists it in its front matter (`requests:`) and quotes its original text.
- RQS-12 · Agents can raise, accept, ask, reroute, scope, decline and merge requests with `org-agent request …`, with the same words as the app.
- RQS-13 · Similar open requests show while typing, with Add to REQ-n.

## 6. Open questions

**For ada and codex-dev**
- ~~A29~~ · *Resolved (ada):* a request is a work-item kind on the shared core (REQ-n, a state machine, a thread of messages). Approved creates the linked tasks in one command. codex-dev's W7 model covers it. Plan row W16: codex-dev (model), Remy (CLI), app-frontend-dev (UI).
- ~~A30~~ · *Resolved (ada):* the Tauri global-shortcut plugin in the menu-bar app. It's configurable, and if another app owns the shortcut, Settings shows "⌥⌘N is used by another app. Choose another shortcut." (RQS-1).
- ~~C22~~ · *Resolved (ada):* smart feature #16, with the rule fallback when no runtime is set up.

**For the user**
- U21 · ⌥⌘N as the default shortcut (⌘N is New in many apps, and ⌥⌘N is rarely taken)? And is an approval step before tasks are created what you want (my recommendation), or should a scoped request become tasks straight away?
