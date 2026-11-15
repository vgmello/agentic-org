> **Later product decision.** This imported source preserves the earlier private-note deferral and access model. The newer [private task-note requirements](../design/organization-data/05-operational-data.md#private-notes-attached-to-tasks) select private task notes as required and define the current proposed privacy/storage contract. Those requirements supersede conflicting scope and access statements below; their runtime implementation remains pending.

# Tasks: follow what you asked for, and work agents can run on

Emery (`ac-product-manager-emery`) · **Rev 1 (2026-11-12): decided.** The owner asked for this in session on 2026-11-12, and delegated the product calls to me.
**Rev 12 (2026-11-13): v1.0 scope.** Following ada's sizing decisions (Rowan's `v1-prd-sizing.md`), TK-5 Review is v1.1 (§8), and D26's option (a) stands until then. Its CLI verbs move from AC-TK10-1b (Next) to a new AC-TK10-1c that ships with TK-5. No requirement changed.
**Rev 2 (2026-11-12):** any project member may create a task for anyone on the project (§4.4). "The assignee's manager" and the project manager are defined once (§4.2). Waiting on you shows under every Following filter, and a row's unseen state covers its nested subtasks (§4.1). Needs me takes every wait. Deleting an open question ends its wait, and system comments are locked. A promoted step's tick is derived. `tasks list --created-by`. Cycle and lead time skip Done and Cancelled spans (T8).
**Rev 3 (2026-11-12):** who may add a subtask or promote a step (§4.6, AC-TK8-8) is separate from whom a new task may be assigned to (§4.4, AC-TK6-6). "The assignee's manager" and "the project manager" have the same rights everywhere: reassign, cancel, reopen, add a subtask and promote a step, but not start, block or mark Done (§4.2). A Hand-off opens whenever someone other than the owner creates a task assigned to the owner, whoever Asked by is (§4.4). Opening a task clears its unseen changes and its nested subtasks'. The badge counts tasks with unseen changes, each once (§4.1).
**Rev 4 (2026-11-12):** A Hand-off is defined by its opening event: someone other than the owner assigns a task to the owner, by creating it (a subtask or a promoted step included), assigning it from unassigned, reassigning it or reopening it (§4.4). The task records that its Hand-off is open (§6.13). Only the assignee or the owner can start with `reopen --start` (§5). Unseen state is defined once, against "last seen" (§4.1). Work gets a Followed filter (§4.1, AC-TK2-3). "Budget reached" is a system move to Blocked (§4.2). Other than the owner, Asked by and the managers can't start, block, unblock or mark Done another member's task (§4.2).
**Rev 5 (2026-11-12):** "Budget reached" is a fifth way a task waits on the owner (§4.1). When a budget stops an In progress task, it moves to Blocked with no actor, and moves back to In progress when the owner raises the budget or allows once (§4.2). A To do or In review task gets a budget hold instead: its status stays, and agent turns on it are refused. Reassigning doesn't lift a block or hold, and cancelling clears it (AC-TK4-18, AC-TK4-19). The transition table now lists the cancel cascade and `reopen --with-subtasks` as system moves. The owner running `assign --none` clears a Hand-off (§4.4). An open question on a Done task stays in Waiting on you until answered or deleted (§4.8). Changing the reviewer is `tasks review --reviewer <member>` (§5).
**Rev 6 (2026-11-12):** A budget stop (block or hold) has its own "Budget reached" Inbox row, with Raise budget, Allow once and Stop. AC-TK4-3's withdrawal never applies to it (§4.1, §6.13). Subtasks stopped by their parent's budget get no Inbox item; their Waiting on you row links to the parent's. Reassigning a task blocked "Budget reached" moves it to To do and turns the block into a budget hold (§4.2, AC-TK6-4). The owner or the project manager may resolve a task budget, and a cost record moved out of the task can lift it. A top-level task Blocked on someone else keeps its waiting-on and gets the hold. A system move counts as an unseen change unless the owner's action caused it (§4.1). The unblock refusal is no longer called an exception. Asked by and the managers wording is simpler (§4.2).
**Rev 7 (2026-11-12):** A held task can still be finished by a person: Approve and Done are allowed on a held In review or To do task, and reaching Done clears the hold and its Budget reached Inbox item. Agent turns stay refused (§4.2, AC-TK4-19, §6.2). Clearing the budget is also a way out of a budget stop. A move back caused by a budget action or a moved-out cost record is named in the row. `block --on` and `unblock` refusals name "Budget reached or Parent budget reached". A held task can't be started or unblocked into In progress until the budget is resolved, but a held To do task's Hand-off can be accepted. "Seen" for Done earlier means no unseen changes. Cancelling a task ends an open question's wait (§4.1, §4.8, AC-TK7-8).
**Rev 8 (2026-11-12):** On a held In review task, request changes (human reviewer or owner) and withdraw (human assignee) move it to Blocked "Budget reached"; an agent assignee can't withdraw (§4.2, AC-TK4-19). A tick on a held task is recorded but doesn't auto-start it (§4.2, AC-TK4-5). A task already Blocked on the owner keeps its block, gets the hold and the Budget reached item. Reopening a task at or over its budget re-applies the stop at once, by its new status (`costs-and-budgets-v1` requirement 18). The owner or the project manager resolves the budget. The "cancelling ends a question's wait" sentence moved from AC-TK1-2 to AC-TK7-8. Unblock or start on a held task is refused with "T-12 is on budget hold: the owner or project manager must raise it." (§5, AC-TK10-4).
**Rev 9 (2026-11-12):** Only a person (the owner or a human project manager) raises, clears or allows once on a task budget; an agent project manager is refused (`costs-and-budgets-v1` requirement 14), and so is its move of a cost record off a stopped task (requirement 33; §4.2, §6.2, AC-TK4-18, AC-TK4-19). The held-task refusal now reads "T-12 is on budget hold: a person must raise it, allow 20% once or clear the budget." (§4.2, §5, AC-TK10-4).
**Rev 10 (2026-11-12):** "Parent budget reached" is covered wherever "Budget reached" is in reassign and send-back: a reassigned parent-held subtask keeps the parent's stop, and a sent-back one goes to Blocked "Parent budget reached", neither with an Inbox item of its own (§4.2, AC-TK4-19, AC-TK6-4). A Raise or Allow once still at or below spent is refused; lowering a reached budget is accepted and the task stays stopped; deleting the task or moving a cost record off it are ways out (§4.2, AC-TK4-18). An agent project manager's cost-record move off a stopped task is refused (§6.2, AC-TK4-18). When the budget resolves, a task Blocked on someone else stays Blocked on them (§4.2, AC-TK4-19). Rev 9's citations corrected. Later the same day: the agent project manager's refused cost moves cover any move that lowers a reached budget's spent, subtasks included, and AC-TK4-18 reads "raises, clears or allows once" (§4.2, §6.2, AC-TK4-18).
**Rev 11 (2026-11-12 late): staff review.** Ruling: a parent can't be marked Done or approved while any subtask is open; this is a code change, since the code blocks only on required steps (§4.6). Later the same night: its refusal reads "T-12 has 2 open subtasks: T-13, T-14. Finish or cancel them first." (§4.2 Done row, §4.6, §5, AC-TK5-2, AC-TK8-3, AC-TK10-4). With `costs-and-budgets-v1` Rev 13: a task Blocked on the owner stays Blocked on the owner when its budget resolves and keeps its "Blocked on you" item, and a top-level task Blocked on someone else gets the one Budget reached item (§4.2 Budget hold, AC-TK4-19); refusal texts for an agent's Done, Approve and withdraw on a held task and for `unblock` or `block --on` on a Budget reached block (§4.2, AC-TK4-18, AC-TK4-19, AC-TK10-4); a person's unit change to a value above spent is a way out, like a Raise (§4.2). Final pass: the unit change in §4.2's way-out sentence; one refusal check order (§5, AC-TK10-4); on a held task, ticks and unblocking are a person's (§4.2); the Parent budget reached refusal (§4.2, AC-TK4-18, AC-TK10-4); the author or the owner redacts any comment or note version, and "the owner can still read the original" now excepts a redacted one (§4.5, AC-TK7-4, AC-TK10-2, §5).
Labels: **[F]** fact · **[A]** assumption · **[O]** my call.
Related:
- `information-architecture-v2.md`, the IA. This PRD adds one sidebar item, "Following" (§4.1), and a Home card. It amends the sidebar order (AC-IA01-1), the Home order (D1, AC-IA04-1), Work's views (AC-IA18-1) and, once TK-5 ships, D26's Review requested. All of this is recorded as D38.
- `product-vision-and-ux-lens.md`, the lens every story here must pass.

## 1. The problem

**[F] The owner, 2026-11-12:**
- "I might ask you to do something, and eventually come back and want the status of the thing I asked."
- The Inbox is for what needs my attention. Tasks I created are a different list.
- Tasks need more statuses than started and closed: blocked, in review and so on.
- One level of subtasks, and no deeper.
- Agents shouldn't have to announce "I started" or "I finished". Capture the times and the actor behind the scenes.
- Reassignment must keep the record.
- An agent's own working notes on a task are private to that agent, though visible to the owner. They don't pass to the next agent, which starts from the original requirements plus the comments.
- Agents work tasks through an `org-agent tasks` CLI, with create, read, update and delete for comments and notes.
- Record everything.

**[F] What exists today** (product repo `origin/v1`; `W` is `crates/org-proto/src/work.rs`):

| Exists | Missing |
| --- | --- |
| `created_by` (W:383) and a `work.list` filter on it (W:1088). | A "Created by me" view. The UI's "Mine" means assigned to me (`apps/org-ui/src/lib/work/saved.ts:23`). |
| Task statuses todo, in_progress, blocked and done, plus a `resolution` (W:184-211). | In review, and a reviewer. |
| An immutable `WorkEvent` log with actor and time. The server sets the time and the caller can't supply it (W:49, W:761-777). | A points field on tasks. |
| `Assigned {from, to}` events (W:814). | The `org-agent` task CLI. It's designed (`docs/design/tasks-projects-docs.md:817`) but not built. |
| Checklist steps, and `make_task` linking a step to a new task with `part_of`. There's no depth limit (W:262, W:1629). | Private notes. |
| `work.watch`. The creator watches automatically; a commenter watches only when they choose to (`work.comment { watch }`, W:1517-1531). Watchers are built and stored with the item (`org-work/src/tasks.rs:147`, `items.rs:110-117`); only watcher notifications are still design only (`tasks-projects-docs.md:971`). | Any record of who asked for a task when someone else typed it; any per-owner "last seen"; a `work.list` filter by watcher or Asked by (W:1088-1115; W:73 says phase 1 can't list the other watchers). |
| `done` with resolution completed or cancelled (W:208-213); `CommentKind` note, question or answer (W:291). Only *required* steps block Done, unless `done --anyway --reason` is used (design §5). | Comment edit and delete; anything private. |

**[O] Why it matters.** Delegation only feels safe when the person delegating can see, without asking, where everything they handed off stands. Today the owner has to ask in chat, which costs an agent a turn, or dig through Work. That's the core loop of running a long-lived team of agents, and it's missing.

## 2. Users and jobs

- **The owner:**
  - "Where is the thing I asked for?"
  - "What came out of it?"
  - "Is anything stuck, and on whom?"
- **An agent:**
  - plan the work as steps;
  - work through them;
  - leave a record others can pick up;
  - keep its own scratch thinking out of everyone else's way;
  - spend as few tokens as possible on bookkeeping.

## 3. Goals, non-goals and success

**Goals:**
- Every request the owner makes is findable in **two clicks** from any page, along with its status and outcome.
- Task bookkeeping costs an agent **one CLI call per real event**: start, step done, blocked, done. Times and actors are never typed.
- A task's history is complete and tamper-evident.
- Private notes never reach another member. This depends on ADR-0003's M-A for agents started by hand (§6.4).

**Non-goals:**
- subtasks of subtasks;
- several assignees on one task;
- sprints and boards beyond today's;
- due dates on tasks;
- time tracking typed by people;
- notifications outside the app.

**Success**, measured 30 days after release:
- At least 90% of the owner's chat requests for work become tasks with the owner as **Asked by**. This is measured on a monthly sample of 30 owner messages that ask an agent for work, checking each for a task with `--from-message` pointing at it.
- The owner's "status?" chat messages to agents fall by half. **[A]** This needs a before-count from chat history.
- Zero private-note leaks, verified by a test across every read path (§6.4).

## 4. The experience

### 4.1 Following: what you asked for, in one click

**[O] Decision T1.** A new sidebar item, **Following**, sits directly under Inbox. Inbox and Following are two lists with two urgencies:

| | **Inbox** | **Following** |
| --- | --- | --- |
| Question it answers | "What needs me now?" | "Where are the things I asked for?" |
| What's in it | Approvals, questions, decisions, problems | Tasks you asked for or created, plus tasks you watch |
| Badge | amber count (waiting on you) | neutral indigo count of **tasks with unseen changes** (never amber). Each task, subtasks included, counts once. |
| How items leave | when they're resolved | when they're done and you've seen them, they move to "Done earlier" |

**Why a sidebar item, and not only a Work view:**
- Following is the owner's second most frequent loop, after the Inbox. A view three clicks deep fails the two-click goal.
- The badge tells the owner that something they care about moved, without making it urgent.
- A Work view alone would also bury the outcome.

**The Following page:**
- **Groups**, in this order:
  - Waiting on you: followed tasks whose state waits on the owner. **A task waits on the owner** in exactly five cases, used everywhere in this PRD: it's blocked on the owner, the owner is its reviewer, a Hand-off to the owner is open (§4.4), a question to the owner is open, or its budget is reached (a Budget reached block or hold, §4.2). Each of the five has an Inbox item too: Blocked and Hand-off from their rows (§6.13), the question from its row (FREEZE:73, slice IA-10c), the review from its row (TK-5), and the budget stop from its own Budget reached row, with Raise budget, Allow once and Stop (`costs-and-budgets-v1`, §6.13). A subtask stopped by its parent's budget ("Parent budget reached") has no Inbox item of its own: its Waiting on you row links to the parent's item. They're shown for context, with no inline actions. Each row links to the Inbox filtered to that task, so acting stays in one place, even when a task has more than one open item. This group's header and row marker may use amber, because it waits on the owner. The sidebar badge stays neutral.
  - Blocked;
  - In review (hidden until TK-5 ships);
  - In progress;
  - To do;
  - Done (unseen);
  - Done earlier (collapsed): followed tasks closed (Done or Cancelled) 0 to 30 days ago that the owner has seen, measured from their close time (Done time, or Cancelled time). "Seen" means the task has no unseen changes; a task the owner closed themselves goes straight to Done earlier. Unseen ones stay in the Done group. Older ones are reached through Work's filters (Created by me, Asked by me or Followed, with Done), or through Work's All view with filters.
- **Each row shows:**
  - the title and the project;
  - the assignee's avatar and name;
  - the status in words, with an icon;
  - steps as "3/5", and on a parent, "Subtasks 1/2" next to them;
  - the last change in one line, such as "Kai moved it to In review, 2 h ago";
  - for done tasks, the **outcome line** and its first link (§4.7).
- **Subtasks:** a followed parent's subtasks are followed with it. They show nested under the parent's row, not as rows of their own. A change to a subtask counts as a change to the parent's row: it marks the row unseen and updates its last change. A subtask the owner follows without following its parent shows as its own row, labelled "Subtask of T-9". **One exception to "each task appears once":** a subtask that waits on the owner (any of the five cases above) also shows as its own row in Waiting on you, labelled "Subtask of T-9", even when its parent is followed.
- **A nested subtask** is followed through its parent. It can't be unfollowed on its own: Unfollow on its page is disabled, with "Unfollow T-9 to stop following this subtask". A subtask followed on its own, with its own row, can be unfollowed like any task.
- **Unseen changes** show as a bold row plus a dot. **Unseen is defined once, against "last seen"** (§6.7): a change is unseen when someone other than the owner made it after the task's last seen time. A system move counts as an unseen change unless the owner's own action caused it (AC-TK1-7). **Last seen** is set by opening the task, or by Mark all as seen. A row's unseen state is any unseen change to the task or to its nested subtasks. The owner's own changes never count. Opening a task, parent or subtask, sets last seen on that task and its nested subtasks, which clears their unseen changes on every row they marked. Opening a nested subtask clears its changes on its own row and on its parent's row. Example: subtask T-12 under T-9 becomes blocked on the owner, and the owner opens it from the Inbox: its Waiting on you row and T-9's row both clear, and the badge is 0 if nothing else is unseen. If the owner opens T-9 instead, both rows clear too.
- **The badge counts tasks with unseen changes.** Each task, subtasks included, counts once. A parent marked only by its subtasks' changes isn't counted on its own. Example: two nested subtasks of T-9 change, and the badge is 2. A change to a nested subtask that waits on the owner marks its own Waiting on you row and its parent's row, and adds one to the badge for that subtask.
- **Filters:** Asked by me · Created by me · Followed (followed by hand) · All. "All" means every task you follow, and it's the default, so whatever the badge counts is on the first screen. The other three narrow it down, except that a task waiting on you shows in Waiting on you under every filter, even one it wouldn't otherwise match. Tasks that wait on you are followed automatically (§6.12). A task kept followed after its wait ends (§6.12) appears under All, and not under Asked by me, Created by me or Followed unless those also apply.
- **Mark all as seen** sets last seen on every followed task at once, which clears every unseen state.
- **Following a task:** any task page has a **Follow** button, which becomes **Following** once pressed; it's `work.watch` on the wire. Tasks you ask for or create are followed automatically. "Follow" is the only verb shown to the owner; "watch" never appears.
- **States:**
  - **Empty:** "Nothing to follow yet. Tasks you ask for show up here." Each filter has its own empty state, such as "Nothing you created is open."
  - **Loading:** skeleton rows.
  - **Error, or the office unreachable:** the shared error and offline patterns in the DS, showing the last loaded list marked "As of {time}".
  - **Cancelled tasks** sit in "Done (unseen)", marked Cancelled, until the owner has seen them. Then they move to "Done earlier", measured from their Cancelled time.
  - The Home card is hidden when there's nothing to follow.
- **Clicking a row** opens the task, scrolled to the first change since your last visit, under a divider: "New since you last looked".
- **Home** gains a **Following** card with the five most recently changed rows, after Needs you and before Projects. The Inbox stays first.
- **Work** gains the views **Created by me** and **Asked by me**, and a **Created by ▾** filter for any member, person or agent. It also gains a **Followed** filter: tasks the owner follows by hand, in any status, so a task followed by hand stays findable after it leaves "Done earlier".

**[O] Decision T2: Asked by.** A task records **who asked for it**, separately from who typed it.
- When an agent creates a task from the owner's chat message, it passes the message ID.
- The server sets **Asked by** to that message's author. The agent can't type a name.
- **The message must have been addressed to that agent:** a direct message, or a thread or channel message that mentions it. That stops an agent citing any message it can merely read.
- Without a message, Asked by is the creator.

### 4.2 Statuses

**[O] Decision T3.** Six statuses, always shown as an icon plus a word. In review arrives with TK-5 (§8):

| Status | Meaning | Required to enter it |
| --- | --- | --- |
| **To do** | not started, or just reassigned | — |
| **In progress** | the assignee is working on it | It's entered when the assignee starts, or automatically on any tick by the assignee while the task is To do (AC-TK4-5). |
| **Blocked** | can't move | **Waiting on** (a member, or "outside") and a **reason**, which every block needs. If the owner is the one waited on, an Inbox item opens (D26). |
| **In review** | the work is finished and waits for a reviewer. It's optional unless the task has Needs review (§4.3). | a **reviewer**, who isn't the assignee (the default is Asked by), and the **outcome line** |
| **Done** | finished | **Done's guards**, on every route to Done: an **outcome line** (carried over from review), every *required* step ticked, and no open subtasks (refused, listing them by key: "T-12 has 2 open subtasks: T-13, T-14. Finish or cancel them first."). While Needs review is set, the only way in is In review → Approve; every other move to Done is refused. Optional steps left open are listed under the outcome as "Not done". `--anyway --reason` overrides only unticked required steps, which are recorded as done anyway. It works on every route to Done, including a reviewer's Approve. It never overrides open subtasks or Needs review. **[F]** It exists today (W:1309-1314, `done_anyway`). |
| **Cancelled** | won't be done | a reason |

- Display names change: "todo" becomes **To do**. There's no separate "Pending" status: To do is pending.
- Today Cancelled is a resolution of Done (`done --cancelled`). That can stay; the owner just sees it as a distinct status.
- Any open status can go to Cancelled. Done can't be cancelled: reopen it first. **Who can cancel:** Asked by, the owner, the assignee's manager or the project manager (both defined below). The assignee can cancel only a task they asked for themselves; otherwise they ask, with a question comment to Asked by. `done --cancelled` is the same move as Cancel, with the same actors.
- Done and Cancelled can be **reopened**, which needs a comment, and the task returns to To do.
- **Who can reopen:** for a Done task, Asked by, the owner, the assignee's manager, the project manager, or the last assignee. For a Cancelled task, only Asked by, the owner, the assignee's manager or the project manager, not the last assignee. Reopening keeps the last assignee.
- **Reopening to the owner.** When the last assignee is the owner and someone else reopens the task, a Hand-off to the owner opens (§4.4, AC-TK6-5). When the owner reopens it, no Hand-off opens.
- **Reopening keeps the Needs review flag** and voids any earlier approval. A reopened task with the flag set reaches Done again only through a new In review → Approve.

**[O] Transitions.** The table lists the manual moves, who makes each, and what it needs, plus the system moves (no actor, or the caller's action as cause). Anything not listed is refused.

| From | To | Who | Required |
| --- | --- | --- | --- |
| To do | In progress | the assignee | `start`, or automatically on any tick by the assignee while the task is To do (AC-TK4-5). When the owner accepts a Hand-off, the Accept counts as the owner's start (AC-TK6-3). |
| To do | To do (assign or reassign) | as §4.4: the assignee, Asked by, the owner, the assignee's manager or the project manager; on an unassigned task, as AC-TK6-1 | `assign`, with an optional reason. `assign` on a Done or Cancelled task is refused: "Reopen it first". |
| To do or In progress | Blocked | the assignee | waiting on, and a reason |
| In progress | Blocked (Budget reached) | the system, no actor | waiting on the owner, reason "Budget reached", or "Parent budget reached" for a subtask of a stopped parent (`costs-and-budgets-v1`). See the system moves below. |
| Blocked (Budget reached or Parent budget reached) | In progress | the system, caused by a budget action (the owner or a human project manager) or a moved-out cost record | the owner or a human project manager raises the budget, allows once, clears (removes) the budget or changes its unit to a value above spent on a task budget, or a cost record moved out of the task lifts it. An agent project manager can't, and can't move a cost record that lowers the spent of a reached budget either (off the stopped task, any of its subtasks, Done or Cancelled included, or a subtask whose parent's budget is reached): "Only a person can raise, clear or extend a budget." (`costs-and-budgets-v1` requirements 19, 23, 33). A Raise or Allow once whose new limit is still at or below spent is refused and changes nothing; lowering a reached budget is accepted, and the task stays stopped. Deleting the task, or a person moving a cost record off it, also ends the stop (`costs-and-budgets-v1` requirements 22-23, 33). The timeline shows the move back as caused by that action, for example "Resumed: budget raised by Ana". |
| Blocked | Blocked (change waiting on) | the assignee | `block --on` again, with a new waiting on and a reason. The task stays Blocked; the timeline records the old and new waiting on. Refused on a Budget reached block: "T-12 is Blocked "Budget reached": a person must raise it, allow 20% once or clear the budget."; on a subtask's Parent budget reached block: "T-13 is Blocked "Parent budget reached": a person must raise T-12's budget, allow 20% once or clear it." |
| Blocked | the status it had before it was blocked (To do or In progress) | the assignee | `unblock`; it clears waiting on. Refused on a Budget reached or Parent budget reached block (with the `block --on` row's texts), and into In progress while the task has a budget hold: "T-12 is on budget hold: a person must raise it, allow 20% once or clear the budget." On a held task, unblocking into To do is for a human assignee or the owner only: an agent's unblock is a turn, and is refused with the same hold text (`costs-and-budgets-v1` req 19). |
| In progress | In review | the assignee | a reviewer and the outcome line (§4.3). It's the only way to Done when Needs review is set. |
| In review | In review (change the reviewer) | the owner, Asked by or the assignee | a new reviewer, never the assignee. Recorded as "Reviewer changed from A to B by C". |
| In review | In progress | the reviewer, or the owner unless the owner is the assignee (request changes); or the assignee (withdraw) | a comment. On a held task (budget hold), only a human reviewer or the owner may request changes, and the task then moves at once to Blocked "Budget reached" as a system move (the hold becomes a block; costs-and-budgets-v1 req 19). A human assignee's withdraw on a held task behaves the same way: it moves at once to Blocked "Budget reached". For a subtask held by its parent's budget, either move goes to Blocked "Parent budget reached" instead, with no Inbox item of its own. An agent assignee can't withdraw a held task: "T-12 is on budget hold: only a person can withdraw it from review." |
| In review | Blocked | nobody | refused: withdraw to In progress first |
| In progress | Done | the assignee | Done's guards (the Done row above): the outcome line, every required step ticked, no open subtasks, and refused while Needs review is set |
| In review | Done | the reviewer (approve), or the owner unless the owner is the assignee | Done's guards. Approve is the one route to Done while Needs review is set. Allowed on a held task for a human reviewer or the owner (see Budget hold); reaching Done clears the hold. An agent reviewer's Approve on a held task is refused: "T-12 is on budget hold: only a person can approve it." |
| To do | Done | the assignee | Done's guards, so it's refused while Needs review is set. Refused on an unassigned task: assign it first. Started and Done are recorded at the same moment ("Started and done"). Allowed on a held task for a human assignee or the owner (see Budget hold); reaching Done clears the hold. An agent's Done on a held task is refused: "T-12 is on budget hold: only a person can mark it Done." |
| In progress, Blocked or In review | To do (reassign) | as §4.4: the assignee, Asked by, the owner, the assignee's manager or the project manager | `assign`, with an optional reason. It clears the reviewer and "waiting on" (§4.4). **Exception:** a task blocked "Budget reached" or "Parent budget reached" moves to To do and the block becomes a budget hold: the wait, the Inbox item and the Needs me entry stay (§4.2). A reassigned subtask held by its parent's budget keeps the parent's stop: it gets no Inbox item of its own, and the parent's item stays. |
| any open status | Cancelled | as above: Asked by, the owner, the assignee's manager or the project manager; the assignee only on a task they asked for. `done --cancelled` has the same actors. | a reason |
| Done | To do (reopen) | Asked by, the owner, the assignee's manager, the project manager, or the last assignee | a comment. `--start` is allowed for the assignee or the owner (§5). If the last assignee is the owner and someone else reopens, a Hand-off to the owner opens (§4.4). |
| Cancelled | To do (reopen) | Asked by, the owner, the assignee's manager or the project manager | a comment. The same `--start` and Hand-off rules apply. |
| any open subtask | Cancelled (cancel cascade) | the system, with the parent's canceller as the actor | the parent was cancelled (§4.4). Not checked against the subtask's own Cancel actors. Shown as "Cancelled with T-9 by Kai". |
| Cancelled subtask | To do (`reopen --with-subtasks`) | the system, with the caller as the actor | the caller may reopen the parent, and the subtask was cancelled by that parent's cascade. Not checked against the subtask's own Reopen actors. |

- **[F]** Blocking from To do is a new move. Today's machine is `todo → in_progress ⇄ blocked → done` (W:1309).
- **[O]** `unblock` returns the task to the status it had before it was blocked. This supersedes the design's default of To do with `--start` (`tasks-projects-docs.md:823`). `--start` stays accepted, and starts the task if it returns to To do.
- **The owner** may make any move the assignee can, recorded as made by the owner; separately, nobody approves their own work, the owner included. One exception: only the assignee's tick auto-starts a task. Any tick by the assignee while the task is To do moves it to In progress, the owner's included when the owner is the assignee. The owner's tick on someone else's task doesn't start it. Asked by and the managers, unless they're the owner, can't start, block, unblock or mark Done another member's task. `done --cancelled` isn't marking Done: it's Cancel, with Cancel's actors.
- **Unassigning** (`assign --none`) is allowed only while the task is To do, by anyone who may reassign (§4.4).
- **An unassigned To do task can't be started, blocked or marked Done** until someone is assigned. To pick it up, a member assigns it to themselves, then starts it.
- **Two terms, defined once.** **The assignee's manager** is the member the assignee reports to, or the owner when the assignee reports to no one. For a Done or Cancelled task, it's the last assignee's manager. **The project manager** is the owner, or a lead of a team attached to the project (`projects-planning-v1.md` §4.7). Both managers have the same rights everywhere in this PRD: reassign, cancel, reopen, add a subtask and promote a step. Neither can start, block, unblock or mark Done another member's task. For an unassigned task, see AC-TK6-1.
- **Assigning an unassigned task** is open to Asked by, the owner, the project manager, or any member assigning it to themselves. If the project has no attached team, only the owner and Asked by can assign it to someone else (AC-TK6-1).
- **The cancel cascade is a system move.** A subtask cancelled because its parent was cancelled (§4.4) isn't checked against the subtask's own Cancel actors. It's recorded as an automatic event, with the actor who cancelled the parent, and shown as caused by the parent's cancel: "Cancelled with T-9 by Kai". **[F]** Today's system cascades carry no actor (W:766-767); recording the caller is a change for Rowan.
- **"Budget reached" is a system move** (`costs-and-budgets-v1`). An In progress task (or subtask) moves to Blocked, waiting on the owner, with the reason "Budget reached" ("Parent budget reached" for a subtask of a stopped parent) and no actor. The stop has its own "Budget reached" Inbox row (§6.13).
  - **Moving back.** When the owner or a human project manager raises a task budget, allows once, clears it or changes its unit to a value above spent (like a Raise), or a cost record moved out of the task drops it below the limit (`costs-and-budgets-v1` requirements 19, 23, 33), the task moves from Blocked back to In progress as a system move, recorded as caused by that action. **Only a person lifts a budget:** an agent project manager may set a first budget or lower one, but raising, clearing, Allow once and a cost-record move that lowers the spent of a reached budget (off a stopped task, any of its subtasks, Done or Cancelled included, or a subtask whose parent's budget is reached) are refused for it: "Only a person can raise, clear or extend a budget." (`costs-and-budgets-v1` requirement 14). The assignee can't `unblock` or `block --on` a Budget reached or Parent budget reached block: "T-12 is Blocked "Budget reached": a person must raise it, allow 20% once or clear the budget." (for a subtask's Parent budget reached block: "T-13 is Blocked "Parent budget reached": a person must raise T-12's budget, allow 20% once or clear it."). A person's budget action is the way out. **[O]** `unblock` by hand is refused for everyone, the owner included, because the budget is the cause. The way out is Raise budget, Allow once or clearing (removing) the budget, a person changing the budget's unit to a value above spent in the new unit, a person moving a cost record off the task, or cancelling or deleting the task. A Raise or Allow once whose new limit is still at or below spent is refused and changes nothing. Lowering a reached budget is accepted, and the task stays stopped.
  - **Budget hold.** A To do or In review task (or subtask) keeps its status, and its reviewer and review stay. Agent turns on it are refused until a person (the owner or a human project manager) resolves the budget. **A person can still finish it:** Approve (by a human reviewer or the owner) and Done (by a human assignee or the owner) are allowed on a held In review or To do task, because a Done task isn't stopped (`costs-and-budgets-v1` AC-CB6-14). Reaching Done clears the hold and its Budget reached Inbox item. An agent is refused: Done with "T-12 is on budget hold: only a person can mark it Done.", Approve with "T-12 is on budget hold: only a person can approve it." and withdraw with "T-12 is on budget hold: only a person can withdraw it from review." A held task can't be started, or unblocked into In progress, until the budget is resolved; a held To do task's Hand-off can still be accepted, and it stays To do and held. On a held task, ticking is for a human assignee or the owner only: their tick is recorded but doesn't auto-start the task (AC-TK4-5). An agent's tick is a turn, and is refused with "T-12 is on budget hold: a person must raise it, allow 20% once or clear the budget."; so is an agent's unblock into To do. A held In review task that is sent back (request changes by a human reviewer or the owner, or withdraw by a human assignee) moves at once to Blocked "Budget reached" (the hold becomes a block); a subtask held by its parent's budget moves to Blocked "Parent budget reached" instead, with no Inbox item of its own. A subtask, or a top-level task, already Blocked on someone else keeps its waiting on and also gets the hold (a top-level task also gets the one Budget reached item); when the budget is resolved, the hold lifts and the task stays Blocked on them. A task already Blocked on the owner when its budget is reached keeps its block, gets the hold, and gets the Budget reached item too, and keeps its "Blocked on you" item; when the budget is resolved, the hold and the Budget reached item clear, and it stays Blocked on the owner with its "Blocked on you" item. **Reopening** a task whose spend is at or over its budget re-applies the stop at once, by its new status: a hold on To do, or a block if it's started (`costs-and-budgets-v1` requirement 18: budgets are checked on every change).
  - **A fifth wait.** A Budget reached block or hold waits on the owner: it opens a Budget reached Inbox item (§4.8), which AC-TK4-3's withdrawal never touches, shows in Waiting on you and Needs me, and follows the owner (§6.12).
  - **Reassigning** a task blocked "Budget reached" or "Parent budget reached" moves it to To do, as any reassign of a Blocked task, and turns the block into a budget hold: the wait, the Inbox item and the Needs me entry stay. A reassigned subtask held by its parent's budget keeps the parent's stop: it gets no Inbox item of its own, and the parent's item stays. When the budget is resolved, the hold lifts and the task stays To do for the new assignee to pick up. Reassigning a task that already has a hold doesn't lift it. Cancelling clears a block or hold.
- **Undo window.** **[F]** Automatic changes can be undone for an hour (W:770, `work.undo`). It refuses a manual event and a cascade, but it can undo any other automatic event, an auto-start included (W:1561-1564). **[O]** Undoing an auto-start is refused, with "Reassign or cancel instead". Started and Picked up stay. This is a change for Rowan. No other new undo is added. Reopening a cancelled parent asks "Also reopen the N subtasks cancelled with it?" (CLI `reopen --with-subtasks`). It reopens only the subtasks that its own cascade cancelled. Like the cancel cascade, it's a system move recorded with the caller, with no check of each subtask's own Reopen actors. An auto-start by a tick isn't reversed by unticking. The task stays In progress, and the assignee can reassign or cancel it if the start was a mistake.

### 4.3 Review

**[O] Decision T4.** Review is a status with a reviewer on the same task, not a new task. The task keeps one history, and the owner's Following row stays one row.
- **Review is optional.** A task carries a **Needs review** flag, off by default. The owner can always set or clear it before Done, in the UI or the CLI, including when the owner is both Asked by and the assignee: the owner is the final authority. Asked by can also set or clear it, and the assignee can set it. If Asked by is not the owner and is also the assignee, they can't clear it, because nobody waives their own review; only the owner can. Clearing the flag isn't approving: the owner still can't Approve their own work.
- **Nobody approves their own work, the owner included.** If the owner is the assignee, someone else reviews.
  - With the flag off, In progress → Done is allowed, and the assignee may still choose In review.
  - With the flag on, Done is reached only through In review → Approve.
  - Reopening keeps the flag and voids any earlier approval (§4.2).
  - So the owner gets review requests only for the work they, or the assignee, asked to have checked, not for every task they ask for.
- **Who reviews:** the assignee moves the task to In review with the outcome line, and names a reviewer: a person or an agent other than the assignee. If Asked by is the assignee, the assignee has to name someone else.
- **Who can decide:** the reviewer, or the owner unless the owner is the assignee.
- **Changing the reviewer:** while the task is In review, the owner, Asked by or the assignee may change the reviewer, never to the assignee. The timeline records it as "Reviewer changed from A to B by C".
- **Withdrawing:** the assignee can take the task back to In progress, with a comment. A task In review can't be Blocked; withdraw it first.
- **The reviewer's two actions:**
  - **Approve:** the task goes to Done.
  - **Request changes:** the reviewer must add a comment, and the task goes back to In progress with the same assignee.
- **When the owner reviews**, the Inbox shows a "Review requested" Approval (D26). Until TK-5 ships, D26's v1.0 note stands: Review requested is an agent's ask marked as a review (Rowan's option a). Once TK-5 ships, the item comes from the task's In review state instead, and D26's "a real review state comes after v1.0" is superseded.
- **When an agent reviews**, it's notified and sees the task in its own `tasks list --reviewing`.
- **Review needs no access to the assignee's private notes** (§4.5). The reviewer works from the brief and the comments.

### 4.4 Assignment

**[O] Decision T5.** A task has **one assignee at a time**, or none while it's To do. Reassignment is allowed and always recorded.
- **Who can reassign:**
  - the assignee, to hand the work over;
  - Asked by;
  - the owner;
  - the assignee's manager;
  - the project manager (§4.2).
- An unassigned task is assigned as §4.2 says (AC-TK6-1). A Done or Cancelled task can't be assigned: "Reopen it first".
- **What reassignment does:**
  - The status returns to **To do** until the new assignee starts, so "picked up" is a real event with a real time.
  - The timeline shows "Reassigned from Kai to Sage by Kai: {optional reason}".
  - Reassigning to the owner opens a **Hand-off** Approval (Accept) in the Inbox (D26), unless the owner assigns it to themselves (AC-TK6-3). Accept counts as the owner's start: To do → In progress. The Hand-off is defined below.
- **Hand-off.** **A Hand-off opens when someone other than the owner assigns a task to the owner:** creating it (including a subtask, `--parent`, or promoting a step with `--assignee owner`), assigning it from unassigned, reassigning it, or reopening it to the owner. The owner assigning, creating or reopening for themselves never opens one. The task records that its Hand-off is open (§6.13). It stays open until the owner makes one of the moves below. It clears on:
  - any move by the owner: Accept, `start`, a tick, a block, or Done;
  - the owner running `assign --none` on the task handed to them;
  - a reassignment away from the owner, by anyone allowed to reassign;
  - a cancel.
  - Unblocking later doesn't reopen it.
  - Comments and step edits don't clear a Hand-off; only the moves listed do.
- **Creating a task with an assignee** (`tasks new --assignee`): any member of the project may create a task assigned to anyone on the project, so agents can delegate. This decides only whom the task may be assigned to. The only refusal is an assignee who isn't on the project. Who may add a subtask or promote a step is a separate question, decided in §4.6. Asked by is the message's sender with `--from-message`, and the creator otherwise. This differs from reassigning an existing task (AC-TK6-1). Creating a task assigned to the owner opens a Hand-off as defined above, and makes the owner follow it (AC-TK6-3, AC-TK6-6). This holds whoever Asked by is: the owner asking for a task doesn't mean the owner agreed to do it.
- **Unassigning:** only while To do (§4.2).
- **Reassigning a Blocked or In review task** clears its reviewer and its "waiting on", and the task returns to To do. The timeline keeps both, and the new assignee sets them again if they still apply.
- **Cancelling a parent** also clears any budget block or hold on it and on its subtasks. It cancels its open subtasks, with the same reason, and the timeline records each one as "Cancelled with T-9 by Kai". It's a system move, recorded with the actor who cancelled the parent (§4.2).
- **Help with part of the work is not reassignment.** Promote a step to a subtask and assign that (§4.6).

### 4.5 Brief, comments and private notes

**[O] Decision T6.** A task carries three kinds of text, which differ in who sees them and whether they travel:

| | **Brief** | **Comments** | **Private notes** |
| --- | --- | --- | --- |
| What it is | the requirements: title, description (Markdown) and definition of done. Steps show with it but aren't part of it (§4.6). | the shared record: progress, questions, decisions, changed requirements | the author's own working notes |
| Who sees it | everyone who can see the task | everyone who can see the task | **the author, and the owner**. No other member, person or agent, including reviewers and managers. |
| On reassignment | travels | travels | **stays with its author**. The new assignee doesn't see it. |
| Edit | **versioned**: the title, description and definition of done. Only Asked by and the owner edit it; the assignee proposes changes in a comment. See below | the author can edit; everyone sees "edited", but only the author and the owner can open the earlier versions. That way, text removed by an edit, such as a pasted secret, stops being visible to others; to remove a secret entirely, its author or the owner redacts the earlier version, which erases it (AC-TK7-6). In v1.0, only the owner can open earlier versions, until M-A is closed (AC-TK7-4). | the author can edit and delete; the owner sees the history |
| Delete | — | by the author or the owner; soft: shows "Comment deleted", and the owner can still read the original unless it was redacted (AC-TK7-6) | soft: the owner can still read the original unless it was redacted (AC-TK7-6) |
| Redact | — | the author or the owner, any version (current, earlier or deleted): erases its text (AC-TK7-6) | the author or the owner, any version (current, earlier or deleted): erases its text (AC-TK7-6) |

**Why this concept works:**
- The owner's reasoning matches how good teams already work: the ticket says what's needed, the thread says what changed, and your own notebook is your own.
- It keeps one agent's rationale from steering the next agent's or a reviewer's judgment, which is what the owner asked for.
- I considered a single stream with a "private" flag on each comment, and rejected it. A flag is easy to miss, and one wrong click leaks a note. Two separate composers make the choice explicit every time.

**Brief edits.** Only Asked by and the owner edit the brief. The assignee proposes a change in a comment. Anyone else's edit, the assignee's included, is refused: "Only Asked by or the owner can change the brief. Propose it in a comment." Once a task has ever been started, any edit to the brief needs a reason, even if the task has since returned to To do through reassignment or reopening. The reason is posted as a comment, "Changed the brief: {reason}", linked to a diff. So requirement changes always show up in the shared record, and the original brief can always be read under **Versions**, except a version redacted for a pasted secret. The owner, or a version's author, can redact a version that holds a pasted secret, which erases its text (AC-TK7-6). The title, the description and the definition of done are all versioned. Step changes aren't brief edits: they need no reason, make no version and post no "Changed the brief" comment (§4.6).

**System comments.** A comment the system posts for a human action, such as "Changed the brief: {reason}", can't be edited or deleted by anyone (AC-TK7-9). The record of a requirement change stays whole.

**How the UI makes privacy unmistakable** (for Eden to design):
- The task page has **two composers**, as tabs: **Comment** and **Private note**.
  - The Private note tab shows a lock icon and a line that depends on who's writing:
    - **for the owner:** "Only you can see this. It isn't sent to {assignee}.";
    - **for anyone else, a person or an agent:** "Only you and the owner can see this. It isn't sent to {assignee}.";
    - **with no assignee,** the second sentence reads "It isn't shared with anyone working on this task."
  - On an agent's note, as the owner sees it: "{Agent}'s private notes. Other agents can't see them."
- Private notes sit in their own section, below the timeline, with the lock icon and a neutral, tinted background. They're never mixed into the comment stream.
- The owner's own notes are private to the owner. No agent sees them.
- **Agents are told plainly** in the CLI help that the owner can see their notes, and that deleting a note doesn't erase it: the owner can still read the original unless it was redacted (AC-TK7-6). Honesty with agents matters too.
- **Write states.** When a comment, a note or a status move (with its reason, outcome line or comment) fails to save, or the office is offline, the draft stays in the composer with "Couldn't save. Retry". Nothing typed is lost.

**[O] Until private notes ship (v1.0).** Private notes come after v1.0, because they depend on M-A (§6.4, §8). The agent instructions that ship with TK-10a tell agents to keep their working rationale in their own memory or journal, not in task comments. Comments are for facts, requirement changes, questions and results.

### 4.6 Steps and subtasks: one level

**[O] Decision T7.**
- **Steps** are the task's checklist. Agents plan by adding steps and tick them off one by one, which costs one call each.
- **Steps aren't part of the versioned brief** (§4.5). Adding, editing, removing, reordering, ticking and unticking a step are plain timeline events: no reason, no brief version, and no "Changed the brief" comment, even after the task has started.
- **Who can change steps:**
  - add, edit, reorder and remove an optional step: the assignee, Asked by and the owner;
  - **required steps:** only Asked by and the owner can add a required step, make a step required or optional, edit a required step's text, or remove a required step. The assignee can't. To get past a required step they won't do, the assignee uses `done --anyway --reason` (§4.2);
  - tick and untick any step, required or optional: the assignee and the owner. Only the assignee's tick auto-starts a To do task (§4.2);
  - promote a step to a subtask, or create a subtask (`--parent`, "Add subtask"): the assignee, Asked by, the owner, the assignee's manager and the project manager (§4.2). This is the one rule for both. On an unassigned parent, the project manager acts alone, besides Asked by and the owner.
- **A step can be promoted to a subtask** when it needs its own assignee. Whom the subtask may be assigned to follows §4.4: anyone on the project.
- **A promoted required step** counts as ticked while its subtask is Done (completed), and unticks if that subtask is reopened. A Cancelled subtask leaves it unticked, so the parent needs `done --anyway --reason` to reach Done. That tick is not the assignee's own: it doesn't auto-start a To do parent. Only the assignee's own tick does (§4.2).
- **A closed task takes no step changes.** While the task is Done or Cancelled, adding, editing, reordering, removing, ticking or unticking a step, or making it required or optional, is refused: "Reopen it first".
- **A subtask can't have subtasks**, and the server refuses deeper nesting. A subtask can have its own steps, so its assignee can plan it too, but those steps can't be promoted.
- A subtask can also be created directly on a parent: `tasks new --parent ID`, or "Add subtask" on the task page.
- The parent shows its subtasks inline, with their status.
- A parent can't be Done while a subtask is open, unless that subtask is Cancelled.
  - **Ruling (2026-11-12, staff review):** this rule stands. A parent task can't be marked Done or approved while any of its subtasks is open (To do, In progress, Blocked or In review); required checklist steps keep their separate rule. The current code only blocks on required steps, so this is a code change. Done and Approve are refused with the open subtasks listed by key: "T-12 has 2 open subtasks: T-13, T-14. Finish or cancel them first."
- **A closed parent takes no subtask changes.** While the parent is Done or Cancelled, `tasks new --parent`, promoting one of its steps, and reopening one of its subtasks are refused: "Reopen T-9 first".
- Reopening a parent doesn't reopen its subtasks. They keep their status, and each can be reopened on its own.
- Steps and subtasks are optional. A plain task is fine.

### 4.7 Captured automatically, never typed

**[O] Decision T8.** Every change is a `WorkEvent` with its actor and the server's time. The owner sees the figures on the task and in Following:

| Figure | How it's derived |
| --- | --- |
| Started | the first move to In progress, or a move from To do straight to Done, which records Started and Done at the same moment |
| Done at or Cancelled at | the move to Done, or to Cancelled |
| Picked up by {member} | each move to In progress after an assignment |
| Time in to do, in progress, blocked and in review | the sum of the intervals in each status |
| Time waiting on you | the sum of the spans in which the task waited on the owner (the five cases in §4.1), measured from those spans, not from a status |
| Cycle time | from started to done, leaving out any span spent in Done or Cancelled (a reopened task's closed time doesn't count; `project-insights-v1.md` §4.2) |
| Lead time | from created to done, leaving out any span spent in Done or Cancelled, as for cycle time |
| Step timing | the tick events |

**Points** (optional, set when planning) are a new field and show only when set (the same rule as D15's project fields). The assignee, Asked by or the owner can set or clear them. Due dates are out of scope here. **[F]** Projects carry no due date today: the project record has none (`docs/design/tasks-projects-docs.md:142`). AC-IA17-1 shows a due date only when set, so today it never shows. Target dates belong to releases (`projects-planning-v1.md`).

**[O] Output.** The Done outcome line may contain links. The task page has an **Output** section that lists the task's links and deliverables: docs, branches, commits and files. In Following, a Done row shows the outcome line and its first link.
- Agents fill Output from the CLI in v1.0: `tasks link ID URL|path [--title]` and `tasks unlink ID LINK` (§5).
- In v1.0, branches and commits are linked by URL. Typed commit and branch references stay with the design's P2 plan (`tasks-projects-docs.md:972`, C7).

What can't be deterministic is whether the work is really done. Done is the assignee's claim. The required outcome line, the ticked steps, and the reviewer's approval when there is one make that claim checkable.

### 4.8 What reaches the Inbox from a task

The Inbox gets an item only when a task waits on the owner:
- a question asked of the owner, which is a question comment;
- the task is Blocked on the owner;
- review is requested from the owner;
- a Hand-off to the owner is open: someone other than the owner created the task for the owner, assigned it to the owner (from unassigned or by reassigning) or reopened it to the owner (§4.4). A created task opens one whoever Asked by is;
- its budget is reached: a Budget reached block or hold (§4.2), with its own Budget reached item. Subtasks stopped by their parent's budget get none of their own.

Deleting an open question comment to the owner ends the wait and clears its Inbox item. Editing it keeps the wait. Cancelling the task also ends an open question's wait: the question stays in the thread, closed, and its Inbox item clears. **[O]** An open question to the owner on a Done task stays in Waiting on you until it's answered or deleted. That's intended: the question is still open.

A task being done is **not** an Inbox item. It's news in Following.

### 4.9 Task, message or Inbox item?

| | **Task** | **Message** (Chat) | **Inbox item** |
| --- | --- | --- | --- |
| What it is | a unit of work | a conversation turn | a pointer that says "this waits on you" |
| Has an owner, status and outcome | yes: one assignee, six statuses, an outcome | no | no; it has a clearing rule (D26) |
| Lives | in Work and Following | in Chat | in the Inbox. It's computed from its source, not stored on its own (§10). |
| Use it when | you'd want to know later whether it got done | you want to inform, discuss or ask in passing | never created directly; it appears when a task, message or request waits on the owner |
| Example | "Migrate the docs site" | "FYI, the build is slow today" | "Kai asks: which region?" (a question on a task), or "Review requested: T-131" |

If a chat message asks for work, the agent makes it a task with `--from-message`, which records the owner as Asked by.

## 5. The agent CLI: `org-agent tasks`

**[O] Decision T9.** The CLI noun is **`tasks`**, the owner's choice. The existing design (`docs/design/tasks-projects-docs.md` §5, :814-860) uses `task`; this supersedes the noun and keeps that design's verbs wherever they fit:
- `new` (with `add` as an alias, the owner's word), `list`, `show`, `start`, `block` and `unblock`, `done` (with `--cancelled` and `--anyway --reason`), `reopen`, `assign`, `edit`, `current`, `comment` (with `--ask` and `--answer`), `link` and `unlink`, `checklist`, `check` and `uncheck` (closing steps one by one), and `step`;
- `--json` and `--office` on every command.

Rowan owns the exact flags and output. **What this PRD adds:**

| Need | Addition (indicative) |
| --- | --- |
| Asked by | `tasks new … --from-message ID`. The server derives Asked by from the message (T2). |
| Points and review | `tasks new … --points N`; `tasks edit ID --points N \| --points none`, and `tasks edit ID --needs-review \| --no-needs-review`, which ships with TK-5. These aren't brief edits: no reason, no brief version, and each is recorded in the timeline. Who: the flag as §4.3; points by the assignee, Asked by or the owner. |
| Blocked on whom | `tasks block ID --on MEMBER\|outside --reason …` (from To do or In progress). On a Blocked task, it changes who or what the task waits on, with a new reason, without unblocking it. |
| Steps | `tasks step add\|edit\|rm\|move`, `tasks check\|uncheck` (design :831-834). These aren't brief edits: no reason, no brief version, and each is recorded in the timeline. Who: as §4.6. Adding a required step, making a step required or optional, editing a required step's text, or removing a required step is for Asked by and the owner only: the assignee is refused and told to use `done --anyway --reason`. The assignee adds, edits, reorders and removes optional steps, and ticks any step. Every step change on a Done or Cancelled task is refused: "Reopen it first". |
| Unblock | `tasks unblock ID [--start]`: back to the status before Blocked (§4.2). On a held task, unblock into In progress and `start` are refused: "T-12 is on budget hold: a person must raise it, allow 20% once or clear the budget." |
| Reopen | `tasks reopen ID --comment … [--start] [--with-subtasks]`: back to To do, keeping the last assignee. `--start` also starts it, and is allowed for the assignee or the owner; for anyone else it's refused with "Only the assignee or the owner can start T-12". `--with-subtasks` also reopens the subtasks this parent's cancel cascaded (AC-TK8-7), as a system move recorded with the caller. |
| Lists | `tasks list --asked-by-me \| --created-by MEMBER \| --reviewing` |
| Review | `tasks review ID --reviewer … --outcome "…"`, `tasks approve ID`, `tasks request-changes ID --comment …`, `tasks withdraw ID --comment …`, and changing the reviewer while In review with `tasks review ID --reviewer <member>` (Rowan owns the exact flags) |
| Done needs an outcome | `tasks done ID --outcome "…"` (and `--cancelled --reason …`, which is Cancel, with Cancel's actors: §4.2) |
| Reassignment reason | `tasks assign ID MEMBER [--reason …]`. Refused on a Done or Cancelled task: "Reopen it first". |
| Assign on create | `tasks new … --assignee MEMBER`, also with `--parent` and on `tasks step promote`. Whom: anyone on the project (AC-TK6-6). Who may use `--parent` or `step promote`: §4.6. Asked by is the creator unless `--from-message` sets it (§4.4). |
| Brief | `tasks edit ID --title … --description … --dod …` (Rowan picks the flags), with `--reason` once the task has been started. Asked by and the owner only; anyone else is refused: "Only Asked by or the owner can change the brief. Propose it in a comment." |
| Output | `tasks link ID URL\|path [--title …]` and `tasks unlink ID LINK`, in TK-10a. Branches and commits are linked by URL in v1.0 (§4.7). |
| Subtasks | `tasks new --parent ID …` (refused when the parent is itself a subtask), and `tasks step promote` (today's `make-task`, refused on a subtask's steps). Both, and reopening a subtask, are refused while the parent is Done or Cancelled: "Reopen T-9 first" (§4.6). Who: as §4.6. |
| Comment CRUD | `tasks comment add\|edit\|delete\|list` (today only add exists), and `tasks comment redact <id> [--version n]`, by the comment's author or the owner only (AC-TK7-6) |
| Private notes | `tasks note add\|edit\|delete\|list`: only my own notes, and `tasks show` includes only my own; `tasks note redact <id> [--version n]`, by the note's author or the owner only (AC-TK7-6) |

**A naming clash to resolve:** the wire's `CommentKind` already has a value called `note` (`work.rs:291-293`), which is an ordinary shared comment. "Note" must mean **private note** only, everywhere the owner or an agent sees it. Rowan picks how to rename the wire value or the new concept.

**CLI requirements:**
- Output is **compact by default**: one line per item, and `show` sends no more than needed. That keeps the token cost low.
- Every write is idempotent on a client-made ID (as `change_id` is today), so a retry never duplicates.
- **Check order for every refusal** (as `costs-and-budgets-v1` §5.4a): (1) is the action available in this state (for example, Allow once only at 100%); (2) is the actor permitted (for example, person-only actions and the budget-hold texts); (3) other guards (for example, open subtasks or required steps). The first failing check's text is returned. So an agent's Done on a held parent with open subtasks gets the budget-hold text, and a person's gets the open-subtasks text.
- Errors say what to do next, for example "Done needs --outcome" or "3 required steps open: check them or use --anyway --reason", or, for Done or Approve on a parent with open subtasks, "T-12 has 2 open subtasks: T-13, T-14. Finish or cancel them first."
- **Agents don't post "started" or "done" messages in chat.** The task timeline replaces them. Agent guidance says so.
- **Until private notes ship,** agent guidance also says to keep working rationale in the agent's own memory or journal, not in task comments (§4.5).

## 6. Requirements for engineering (Rowan designs the how)

1. **Asked by** comes from a message the server can verify, never from the agent's claim (T2).
2. **In review**, a reviewer field, **Cancelled** as the owner sees it, and the required fields for each status (T3). The server enforces the §4.2 transition table, with its actors. Blocked must remember the status it was entered from, so `unblock` can return to it. A cancel cascade is a system move, recorded with the actor who cancelled the parent. Reopening a cancelled parent can also reopen exactly the subtasks its cascade cancelled, when the caller asks (`reopen --with-subtasks`, AC-TK8-7). Like the cascade, that's a system move recorded with the caller, with no check of each subtask's own actors. A Blocked task can change what it waits on without unblocking (§4.2). `work.undo` must refuse an auto-start, with "Reassign or cancel instead"; today it can undo one (W:770, W:1561-1564). No other new undo is needed (§4.2). Creating a task with an assignee is open to any project member, for anyone on the project, and creating one assigned to the owner, by anyone else, opens a Hand-off whoever Asked by is (AC-TK6-6). "Budget reached" is a system move In progress → Blocked, with no actor, and its resolution (by the owner or a human project manager raising, allowing once or clearing the budget, or a cost record moved out; an agent project manager is refused those three) is a system move back to In progress; an agent project manager's move of a cost record that lowers the spent of a reached budget (off a stopped task, any of its subtasks, or a subtask whose parent's budget is reached) is refused too; a To do or In review task gets a budget hold instead (§4.2). A person (a human reviewer, a human assignee or the owner) can still Approve or Done a held task; that clears the hold and its Inbox item, and agent turns stay refused. Reassigning a Budget reached or Parent budget reached block moves it to To do as a hold (a parent-held subtask keeps the parent's stop, with no Inbox item of its own); reassigning a hold doesn't lift it; cancelling clears either. Who may add a subtask or promote a step is §4.6's rule, not this one. Step changes follow §4.6's actors and are refused on a Done or Cancelled task.
3. **Depth:** a subtask can't have subtasks, and its steps can't be promoted. The server enforces this for both `--parent` and `make_task` (T7). Today `make_task` has no limit (W:1629). A promoted required step counts as ticked while its subtask is Done (completed), and unticks if that subtask is reopened, and stays unticked when it's Cancelled (§4.6).
4. **Private notes are enforced on the server** across every read path: the API, the CLI, search, Activity, exports, the `work.changed` live stream (EventAdded), replication to other offices, the context handed to an agent, and watcher notifications once they're built (`tasks-projects-docs.md:971`). Only the author and the owner can read them. A non-author who edits, deletes or reads a note or a note event by its ID gets the same "not found" answer as for one that doesn't exist. Creates are covered too: a create whose ID collides with a note's ID must not reveal the note. It gets the same answer as any other ID collision, which never names notes (Rowan picks how). A test proves that no other member, agent or person, can read a note, or learn that it exists, through any of these paths.
   - **Dependency:** this privacy holds only while agents can't impersonate one another. ADR-0003 still accepts risk M-A for agents started by hand, where one agent can bind as another (ADR-0003:256). TK-7 can't claim "private" for those agents until M-A is closed for them. Rowan and ada decide whether TK-7 waits for that fix or ships with the limit stated.
5. Comment and note **edits are versioned** and **deletes are soft**. Superseded and deleted comment bodies are readable only by the author and the owner (after a delete, only the owner; after a redaction, nobody, AC-TK7-6), on every read path in §6.4: events, Activity, `work.changed`, search, exports, replication and agent context. Everyone else sees only the current text, or "Comment deleted". In v1.0, comment history is readable by the owner only, until M-A is closed (§6.4, AC-TK7-4).
6. **Brief versions**, with a reason required once the task has ever been started (T6). The brief is the title, the description and the definition of done, and all three are versioned. Only Asked by and the owner can edit it. Steps, Needs review and points aren't part of the brief: changing them needs no reason and makes no version, and each change is a timeline event (§5).
7. **Per-owner "last seen" per task**, which feeds Following's unseen count and the "New since you last looked" divider.
8. A points field, and "waiting on" (a member or outside) on Blocked.
9. The `org-agent tasks` CLI (§5).
10. Every change is a `WorkEvent` (actor and server time), including note and comment edits. **Note events are invisible** to every reader except the author and the owner. Other members don't even learn that a note exists: it doesn't appear in the timeline, Activity, `work.changed`, Following's "last change", or the unseen count. For the owner, a new or edited agent note does count as unseen, and can be the owner's Following "last change" ("Kai added a private note"). **A note event must not change anything another member can observe:** the item's version (W:764), its last-updated time (W:385), list ordering, counts, or a stale-write conflict. The event log stays complete, and the note stays private. Rowan chooses the mechanism.
11. **A read for Following:** `work.list` can't filter by watcher or Asked by today (W:1088-1115, W:73). Following's "Followed" and "Asked by me" need one. **The product must tell a follow the owner chose from an automatic one**, so the "Followed" filter and the Unfollow and re-follow rules in §6.12 hold. Today nothing records the difference. Rowan chooses the mechanism.
12. **Follow rules.** Following is exactly the set of tasks the owner follows.
    - **Followed automatically:**
      - Asked by becomes a watcher when the task is created. Today only the caller is seeded (`org-work/src/tasks.rs:147`).
      - The creator.
      - The owner, whenever a task starts waiting on them: blocked on the owner, the owner as reviewer, a hand-off to the owner, a question to the owner, or a budget reached.
    - The **"Followed" filter** means followed by hand, with Follow.
    - Anyone can **Unfollow**, including the creator and Asked by. That replaces today's "the creator always watches" (`org-work/src/items.rs:107-108`).
    - Unfollowing removes the task from Following entirely, under every filter, and stops it counting toward the badge. It stays in Work's Created by me and Asked by me views. Follow brings it back.
    - **Precedence:** a task that waits on the owner is always followed and shown. Unfollow is disabled while it waits ("You can unfollow once this no longer waits on you"). A new wait re-follows a task the owner had unfollowed, and the task stays followed after the wait ends, until the owner unfollows it again. Once the wait ends, it shows under All, and not under Asked by me, Created by me or Followed unless those also apply.
    - **Subtasks:** following a parent follows its subtasks with it. In Following they nest under the parent's row, and a subtask change counts as a change to that row (§4.1). The one exception: a subtask waiting on the owner also gets its own row in Waiting on you (§4.1). A nested subtask can't be unfollowed on its own (unfollow the parent); a subtask followed on its own, with its own row, can be. Opening a task, parent or subtask, clears its unseen changes and its nested subtasks' on every row they marked, and the badge counts tasks with unseen changes, each once (§4.1).
13. **Two Inbox source rows ship with TK-4 and TK-6 in v1.0**, so D26's rule holds: "work blocked on the owner always has an open Inbox item".
    - "Blocked on you" from task state (today a later row, FREEZE:77).
    - A task hand-off to the owner, open as §4.4 defines, from a create, an assign or a reopen (a work row, separate from Chat's hand-off row, FREEZE:72). The task records that its Hand-off is open, set by the opening event and cleared by the moves in §4.4, so the Inbox row, Needs me, Waiting on you and the owner-follow all read one field. Rowan decides how it's stored.
    A budget stop, block or hold, has its own "Budget reached" row, with Raise budget, Allow once and Stop (`costs-and-budgets-v1`). It doesn't ride the "Blocked on you" row, and AC-TK4-3's withdrawal never applies to it. Subtasks stopped by their parent's budget get no row; their Waiting on you row links to the parent's.
    If either can't make v1.0, then "blocked on the owner" and "reassign to the owner" can't ship either.
14. **Notes replicate to HQ like everything else**, so the owner can read them. "Replication" in §6.4 means that no other member can read them on any office.
15. **An Inbox filter by work item:** `inbox.list` takes only state, kinds, project and team today (FREEZE:28). Following's 'Waiting on you' rows link to the Inbox filtered to one task, so it needs a work-item filter (Rowan).
16. **Output:** the outcome line accepts links, and the task page lists the task's links and deliverables (docs, branches, commits, files) in an Output section (§4.7). Agents add and remove them with `tasks link` and `tasks unlink` (§5). **[F]** A link today points only at an item or a doc (`LinkTarget`, W:584-601), so a URL or file path needs a new target; Rowan chooses the mechanism.

## 7. Stories

IDs are stable. Each story also passes the UI preflight and the lens (`product-vision-and-ux-lens.md`).

- **TK-1 Following.**
  - **AC-TK1-1:** The sidebar shows Following directly under Inbox. Its badge counts the tasks with a change by someone other than the owner since the owner's last seen (§4.1). Each task, subtasks included, counts once. A parent marked only by its subtasks' changes isn't counted on its own. The badge is neutral, never amber, and hidden at 0.
  - **AC-TK1-2:** `/following` lists exactly the tasks the owner follows. Those are followed automatically (Asked by, the creator, or the task waiting on the owner: §6.12) or by hand, and they exclude any the owner has unfollowed, unless the task waits on the owner, which always wins (§6.12). They're grouped in the §4.1 order; the In review group is hidden until TK-5 ships. Each task appears once, in the first group it matches, in the §4.1 order. The one exception: a subtask that waits on the owner also appears as its own row in Waiting on you (AC-TK1-9). Each row shows what §4.1 lists: the title and project, the assignee's avatar and name, the status in words with its icon, steps x/y (and "Subtasks x/y" on a parent), the last change, and for done tasks the outcome line and its first link. Done earlier lists followed tasks closed (Done or Cancelled) 0 to 30 days ago that the owner has seen (no unseen changes; a task the owner closed themselves goes straight to Done earlier), measured from their close time; unseen ones stay in the Done group; older ones are reached through Work's filters (Created by me, Asked by me or Followed, with Done), or through Work's All view with filters. The filter is in the URL.
  - **AC-TK1-3:** Clicking a row opens the task, scrolled to the first unseen change under "New since you last looked". Opening the task from anywhere (Following, Work, Inbox, Home or a link) clears its unseen state. A row's unseen state is any unseen change to the task or its nested subtasks. Opening a task, parent or subtask, clears that task's and its nested subtasks' unseen changes on every row they marked, and the badge drops to match. Test: subtask T-12 under T-9 becomes blocked on the owner, with nothing else unseen. Following shows T-12 in Waiting on you and T-9's row bold, with badge 1. The owner opens T-12 from the Inbox: both rows clear and the badge is 0. Parent-open case: with the same state, the owner opens T-9 instead: both rows clear and the badge is 0. Count case: two nested subtasks of T-9 change, and the badge is 2. "Mark all as seen" on the page clears every unseen state at once.
  - **AC-TK1-4:** Home shows a Following card with the five most recently changed rows, after Needs you.
  - **AC-TK1-5:** The empty, per-filter empty, loading, error and offline states match §4.1. The Home card is hidden when there's nothing to follow.
  - **AC-TK1-6:** Every task page has Follow, which becomes Following once pressed; pressing it again unfollows, even on a task the owner created or asked for, except while the task waits on the owner, when Unfollow is disabled with its reason. A nested subtask is the other exception: it can't be unfollowed on its own (AC-TK1-11). Tasks the owner asked for, created, or that wait on the owner are followed automatically (§6.12). No owner-facing text says "watch".
  - **AC-TK1-7:** Automatic changes count toward the badge, unless an owner action caused them (for example, a cancel cascade the owner started).
  - **AC-TK1-8:** Each 'Waiting on you' row links to the Inbox filtered to that task, showing only its open items (§6.15). The group's header and row marker may use amber; the sidebar badge stays neutral.
  - **AC-TK1-9:** A followed parent's subtasks are followed with it. They show nested under the parent's row, not as separate rows, except that a subtask waiting on the owner (blocked on the owner, the owner as reviewer, an open Hand-off to the owner, an open question to the owner, or a Budget reached block or hold) also shows as its own row in Waiting on you, labelled "Subtask of {parent}". A subtask change marks the parent's row unseen and updates its last change. The parent row shows "Subtasks x/y" next to steps x/y. A subtask followed without its parent shows as its own row labelled "Subtask of {parent}".
  - **AC-TK1-10:** A task that waits on the owner shows in Waiting on you under every filter (Asked by me, Created by me, Followed and All), even when it doesn't otherwise match the filter. A task that stays followed after its wait on the owner ends shows under All, and not under Asked by me, Created by me or Followed unless those also apply.
  - **AC-TK1-11:** A subtask nested under a followed parent can't be unfollowed on its own: its Unfollow is disabled, with "Unfollow T-9 to stop following this subtask". A subtask followed without its parent, on its own row, can be unfollowed like any task.
  - **AC-TK1-12:** Work's Needs me lists every task that waits on the owner, in all five cases of §4.1, an open question to the owner and a Budget reached block or hold included (AC-IA18-1).
- **TK-2 Work views.**
  - **AC-TK2-1:** Work's Show ▾ menu adds Created by me and Asked by me, and a Created by ▾ filter lists every member.
  - **AC-TK2-2:** Mine still means assigned to me.
  - **AC-TK2-3:** Work has a Followed filter that lists the tasks the owner follows by hand, in any status, including Done and Cancelled tasks older than Done earlier's 30 days. A task followed only automatically (Asked by, creator, or a wait on the owner) isn't listed by it unless the owner also pressed Follow. Test: follow a task by hand, let it close and age past 30 days. It's gone from Following, and Work → Followed still lists it.
- **TK-3 Asked by.**
  - **AC-TK3-1:** A task created with `--from-message ID` gets Asked by set to that message's author. This works only if the message was addressed to the creating agent (a direct message, or a mention); otherwise the request is refused.
  - **AC-TK3-2:** Asked by can't be set any other way except as the creator.
  - **AC-TK3-3:** The task header shows "Asked by {name}". The link to the message is shown only to members who can read that message.
  - **AC-TK3-4:** A task created with `--from-message` must have a description restating the request, so any later assignee starts from the original request even without access to the message. A missing description is refused. The agent guidance says the brief is visible to everyone who can see the task, so restate the request, and leave out private detail from the message.
- **TK-4 Statuses.**
  - **AC-TK4-1:** The statuses in §4.2 each show an icon and a word. In review appears only once TK-5 ships; until then there are five. The Work board has one column per status, Cancelled included, and the Cancelled column is collapsed by default.
  - **AC-TK4-2:** Entering a status without its required field is refused with a plain error.
  - **AC-TK4-3:** Blocked on the owner opens the D26 "Blocked on you" item, following D26's rules: it's withdrawn while an Approval or Question about the same work is open, and it returns after a refusal. That withdrawal applies to "Blocked on you" only: a Budget reached item is never withdrawn by it, even while an Approval or Question about the same work is open. Test: a task at a stopped budget with an open question keeps its Budget reached item.
  - **AC-TK4-4:** Reopening needs a comment and returns the task to To do. Cancel and Reopen are refused for anyone outside the §4.2 actor sets, with an error that names who can do it.
  - **AC-TK4-5:** `start` is an explicit event by the assignee. Any tick by the assignee while the task is To do moves it to In progress automatically, as an automatic event whose `cause` points at the tick. Only the assignee's tick does this; a tick by anyone else, the owner included, doesn't change the status. On a task with a budget hold, only a human assignee's or the owner's tick is recorded, and it doesn't start the task; an agent's tick is a turn and is refused with the hold text (AC-TK4-19). Unticking that step doesn't undo the start (§4.2). `work.undo` on the auto-start is refused, with "Reassign or cancel instead"; Started and Picked up stay. Either way, "Picked up by" is the assignee, at the server's time.
  - **AC-TK4-6:** `unblock` returns the task to the status it had before it was blocked (To do or In progress), and clears "waiting on". `unblock --start` is accepted, and starts the task if it returns to To do.
  - **AC-TK4-7:** The assignee can block a To do task, with waiting on and a reason. Its later `unblock` returns it to To do.
  - **AC-TK4-8:** The assignee can mark a To do task Done, with Done's guards. Started and Done are recorded at the same moment, and the timeline shows "Started and done". Once TK-5 ships, To do → Done is refused while Needs review is set, as is every move to Done other than Approve.
  - **AC-TK4-9:** The owner can make any move the assignee can, and the timeline records it as made by the owner. The exceptions: the owner can't approve their own work, and the owner's tick on someone else's task doesn't auto-start it (AC-TK4-5). Other than the owner, Asked by and the managers can't start, block, unblock or mark Done another member's task; they're refused with an error that names who can. `done --cancelled` is Cancel, and follows Cancel's actors (§4.2), not Done's.
  - **AC-TK4-10:** Unassigning (`assign --none`) is refused unless the task is To do.
  - **AC-TK4-11:** Reopening a Cancelled task is refused for the last assignee, unless they're also Asked by, the owner, the assignee's manager or the project manager.
  - **AC-TK4-12:** Reopening a Done or Cancelled task keeps its last assignee. The task returns to To do with that assignee.
  - **AC-TK4-13:** When a status move on the task page (with its reason, outcome line or comment) fails to save, or the office is offline, the draft stays in its composer with "Couldn't save. Retry". Nothing typed is lost.
  - **AC-TK4-14:** Starting, blocking or marking Done an unassigned To do task is refused, with an error saying to assign it first. A member who assigns it to themselves can then start it.
  - **AC-TK4-15:** Cancel is refused on a Done task, with "Reopen it first". Every open status can be cancelled.
  - **AC-TK4-16:** Every block needs a reason, whether it waits on a member or on "outside". A block without one is refused.
  - **AC-TK4-17:** On a Blocked task, `block --on` again, with a reason, changes who or what the task waits on without unblocking it. The task stays Blocked, and the timeline records the old and new waiting on, the reason and the actor. Changing it to or from the owner opens or clears the "Blocked on you" item (AC-TK4-3).
  - **AC-TK4-18:** When a budget stops an In progress task, it moves to Blocked, waiting on the owner, with the reason "Budget reached" ("Parent budget reached" for a subtask of a stopped parent) and no actor. When the owner or a human project manager raises, clears or allows once on a task budget, or a cost record moved out of the task lifts it, it moves back to In progress, recorded as caused by that action. An agent project manager's Raise, Allow once or clear, or its move of a cost record that lowers the spent of a reached budget (off the stopped task, off any of its subtasks, Done or Cancelled included, or off a subtask whose parent's budget is reached, `costs-and-budgets-v1` requirement 33), is refused with "Only a person can raise, clear or extend a budget." and the task stays Blocked. A Raise or Allow once whose new limit is still at or below spent is refused and changes nothing; lowering a reached budget is accepted and the task stays Blocked. Deleting the task, or a person moving a cost record off it, also ends the stop. The assignee can't `unblock` or `block --on` such a block, and is refused with "T-12 is Blocked "Budget reached": a person must raise it, allow 20% once or clear the budget." (on a subtask's Parent budget reached block: "T-13 is Blocked "Parent budget reached": a person must raise T-12's budget, allow 20% once or clear it."). The owner is refused too. Clearing (removing) the budget, or a person changing its unit to a value above spent, also moves it back to In progress. The block opens its own Budget reached Inbox item (Raise budget, Allow once, Stop), shows in Waiting on you and Needs me, and follows the owner. A subtask stopped by "Parent budget reached" has no Inbox item; its Waiting on you row links to the parent's item. Reassigning moves it to To do and turns it into a budget hold, with the item and wait kept (AC-TK6-4); cancelling clears it. Tests: `unblock` by the assignee and by the owner are both refused; the owner's budget action returns it to In progress; an agent project manager's Raise is refused and the task stays Blocked; an agent project manager's move of a cost record off the stopped task is refused, the record stays and the task stays Blocked; with T-12 Blocked "Budget reached", ada (agent project manager) moving a record off Done subtask T-13 is refused, and a human project manager's same move is allowed and T-12 resumes; the owner's Raise to a value still at or below spent is refused and nothing changes.
  - **AC-TK4-19:** A To do or In review task (or subtask) at a stopped budget gets a budget hold: its status, reviewer and review are unchanged, and agent turns on it are refused until the owner or a human project manager resolves the budget (an agent project manager can't raise, clear or allow once). A person can still finish it: Approve (human reviewer or owner) and Done (human assignee or owner) are allowed, and reaching Done clears the hold and its Budget reached Inbox item. A held task can't be started or unblocked into In progress until the budget is resolved; a tick on it, or an unblock into To do, is for a human assignee or the owner only (an agent's is a turn, refused with "T-12 is on budget hold: a person must raise it, allow 20% once or clear the budget."), and a person's tick is recorded without auto-starting it; a held To do task's Hand-off can still be accepted (it stays To do and held). Request changes on a held In review task (human reviewer or owner), or withdraw by a human assignee, moves it to Blocked "Budget reached"; for a subtask held by its parent's budget, it moves to Blocked "Parent budget reached", with no Inbox item of its own. An agent is refused on a held task: Done with "T-12 is on budget hold: only a person can mark it Done.", Approve with "T-12 is on budget hold: only a person can approve it." and withdraw with "T-12 is on budget hold: only a person can withdraw it from review." A subtask or top-level task already Blocked on someone else keeps its waiting on and also gets the hold (a top-level task also gets the one Budget reached item); when the budget is resolved, the hold lifts and it stays Blocked on them. One already Blocked on the owner keeps its block and its "Blocked on you" item, and gets the hold and the Budget reached item too; when the budget is resolved, the hold and the Budget reached item clear, and it stays Blocked on the owner with its "Blocked on you" item. Reopening a task at or over its budget re-applies the stop by its new status. The hold is a Budget reached wait on the owner, with the same Inbox item, lists and follow as AC-TK4-18. Reassigning doesn't lift it; cancelling clears it. Tests: a top-level task Blocked on Ana at a stopped budget keeps waiting on Ana and shows the hold, and after the owner raises the budget the hold lifts and it is still Blocked on Ana; a human reviewer requests changes on a subtask held by its parent's budget, and it moves to Blocked "Parent budget reached" with no Inbox item of its own while the parent's item stays; the owner approves a held In review task, it moves to Done, and the hold and its Inbox item are cleared; an agent turn on the held task is still refused; a human reviewer requests changes on a held In review task, and it moves to Blocked "Budget reached" (the same for a human assignee's withdraw); an agent assignee's withdraw is refused; a top-level task Blocked on the owner at a stopped budget has both its "Blocked on you" and its Budget reached items, and after the owner raises the budget it is still Blocked on the owner, the Budget reached item is cleared and "Blocked on you" stays; a human assignee's tick on a held To do task is recorded and the task stays To do, and an agent assignee's tick is refused; reopening a Done task at its budget gives it a hold at once.
- **TK-5 Review.**
  - **AC-TK5-0:** Needs review is off by default. The owner can always set or clear it before Done, even when the owner is both Asked by and the assignee. Asked by can set or clear it too, and the assignee can set it. If Asked by is not the owner and is also the assignee, their clear is refused; only the owner can clear it. Clearing isn't approving: the owner still can't Approve their own work. With it off, In progress → Done is allowed. The assignee, the owner included, can never approve their own task. With it on, Done without an Approve is refused, with "This task needs review: tasks review ID --reviewer …". Reopening keeps the flag and voids any earlier approval.
  - **AC-TK5-1:** In review needs an outcome line and a reviewer other than the assignee. The reviewer defaults to Asked by, and naming the assignee is refused. When Asked by is the assignee, there's no default and a reviewer must be named.
  - **AC-TK5-2:** Only the reviewer, or the owner unless the owner is the assignee, can decide. Approve sets Done and keeps the outcome; Done's guards still apply (required steps, and open subtasks), and a refused Approve names what's still open; for open subtasks: "T-12 has 2 open subtasks: T-13, T-14. Finish or cancel them first." `--anyway --reason` on Approve overrides only unticked required steps, never open subtasks. Request changes needs a comment and sets In progress. The assignee can withdraw, with a comment, which also sets In progress.
  - **AC-TK5-3:** Review by the owner shows as a "Review requested" Inbox Approval.
  - **AC-TK5-4:** While the task is In review, the owner, Asked by or the assignee can change the reviewer. Naming the assignee is refused. The timeline records "Reviewer changed from A to B by C".
  - **AC-TK5-5:** Blocking a task that's In review is refused, with "Withdraw it first".
- **TK-6 Reassignment.**
  - **AC-TK6-1:** Only the assignee, Asked by, the owner, the assignee's manager or the project manager can reassign (§4.2 defines both managers). An unassigned task can be assigned by Asked by, the owner, the project manager (the owner or a lead of a team attached to the project), or any member to themselves. If the project has no attached team, only the owner and Asked by can assign it to someone else. For an unassigned task there's no assignee, so the project manager acts alone in reassign or assign, cancel and reopen (for example, to reopen an unassigned Cancelled task). `assign` on a Done or Cancelled task is refused, with "Reopen it first".
  - **AC-TK6-2:** Reassignment sets the task to To do and adds a timeline entry: from, to, by whom, and the reason.
  - **AC-TK6-3:** When someone other than the owner assigns a task to the owner, by reassigning it or by assigning it from unassigned, the owner becomes the assignee, in To do, and a Hand-off Approval opens in the Inbox (§6.13); the task records that its Hand-off is open (§4.4). Accept counts as the owner's start: the task moves from To do to In progress, recorded as the owner's start, and "Picked up by" is the owner. The Hand-off stays open until the owner makes one of the moves below (§4.4). Any move by the owner (Accept, `start`, a tick, a block, or Done), the owner running `assign --none` on it, a reassignment away from the owner by anyone allowed, or a cancel clears it. Unblocking later doesn't reopen it. While it's open, the task shows in Work → Mine and Work → Needs me (AC-IA18-1), and in Following's Waiting on you. A task with the owner as reviewer (TK-5) also shows in Needs me. When the owner assigns a task to themselves, none of this applies: the task goes to To do in Mine, with no Hand-off, no Inbox item, and nothing in Needs me or Waiting on you. The owner declines a hand-off by reassigning the task to someone else or cancelling it; either clears the Approval. Tests: a member assigning an unassigned task to the owner opens a Hand-off; the owner assigning an unassigned task to themselves opens none.
  - **AC-TK6-4:** Reassigning a Blocked or In review task clears its "waiting on" and its reviewer, and the timeline keeps both. Exception: reassigning a task blocked "Budget reached" or "Parent budget reached" moves it to To do and turns the block into a budget hold. The wait, the Inbox item and the Needs me entry stay, and when the budget is resolved the hold lifts and the task stays To do for the new assignee. A reassigned subtask held by its parent's budget keeps the parent's stop: it gets no Inbox item of its own, and the parent's item stays. Tests: reassign a Budget reached task, check it is To do with the hold and the item, raise the budget, and check the hold lifts and the status stays To do; reassign a subtask blocked "Parent budget reached", check it is To do and held, has no Inbox item of its own, and the parent's item is still open.
  - **AC-TK6-5:** When the last assignee of a Done or Cancelled task is the owner and someone else reopens it, a Hand-off to the owner opens, with the same rules as AC-TK6-3. When the owner reopens it, no Hand-off opens. `reopen --start` by the owner on their own task opens none.
  - **AC-TK6-6:** Creating a task with an assignee (`tasks new --assignee`) is open to any member of the project, for anyone on the project, so agents can delegate. This decides only whom a new task may be assigned to; who may add a subtask or promote a step is AC-TK8-8. The only refusal is an assignee who isn't on the project, with an error that says so. Asked by is the message's sender with `--from-message`, and the creator otherwise. When anyone but the owner creates a task assigned to the owner, the task is in To do, a Hand-off to the owner opens and the owner follows the task, as AC-TK6-3, whoever Asked by is (including an agent creating it with `--from-message` from the owner's message). Only the owner creating a task for themselves opens none. This covers a subtask (`--parent`) and a promoted step (`--assignee owner`) too. Tests: a member creating a task with `--assignee owner` opens a Hand-off; the owner running `tasks new --assignee owner` opens none.
- **TK-7 Brief, comments and notes.**
  - **AC-TK7-1:** The task page has Comment and Private note composers. The private one shows the lock and the §4.5 line for the writer: "Only you can see this…" for the owner, and "Only you and the owner can see this…" for anyone else.
  - **AC-TK7-2:** Nobody but the author and the owner can read a note through the API, the CLI, search, Activity, exports, the live `work.changed` stream, replication or an agent's context. That includes other agents, people, reviewers and managers. Nor can they see that a note or a note event exists (§6.10). A non-author who edits, deletes or reads a note or note event by its ID gets the same "not found" answer as for a missing one. A create whose ID collides with a note's ID gets the same answer as any other ID collision, without naming notes (§6.4). Future watcher notifications are a read path too. A test proves each path, including the by-ID answers. The M-A limit in §6.4 is stated wherever it applies.
  - **AC-TK7-3:** The owner sees every agent's notes on a task, labelled "{Agent}'s private notes". The owner's own notes are visible to no agent.
  - **AC-TK7-4:** Comment and note edits show "edited". Only the author and the owner can open earlier versions; after a delete, only the owner can. In v1.0, only the owner can, until M-A is closed (§6.4). A test proves that earlier and deleted comment text can't be read by anyone else on any §6.4 read path. A deleted item shows "Comment deleted" or "Note deleted", and the owner can still read the original unless it was redacted (AC-TK7-6).
  - **AC-TK7-5:** Only Asked by and the owner can edit the brief (the title, the description or the definition of done). Anyone else, the assignee included, is refused with "Only Asked by or the owner can change the brief. Propose it in a comment." Editing the brief of a task that has ever been started needs a reason, which is posted as a comment linked to the diff. Versions shows every version. Adding, editing, removing, reordering, ticking or unticking a step isn't a brief edit: it needs no reason, makes no version and posts no "Changed the brief" comment. It's a plain timeline event.
  - **AC-TK7-6:** The owner, or the author of a brief version, can redact that version. It then shows "Redacted by {name}" to everyone. Redaction covers every diff and comment link that shows the redacted version, so its text can't be reached that way either. Redaction is for pasted secrets, and it's recorded in the timeline. **Ruling (2026-11-12, staff review): redaction erases.** The redacted text is physically erased from everything the app stores (the version, its diffs, earlier comment versions that hold it, and any search index); hiding it at read time isn't enough, because a secret must not stay in the database or its backups going forward. Nobody can read it afterwards, the owner included. The audit row stays: who redacted, when, and on what (the event and its place in the timeline), without the text. The same applies to any version of a comment or a private note, current, earlier or deleted: its author or the owner can redact it, with the same erasure (audit row kept, nobody can read it afterwards, the rotate reminder), from the UI or with `tasks comment redact` and `tasks note redact` (§5). The UI reminds the redactor to rotate the secret, since it may already have been seen or copied. How erasure is done, including existing backups, is Rowan's.
  - **AC-TK7-7:** When a comment or note fails to save, or the office is offline, the draft stays in the composer with "Couldn't save. Retry". Nothing typed is lost.
  - **AC-TK7-8:** Deleting an open question comment to the owner ends the wait: its Inbox item clears, and the task leaves Waiting on you and Needs me if nothing else waits on the owner. Editing the question keeps the wait and its Inbox item (§4.8). An open question on a Done task stays in Waiting on you until answered or deleted; that's intended. Cancelling a task ends an open question's wait: the question stays in the thread, closed, and its Inbox item clears.
  - **AC-TK7-9:** A system-posted comment, such as "Changed the brief: {reason}", can't be edited or deleted by anyone, the author and the owner included. The refusal says it's a system comment.
- **TK-8 Steps and subtasks.**
  - **AC-TK8-1:** A step can be promoted to a subtask with its own assignee.
  - **AC-TK8-2:** Adding a subtask to a subtask, or promoting a subtask's step, is refused. A subtask can have steps. `tasks new --parent ID` and "Add subtask" create a subtask directly.
  - **AC-TK8-3:** A parent can't be Done, or approved, while a subtask is open (Cancelled subtasks don't count). The refusal lists the open subtasks by key: "T-12 has 2 open subtasks: T-13, T-14. Finish or cancel them first."
  - **AC-TK8-4:** Cancelling a parent cancels its open subtasks with the same reason, and the timeline records each one. Being allowed to cancel the parent is enough; the subtasks' own cancel rules don't apply to the cascade. Each subtask's cancel is a system move, recorded with the actor who cancelled the parent, and shown as caused by the parent's cancel: "Cancelled with T-9 by Kai".
  - **AC-TK8-5:** While the parent is Done or Cancelled, `tasks new --parent`, promoting one of its steps, and reopening one of its subtasks are refused, with "Reopen T-9 first".
  - **AC-TK8-6:** Reopening a parent doesn't reopen its subtasks unless asked. They keep their status.
  - **AC-TK8-7:** Reopening a cancelled parent with "Also reopen the N subtasks cancelled with it" (CLI `reopen --with-subtasks`) reopens exactly the subtasks its cascade cancelled. Subtasks cancelled earlier, on their own, stay Cancelled. Like the cancel cascade, each subtask's reopen is a system move recorded with the caller; being allowed to reopen the parent is enough, and the subtasks' own reopen rules don't apply. Each reopened subtask whose last assignee is the owner opens a Hand-off to the owner, as a single reopen would (AC-TK6-5), unless the owner is the caller.
  - **AC-TK8-8:** Step changes follow §4.6's actors. The assignee, Asked by and the owner can add, edit, reorder and remove optional steps. Only Asked by and the owner can add a required step, make a step required or optional, edit a required step's text, or remove a required step; the assignee is refused, with an error that points to `done --anyway --reason`. The assignee and the owner can tick and untick any step; only the assignee's tick auto-starts a To do task (AC-TK4-5). The assignee, Asked by, the owner, the assignee's manager and the project manager can promote a step to a subtask or create a subtask (`--parent`, "Add subtask"); this is the only rule for it, and AC-TK6-6 doesn't apply. Anyone else is refused, with an error that names who can.
  - **AC-TK8-9:** A required step promoted to a subtask counts as ticked while its subtask is Done (completed), and unticks if that subtask is reopened. When the subtask is Cancelled, the step stays unticked, and the parent reaches Done only with `done --anyway --reason`. A step counted as ticked because its subtask is Done doesn't auto-start a To do parent; only the assignee's own tick does (AC-TK4-5).
  - **AC-TK8-10:** On a Done or Cancelled task, every step change (add, edit, reorder, remove, tick, untick, or making a step required or optional) is refused, with "Reopen it first".
  - **AC-TK8-11:** The tick on a promoted required step is derived, never stored on its own. Test: the subtask is Done and the step shows ticked. Reopen the subtask, and the step is unticked.
- **TK-9 Automatic figures and outcome.**
  - **AC-TK9-1:** Started, picked up, time in each status, cycle time and lead time are derived from events. None of them is ever typed. Cycle time and lead time leave out spans spent in Done or Cancelled (T8).
  - **AC-TK9-2:** Done needs an outcome line, and every required step ticked (or `--anyway --reason`, as today). `--anyway --reason` overrides only unticked required steps, recorded as done anyway, on every route to Done, Approve included. It never overrides open subtasks or Needs review. Open optional steps are listed under the outcome as "Not done".
  - **AC-TK9-3:** Points show only when set.
  - **AC-TK9-4:** The outcome line can hold links. The task page has an Output section listing the task's links and deliverables (docs, branches, commits, files). Links added with `tasks link` appear there, and `tasks unlink` removes them. In v1.0, branches and commits are linked by URL. A Done row in Following shows the outcome line and its first link.
- **TK-10 The CLI.**
  - **AC-TK10-1:** TK-10a provides, at least:
    - `tasks new` (also `add`), with `--from-message`, `--parent` and `--assignee` (AC-TK6-6);
    - `list` (with `--mine`, `--asked-by-me` and `--created-by MEMBER`) and `show`;
    - `start`, `current`, `step add` (with `--required`), `step edit`, `step rm`, `step move`, `step promote`, `checklist`, `check` and `uncheck`. Step commands aren't brief edits and take no `--reason` (AC-TK7-5);
    - `edit` (the brief: title, description and definition of done), by Asked by and the owner only, with `--reason` once the task has been started (AC-TK7-5);
    - `block --on … --reason …` (also on a Blocked task, to change what it waits on) and `unblock [--start]`;
    - `assign … [--reason]`, and `assign --none` (To do only, AC-TK4-10);
    - `link ID URL|path [--title]` and `unlink ID LINK`, for Output (§4.7);
    - `done --outcome` (also `--cancelled --reason`, and `--anyway --reason`);
    - `reopen --comment [--start] [--with-subtasks]`. `--start` also starts the task when the caller is the assignee or the owner, and is refused otherwise, with "Only the assignee or the owner can start T-12". `--with-subtasks` is AC-TK8-7;
    - `comment add` (with `--ask owner|MEMBER` for questions, and `--answer COMMENT-ID` for answers; design :829, W:1515), and `comment edit|delete|list|redact` (`redact <id> [--version n]`, author or owner only, AC-TK7-6).
  - **AC-TK10-1b:** TK-10b adds:
    - `note add|edit|delete|list|redact` (`redact <id> [--version n]`, author or owner only, AC-TK7-6);
    - `--points`.
  - **AC-TK10-1c (ships with TK-5, v1.1, §8):** `review`, `approve`, `request-changes`, `withdraw`, changing the reviewer (`tasks review ID --reviewer <member>` while In review; Rowan owns the exact flags), and `list --reviewing`.
  - **AC-TK10-2:** `tasks show` includes only the caller's own notes. The CLI help for `note` says that the owner can see notes, and that deleting a note doesn't erase it: the owner can still read the original unless it was redacted (AC-TK7-6).
  - **AC-TK10-3:** Writes are idempotent on a client-made ID.
  - **AC-TK10-4:** Every refusal names the fix. Checks run in one order, and the first failing check's text is returned: (1) is the action available in this state, (2) is the actor permitted (person-only actions, the budget-hold texts), (3) other guards (open subtasks, required steps) (§5). Test: on a held parent with open subtasks, an agent's Done gets "T-12 is on budget hold: only a person can mark it Done." and the owner's Done gets "T-12 has 2 open subtasks: T-13, T-14. Finish or cancel them first." The list below illustrates the style; it isn't exhaustive. The refusals marked TK-5 ship with it; the rest are in TK-10a:
    - Done without `--outcome`: "Done needs --outcome".
    - Done with required steps open: list them, and offer "check them or use --anyway --reason".
    - Done or Approve with open subtasks: "T-12 has 2 open subtasks: T-13, T-14. Finish or cancel them first."
    - Block without `--on` or `--reason`.
    - `assign` on a Done or Cancelled task: "Reopen it first".
    - `--parent`, promoting a step, or reopening a subtask while the parent is Done or Cancelled: "Reopen T-9 first".
    - Done while Needs review is set: "This task needs review: tasks review ID --reviewer …" (TK-5).
    - Review with the assignee as reviewer, or a reviewer change to the assignee (TK-5).
    - Approve by the assignee: nobody approves their own work (TK-5).
    - Block on an In review task: "Withdraw it first" (TK-5).
    - Unblock into In progress, or start, a task on budget hold: "T-12 is on budget hold: a person must raise it, allow 20% once or clear the budget."
    - `unblock` or `block --on` on a Budget reached block, by anyone: "T-12 is Blocked "Budget reached": a person must raise it, allow 20% once or clear the budget."; on a subtask's Parent budget reached block: "T-13 is Blocked "Parent budget reached": a person must raise T-12's budget, allow 20% once or clear it."
    - An agent's tick, or unblock into To do, on a held task: "T-12 is on budget hold: a person must raise it, allow 20% once or clear the budget."
    - `comment redact` or `note redact` by anyone but the author or the owner: refused, naming who can.
    - An agent on a held task: Done, "T-12 is on budget hold: only a person can mark it Done."; Approve, "T-12 is on budget hold: only a person can approve it." (TK-5); withdraw, "T-12 is on budget hold: only a person can withdraw it from review." (TK-5).
    - Start, block or Done on an unassigned task: assign it first.
    - Promoting a subtask's step, or `--parent` on a subtask.
    - Removing a required step, editing its text or making it optional, or adding a required step or making a step required, as the assignee: "S-3 is required: only Asked by or the owner can change it. Use done --anyway --reason".
    - Any step change on a Done or Cancelled task: "Reopen it first".
    - Editing the brief as anyone but Asked by or the owner: "Only Asked by or the owner can change the brief. Propose it in a comment."
    - `tasks new --assignee` (or promoting a step with an assignee) for someone who isn't on the project: refused, naming the project's members (AC-TK6-6).
    - `--from-message` with a message that wasn't addressed to the caller.
  - **AC-TK10-5:** No owner- or agent-facing surface (UI, CLI output or help) calls a shared comment a "note". "Note" means a private note only, from TK-10a on, so the wire's `CommentKind::note` (W:291) is shown as "comment".
  - **AC-TK10-6:** The agent instructions that ship with TK-10a tell agents to keep their working rationale in their own memory or journal, not in task comments. Comments are for facts, requirement changes, questions and results.
  - **AC-TK10-7:** `tasks edit ID --needs-review | --no-needs-review` (with TK-5) and `tasks edit ID --points N | --points none` (with TK-10b) aren't brief edits: they need no reason, make no brief version, and each change is recorded in the timeline. The flag follows §4.3's actors: the owner can always set or clear it, even as both Asked by and assignee, but another Asked by who is also the assignee can't clear it; points can be set by the assignee, Asked by or the owner. Anyone else is refused, naming who can.

## 8. Scope and sequencing [O]

- **v1.0, recommended:**
  - TK-2, which is small because `created_by` and its filter already exist.
  - TK-3, which is new work: verifying Asked by against a message, across offices.
  - **TK-10a, the minimal CLI** (AC-TK10-1), with its agent instructions (AC-TK10-6). Without it, agents can't record that the owner asked, and Following stays empty, because no task CLI exists today (`apps/org-agent` has messaging only).
  - **TK-8, all of it** (the depth rule, the Done guard on open subtasks, the cancel cascade, and promotion), because TK-10a's `--parent` needs these rules to keep the owner's "one level of subtasks, and no deeper".
  - **TK-6, all of it** (who can reassign, reset to To do, the reason, the Hand-off to the owner, clearing on reassign), because TK-10a's `assign` needs these rules to keep the owner's "reassignment must keep the record", and "Picked up by".
  - TK-4: statuses, including Blocked on whom. In review only arrives with TK-5. AC-TK4-3 and AC-TK6-3 need the two Inbox rows in §6.13, so they ship together. "Blocked on you" (FREEZE:77) also depends on the work-reads freeze.
  - TK-7's AC-TK7-5 and AC-TK7-6: brief versions, with a reason required once the task has been started, and redaction of a version that holds a pasted secret. This doesn't depend on privacy, and the owner's ask that the next agent starts from the original requirements plus the comments needs it in v1.0: changed requirements must show in the comments.
  - TK-7's AC-TK7-4 and AC-TK7-7 for **comments**: edit, soft delete and the failed-save state, because the owner asked for create, read, update and delete on comments.
    - Hiding superseded and deleted text needs the same read-path filtering as notes (§6.4), so it's Tier A too.
    - In v1.0, earlier and deleted versions are readable by **the owner only**. The author can read them as well once M-A is closed (§6.4).
    - Notes follow with the rest of TK-7.
  - TK-9's AC-TK9-1, AC-TK9-2 and AC-TK9-4: the derived figures, Done needs an outcome, and Output, which Following's Done row uses.
  - TK-1 Following, including "last seen". The unseen grouping, the bold rows, the divider and the badge all depend on it, so there's no partial version. AC-TK1-8 depends on the Inbox work-item filter (§6.15). A question to the owner reaches the Inbox only through the question row (FREEZE:73), which ships in slice IA-10c.
- **v1.1:**
  - TK-5 review: **v1.1** (ada's v1.0 sizing, from Rowan's `v1-prd-sizing.md`, 2026-11-13). D26's option (a) for Review requested stands until it ships; its CLI verbs ship with TK-5 in v1.1, not in TK-10a. A TK-5-sourced Review requested needs a new source row in Rowan's IA-10 freeze (FREEZE:71).
- **Next:**
  - the rest of TK-7 (needs the privacy enforcement, its security review (Tier A) and the M-A decision). Until then, agents keep their working rationale in their own memory or journal (§4.5, AC-TK10-6);
  - private notes were the owner's own ask (§1), so deferring them to Next was told to the owner on 2026-11-12. They move into v1.0 if the owner asks.
  - TK-9's AC-TK9-3: points;
  - TK-10b (AC-TK10-1b), with AC-TK10-7's `--points`. Its `--needs-review` ships with TK-5.
- **ada sequences.** Following adds a sidebar item, so nav freeze rev 5 needs a revision from Rowan.

## 9. Hand-offs

- **Rowan:** §6 1-16. In particular, "Asked by" verification, notes privacy across every read path, "last seen", status and resolution modelling, and the CLI shape.
- **Eden:**
  - the Following page and the Home card;
  - the two composers and the private-notes styling;
  - the status icons;
  - the "New since you last looked" divider;
  - the reviewer controls;
  - the Output section and nested subtasks in Following.
- **ada:** sequencing (§8).

## 10. How the Inbox is stored [F]

The owner asked for this. These are the facts as of `origin/v1` (`4acb522a`) and Rowan's IA-10 freeze (`ac-fullstack-engineer-rowan/knowledge/designs/ia-10-inbox-read-freeze.md`, called FREEZE below).

- **The Inbox has no store or table of its own.** It's computed when it's read, on HQ, by one adapter per source: org conditions, messaging asks, reviews and hand-offs, work questions and fixed bugs, and critical bugs (FREEZE:24, :67-79). On v1 there's a collector (`apps/org-server/src/inbox.rs`: the `Source` trait at :79-81, `read`/`collect` at :95/:101, `recount` at :144) and the wire types (`crates/org-proto/src/inbox.rs`, `inbox.list` at :203).
- **An item's identity is a `ref`** built from origin office, source, ID and occurrence. The fields are source, kind, member, project, team, summary, context, recommendation, options, created and expiry times, state (open or handled), handled time, outcome, clearing rule, whether it's dismissable, and its actions (`org-proto/src/inbox.rs:96-124`).
- **Items clear by construction.** Each source recomputes from its own resolved state. "Handled" (the last 30 days) is derived from each source's resolution times; there's no separate history table (FREEZE:25-26, :78-79).
- **What's built** (`origin/v1` at `4acb522a`, the IA-10a1b merge):
  - the wire types and the collector;
  - `inbox.list`, served on HQ only and only to the live owner (`apps/org-server/src/serve.rs:898-920`, routed only when HQ has an inbox and only for the live owner; the handler is `Hq::list` at `apps/org-server/src/inbox/list.rs:10-53`, with its parameter checks at :11-27). HQ advertises the `inbox.list` capability;
  - the first real source, org conditions (`apps/org-server/src/inbox/org_source.rs`: the `OrgSource` struct at :10-13 and its `Source` impl at :24-109, with `read` producing the items at :29-108).
- **Still to come**, designed (FREEZE:67-79) and built in the later IA-10 slices (FREEZE:81-88): the messaging, reviews and hand-offs, work, and critical-bug sources, and `inbox.changed` (its wire type exists, but no office advertises it yet). The task-driven rows in §6.13 and the work-item filter in §6.15 come on top of these.
- **Agents' `~/.agency/spool` is unrelated.** It's the old file-based message relay for agents. ADR-0003 supersedes it, except for agents started by hand (`docs/adr/0003-agent-messaging-office-first.md:25`, :252-256).

**What this means for Tasks:**
- Task-driven Inbox items (§4.8) are simply a work adapter reading task state. They add nothing new to store.
- Following's "last seen" (§6.7) is the one new piece of per-owner state, and Rowan chooses where it lives.
