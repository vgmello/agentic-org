Source: tasks-v1.md (Rev 11) and projects-planning-v1.md (Rev 17) (current revisions).

# Acceptance tests: Tasks and Planning

QA input for engineering sizing. One line per check: Given / When / Then, level tag, AC ID(s).
Levels: [unit] pure rules; [integration] server, storage and CLI; [UI e2e] owner-visible UI; [manual] can't be automated.
Fixtures follow the PRDs: project `chk` (and `pay`, P-2), owner, agents Kai and Sage, human project manager Ana, agent project manager ada, tasks T-9 (parent) with subtasks T-12, T-13, T-14, releases R-1 to R-5, milestones M-1 to M-4.

## Tasks (tasks-v1.md)

### TK-1 · Following
1. Given subtask T-12 under followed T-9 becomes blocked on the owner with nothing else unseen, when the owner opens Following, then T-12 shows in Waiting on you labelled "Subtask of T-9", T-9's row is bold and the badge reads 1; when the owner then opens T-12 from the Inbox (or opens T-9 instead), then both rows clear and the badge is 0. [UI e2e] (AC-TK1-3, AC-TK1-9)
2. Given T-9 is followed and Kai changes two of its nested subtasks after last seen while the owner changes a third, when the badge is computed, then it is 2, T-9 is not counted on its own, and a cancel cascade the owner started adds nothing while one Kai started counts. [unit] (AC-TK1-1, AC-TK1-3, AC-TK1-7)
3. Given a task that waits on the owner and doesn't match "Created by me", when the owner filters Following by Created by me, then it still shows in Waiting on you; when the wait ends and the task stays followed, then it shows under All only. [integration] (AC-TK1-10, AC-TK1-2)
4. Given the owner unfollowed a task they created, when it becomes blocked on the owner, then it is followed again and Unfollow is disabled with "You can unfollow once this no longer waits on you"; after the wait ends it stays followed until the owner unfollows it. [integration] (AC-TK1-6, AC-TK1-2)
5. Given T-12 is nested under followed T-9, when the owner opens T-12's page, then Unfollow is disabled with "Unfollow T-9 to stop following this subtask"; a subtask followed on its own row can be unfollowed, and no owner-facing text says "watch". [UI e2e] (AC-TK1-11, AC-TK1-6)
6. Given a Waiting on you row, when the owner clicks its link, then the Inbox opens filtered to that task with only its open items, the group may be amber, and the sidebar badge stays neutral and is hidden at 0. [UI e2e] (AC-TK1-8, AC-TK1-1)

### TK-2 · Work views
1. Given the owner on Work, when they open Show ▾ and Created by ▾, then Created by me and Asked by me are offered and Created by lists every member, person or agent. [UI e2e] (AC-TK2-1)
2. Given a task the owner created and assigned to Kai, when the owner picks Mine, then it isn't listed (Mine means assigned to me). [UI e2e] (AC-TK2-2)
3. Given one task followed by hand and one followed only automatically, both closed more than 30 days ago, when the owner opens Work → Followed, then only the hand-followed task is listed and neither is in Following. [integration] (AC-TK2-3)

### TK-3 · Asked by
1. Given the owner sends Kai a direct message asking for work, when Kai runs `tasks new --from-message ID` with a description, then Asked by is the owner. [integration] (AC-TK3-1)
2. Given a channel message that doesn't mention Kai, when Kai runs `tasks new --from-message` with its ID, then the create is refused. [integration] (AC-TK3-1, AC-TK10-4)
3. Given no `--from-message`, when Kai creates a task and also tries any other way to set Asked by, then Asked by is Kai, the creator, and the other attempt is rejected. [integration] (AC-TK3-2)
4. Given `--from-message` and no description, when Kai creates the task, then it is refused. [integration] (AC-TK3-4)
5. Given a task with Asked by the owner, when Sage (who can't read the message) and the owner view it, then both see "Asked by {name}" and only the owner sees the message link. [UI e2e] (AC-TK3-3)

### TK-4 · Statuses
1. Given every (from, to, actor) pair, when run through the §4.2 transition table, then the listed moves pass and everything else is refused: Asked by or a manager (not the owner) can't start, block, unblock or mark Done Kai's task, the assignee can't cancel a task they didn't ask for, the last assignee can't reopen a Cancelled task, Cancel on Done says "Reopen it first", and each refusal names who can. [unit] (AC-TK4-4, AC-TK4-9, AC-TK4-11, AC-TK4-15)
2. Given Kai's To do T-12, when Kai ticks a step, then it moves to In progress as an automatic event whose cause is the tick, Picked up by Kai at server time; untick leaves it In progress, `work.undo` on the auto-start is refused with "Reassign or cancel instead", and the owner's tick on Kai's To do task leaves it To do. [integration] (AC-TK4-5)
3. Given Kai's To do and In progress tasks, when Kai blocks each with `--on` and a reason and then unblocks, then each returns to its earlier status with waiting on cleared; a block without a reason is refused; `unblock --start` on the To do one starts it; To do → Done records "Started and done"; start, block or Done on an unassigned task is refused with "assign it first". [unit] (AC-TK4-6, AC-TK4-7, AC-TK4-8, AC-TK4-14, AC-TK4-16)
4. Given T-12 blocked on the owner, when an Approval or Question on it is open, then "Blocked on you" is withdrawn and returns after a refusal; `block --on Sage` clears it; and a task at a stopped budget with an open question keeps its Budget reached item. [integration] (AC-TK4-3, AC-TK4-17)
5. Given In progress T-12 reaches its budget, then it is Blocked "Budget reached" on the owner with no actor and a Budget reached Inbox item; `unblock` by Kai and by the owner are refused with "T-12 is Blocked "Budget reached": a person must raise it, allow 20% once or clear the budget."; ada's Raise and ada's move of a cost record off Done subtask T-13 are refused with "Only a person can raise, clear or extend a budget."; the owner's Raise still at or below spent is refused and changes nothing; Ana's Raise above spent returns it to In progress recorded as caused by that action. [integration] (AC-TK4-18)
6. Given held tasks at a stopped budget, then: the owner approves a held In review task and it is Done with the hold and item cleared; an agent's Done is refused with "T-12 is on budget hold: only a person can mark it Done."; a human assignee's tick on held To do is recorded and it stays To do while an agent's tick is refused; a human reviewer's request changes moves it to Blocked "Budget reached"; a top-level task Blocked on Ana keeps waiting on Ana after the budget is raised; reopening a Done task at its budget gives an immediate hold. [integration] (AC-TK4-19)

### TK-5 · Review
1. Given Needs review is set, when the assignee runs Done from In progress or To do, then it is refused with "This task needs review: tasks review ID --reviewer …"; only Approve reaches Done; reopening keeps the flag and voids the earlier approval. [unit] (AC-TK5-0, AC-TK4-8)
2. Given Kai is both Asked by and assignee, when Kai clears Needs review, then it is refused, while the owner (even as both Asked by and assignee) can clear it and still can't Approve their own task. [unit] (AC-TK5-0)
3. Given Kai moves T-12 to In review, then an outcome line and a reviewer other than Kai are required, the reviewer defaults to Asked by, and when Asked by is Kai there is no default. [unit] (AC-TK5-1)
4. Given parent T-12 In review with open T-13 and T-14, when the reviewer approves (with or without `--anyway --reason`), then it is refused with "T-12 has 2 open subtasks: T-13, T-14. Finish or cancel them first."; Approve by the assignee is refused. [unit] (AC-TK5-2, AC-TK8-3)
5. Given T-12 In review, when the owner changes the reviewer from Sage to Ana, then the timeline reads "Reviewer changed from Sage to Ana by {owner}"; naming the assignee is refused; blocking it is refused with "Withdraw it first"; request changes and withdraw each need a comment and set In progress. [integration] (AC-TK5-4, AC-TK5-5, AC-TK5-2)
6. Given Kai names the owner as reviewer, when the owner opens the Inbox, then a "Review requested" Approval for T-12 is there. [UI e2e] (AC-TK5-3)

### TK-6 · Reassignment
1. Given each actor role, when reassigning or assigning, then only the assignee, Asked by, the owner, the assignee's manager or the project manager may reassign; an unassigned task is assigned by Asked by, the owner, the project manager or a member to themselves; with no attached team only the owner and Asked by assign it to someone else; `assign` on Done or Cancelled says "Reopen it first". [unit] (AC-TK6-1)
2. Given Kai's Blocked T-12 with waiting on and In review T-13 with a reviewer, when Kai reassigns them to Sage with a reason, then both are To do, waiting on and reviewer are cleared and kept in the timeline as "Reassigned from Kai to Sage by Kai: {reason}"; but a task blocked "Budget reached" becomes To do with a budget hold, its item and Needs me entry kept, and stays To do after the budget is raised. [integration] (AC-TK6-2, AC-TK6-4)
3. Given an unassigned task, when Kai assigns it to the owner, then it is To do, a Hand-off Approval opens, the task records it, and it shows in Mine, Needs me and Waiting on you; Accept moves it to In progress as the owner's start with Picked up by the owner; the owner assigning to themselves opens none. [integration] (AC-TK6-3)
4. Given an open Hand-off to the owner, when each listed move happens (owner Accept, start, tick, block or Done; owner `assign --none`; reassign away; cancel), then it clears; a comment or step edit leaves it open; a later unblock doesn't reopen it. [unit] (AC-TK6-3)
5. Given a Done task whose last assignee is the owner, when Kai reopens it, then a Hand-off opens; when the owner reopens it (or runs `reopen --start`), then none opens. [integration] (AC-TK6-5)
6. Given Kai runs `tasks new --assignee owner` (also with `--from-message` from the owner's message, and with `--parent`), then a Hand-off opens and the owner follows the task; the owner's own `tasks new --assignee owner` opens none; an assignee not on the project is refused, naming the project's members. [integration] (AC-TK6-6, AC-TK10-4)

### TK-7 · Brief, comments and notes
v1.0 scope only: AC-TK7-4 to AC-TK7-9. Private notes (AC-TK7-1 to AC-TK7-3) are Next (§8).
1. Given Kai is the assignee, when Kai edits the brief, then it is refused with "Only Asked by or the owner can change the brief. Propose it in a comment."; once the task has ever started, Asked by's edit without a reason is refused and with one posts "Changed the brief: {reason}" linked to a diff; step changes need no reason, make no version and post no comment. [unit] (AC-TK7-5)
2. Given Kai edits then deletes a comment, when Sage reads it through the API, CLI, search, Activity, exports, `work.changed`, replication and agent context, then Sage sees "edited" and "Comment deleted" only, and in v1.0 only the owner can open earlier and deleted versions. [integration] (AC-TK7-4)
3. Given a brief version and a comment version holding a pasted secret, when the owner redacts them, then the text is erased from the version, its diffs, earlier comment versions and the search index, nobody (the owner included) can read it, "Redacted by {name}" shows, and the audit row stays; `tasks comment redact` by Sage (not author or owner) is refused, naming who can. [integration] (AC-TK7-6, AC-TK10-4)
4. Given an open question from Kai to the owner, when Kai edits it, then the wait and Inbox item stay; when Kai deletes it, or the task is cancelled, then the item clears and the task leaves Waiting on you and Needs me; on a Done task the open question stays in Waiting on you. [integration] (AC-TK7-8)
5. Given a system comment "Changed the brief: {reason}", when its author or the owner edits or deletes it, then both are refused with a message saying it's a system comment. [integration] (AC-TK7-9)
6. Given the office goes offline, when the owner saves a comment or a status move with its reason, then the draft stays in the composer with "Couldn't save. Retry". [UI e2e] (AC-TK7-7, AC-TK4-13)

### TK-8 · Steps and subtasks
1. Given subtask T-12 of T-9, when anyone runs `tasks new --parent T-12` or promotes one of T-12's steps, then both are refused; T-12 can still have its own steps. [unit] (AC-TK8-2)
2. Given T-9 with open T-13 and T-14 and Cancelled T-12, when the assignee marks T-9 Done, then it is refused with "T-12 has 2 open subtasks: T-13, T-14. Finish or cancel them first." (named by key), and Cancelled T-12 doesn't count. [unit] (AC-TK8-3)
3. Given T-9 with open T-13, T-14 and T-12 cancelled earlier on its own, when Kai cancels T-9 and later reopens it with `--with-subtasks`, then T-13 and T-14 show "Cancelled with T-9 by Kai" without their own cancel checks, only they reopen, T-12 stays Cancelled, and a reopened one whose last assignee is the owner opens a Hand-off; plain reopen leaves subtasks as they are. [integration] (AC-TK8-4, AC-TK8-6, AC-TK8-7)
4. Given T-9 is Done or Cancelled, when anyone runs `--parent T-9`, promotes a T-9 step or reopens a T-9 subtask, then each is refused with "Reopen T-9 first"; every step change on T-9 is refused with "Reopen it first". [unit] (AC-TK8-5, AC-TK8-10)
5. Given a required step promoted to subtask T-13, when T-13 is Done, reopened, then Cancelled, then the step reads ticked, unticked, unticked (derived, not stored); with T-13 Cancelled, the parent reaches Done only with `--anyway --reason`; the derived tick never auto-starts a To do parent. [unit] (AC-TK8-9, AC-TK8-11)
6. Given each actor, when changing steps and subtasks, then the assignee adds and edits optional steps but is refused on required ones with an error pointing to `done --anyway --reason`; the assignee, Asked by, the owner and both managers can promote a step to a subtask with its own assignee or add a subtask; anyone else is refused, naming who can. [unit] (AC-TK8-8, AC-TK8-1)

### TK-9 · Automatic figures and outcome
1. Given an event log (created, started, blocked, done, reopened, started, done), when figures are derived, then Started, Picked up, time in each status, cycle time and lead time match, and spans in Done or Cancelled are left out. [unit] (AC-TK9-1)
2. Given a client that supplies its own timestamp on a write, when the server records the event, then it uses the server's time and actor, and no figure can be typed. [integration] (AC-TK9-1)
3. Given T-12 with an open required step and an open optional step, when Done is run without `--outcome`, then "Done needs --outcome"; with an outcome, the required step is listed with "check them or use --anyway --reason"; with `--anyway --reason` it is Done, recorded as done anyway, and the optional step is listed as "Not done"; `--anyway` never overrides open subtasks or Needs review. [unit] (AC-TK9-2)
4. Given Kai runs `tasks link T-12 <URL> --title …` and later `tasks unlink`, then the link appears in, then leaves, the task page's Output section. [integration] (AC-TK9-4)
5. Given a followed task Done with an outcome line containing a link, when the owner opens Following, then the row shows the outcome line and its first link. [UI e2e] (AC-TK9-4)

### TK-10 · The CLI
1. Given a held parent T-12 with open subtasks, when an agent marks it Done, then "T-12 is on budget hold: only a person can mark it Done."; when the owner does, then "T-12 has 2 open subtasks: T-13, T-14. Finish or cancel them first." (check order: state, actor, other guards). [integration] (AC-TK10-4)
2. Given a write sent twice with the same client-made ID, when the server processes both, then exactly one event is recorded. [integration] (AC-TK10-3)
3. Given a Done task assigned to Kai, when Sage runs `tasks reopen --comment … --start`, then it is refused with "Only the assignee or the owner can start T-12". [integration] (AC-TK10-1)
4. Given the TK-10a build, when each AC-TK10-1 command runs with `--json` and `--office`, then it exists, succeeds on a valid call and prints compact one-line-per-item output by default. [integration] (AC-TK10-1)
5. Given a wire comment of kind `note`, when it is shown in the CLI output, CLI help and the UI, then it reads "comment", never "note". [integration] (AC-TK10-5)
6. Given the agent instructions that ship with TK-10a, when reviewed, then they say to keep working rationale in the agent's own memory or journal, not task comments, and not to post started or done messages in chat. [manual] (AC-TK10-6)

## Planning (projects-planning-v1.md)

### PL-1 · Keys
1. Given `chk` is P-2 with T-12, when `T12`, `t12`, `T-12`, `chk-T12`, `CHK-T-12`, `chk-T-12`, `P-2-T-12` and `p-2-t12` are parsed, then all resolve to the same task; `T-99` errors with "No T-99 in chk. Run `org-agent tasks list --project chk` to see all its items." [unit] (AC-PL1-2, AC-PL1-6)
2. Given a legacy project keyed `p2` and, separately, none, when `P2-T-12`, `projects show P2` and `P-2` are resolved, then `P2` names `p2` when it exists and P-2 otherwise, `P-2` always names number 2; `t12345` resolves to T-12345 if it exists, else to the short id; `projects show P12345` never falls back to a short id. [unit] (AC-PL1-6, AC-PL1-8, AC-PL1-10, AC-PL1-12)
3. Given `chk` and P-2, when releases, milestones and tasks are created, a task deleted with `work.delete` and R-1 cancelled, then each project starts R-1, M-1, T-1, the next task gets a new number, the next release is R-2, and creating a project keyed `p2` or `t12` is refused naming the rule. [integration] (AC-PL1-1, AC-PL1-3, AC-PL1-7)
4. Given an agent on `chk` and `pay` with no current task, when it runs `releases show R-1` without `--project`, then "Which project? Use chk-R-1 or --project chk."; with a current task in `chk` it resolves there; the owner with two projects gets "Which project? Add --project <key>, or use chk-R-1."; with only `chk`, it shows chk's R-1. [integration] (AC-PL1-9)
5. Given existing data with B-1 created between T-1 and T-2, when the backfill runs, then projects get P-n in creation order (unknown dates last, then by name) and the backlog order is T-1, B-1, T-2. [integration] (AC-PL1-11)
6. Given the owner on chk's project page, when they type `T12`, then it resolves to chk-T-12; global search lists "chk-T-12", "pay-T-12" in that order; in a chat thread with no project only `chk-T-12` becomes a link. [UI e2e] (AC-PL1-14)

### PL-2 · Releases
1. Given Planned releases, when "started" is evaluated, then R-1 starts when subtask T-12 of planned T-9 starts; a started Blocked T-5 planned into R-2 starts it, a T-5 blocked straight from To do doesn't; a Done T-6 with a Started event and a To do → Done T-7 leave R-3 Planned; a To do T-9 with a started subtask planned into R-5 starts it; an In progress release never returns to Planned. [unit] (AC-PL2-7)
2. Given R-1 holds open T-4, Fixed B-4 and Triaged B-5, when `releases release R-1` runs without `--open-items`, then "R-1 has 2 open items: T-4, B-5. Add --open-items next, backlog or R-n." and nothing changes; `--open-items R-1` and a Released `--open-items R-2` are refused with their texts; with a started Blocked T-8 and `--open-items R-4` (Planned), T-8 moves to R-4 and R-4 goes In progress, recorded as the releaser's. [integration] (AC-PL2-19, AC-PL2-7, AC-PL2-16)
3. Given no other open release, when releasing with `--open-items next`, then "No other open release. Use --open-items backlog, or create one first."; with several, `next` picks the earliest target (no date last, then number) and milestone items land without a milestone. [integration] (AC-PL2-12)
4. Given a member with "Can plan releases" who isn't a project manager, when they create a release, then it succeeds; when they reopen a Released R-1, then "Only the owner or a project manager can reopen R-1."; a non-planner creating one gets the planners message; `projects planners add` by anyone but the owner gets "Only the owner can change who can plan releases.". [integration] (AC-PL2-5, AC-PL2-8, AC-PL2-18)
5. Given R-1 released while Planned with M-1 closed with it and M-2 closed by hand earlier, when the owner reopens it with a reason, then R-1 is In progress, "Released on {date}" is history only, M-1 reopens and M-2 stays closed; without a reason it is refused; a release cancelled while Planned reopens to Planned. [integration] (AC-PL2-8, AC-PL2-14)
6. Given R-1 holds Done T-4, Fixed B-4 and Triaged B-5 (plus a Cancelled and a deleted task), when progress is computed, then it is 2 / 3 over top-level items only; a release with nothing counted shows "—" with "Nothing planned yet", never "0 / 0". [unit] (AC-PL2-4, AC-PL2-21)

### PL-3 · Milestones
1. Given R-1 is Released with M-2, when a planner reopens M-2, moves it to R-2, creates a milestone on R-1 or edits M-2's name, then each is refused with "R-1 is Released: reopen it first" and no number is used; editing M-2's description is allowed. [integration] (AC-PL3-10, AC-PL3-11, AC-PL3-12, AC-PL3-15)
2. Given open M-2 on open R-1, when it is moved into Released R-3, then "R-3 is Released: move M-2 to an open release"; from a Released R-1 into a closed R-3, the source's "R-1 is Released: reopen it first"; a Done M-2 gets "M-2 is Done: reopen it, then move it". Nothing moves. [integration] (AC-PL3-7, AC-PL3-11, AC-PL3-16)
3. Given M-2 holds open T-4, when `milestones close M-2` runs without `--confirm`, then it is refused with the "Add --confirm to close it" text and M-2 stays Open with T-4 on it; with `--confirm` M-2 is Done and T-4 stays on R-1 without a milestone, timeline "Moved from M-2 to R-1: M-2 done"; `cancel` behaves the same with its own text. [integration] (AC-PL3-18, AC-PL3-9, AC-PL3-19, AC-PL3-6)
4. Given M-2 is Done on open R-1, when T-12 is planned into it, then "M-2 is Done: reopen it or pick another milestone"; with R-1 Released, planning into its open milestone gets "R-1 is Released: reopen it or pick another release" instead. [integration] (AC-PL3-4, AC-PL3-13)
5. Given M-2 holds a started Blocked item, when it moves from R-1 into Planned R-2, then R-2 goes In progress, recorded with the mover. [integration] (AC-PL3-14)
6. Given a member who isn't a release planner, when they create, edit, close, cancel, reopen or move a milestone, then each is refused with the milestones planners message, nothing changes and no number is used. [integration] (AC-PL3-17, AC-PL3-2, AC-PL3-8)

### PL-4 · Planning
1. Given T-12 planned on M-2, when it is planned into R-3, then it is on R-3 only, never in two places. [unit] (AC-PL4-1)
2. Given Kai, not a planner, when Kai plans his own T-12 and Sage's T-13, then T-12 is planned and T-13 is refused with "T-13 isn't yours: only release planners can plan it"; a planner can plan both. [integration] (AC-PL4-2)
3. Given R-1 is Released and M-2 is Done, when `tasks new --release R-1` and `tasks new --milestone M-2` run, then they are refused with the release and milestone messages and no task is created. [integration] (AC-PL4-2)
4. Given subtask T-12 of T-9, when T-12 is planned on its own, then "T-12 is a subtask: plan the parent, T-9"; `tasks new --parent T-9 --release R-1` gets "Subtasks follow their parent's plan."; replanning T-9 moves its subtasks with it. [integration] (AC-PL4-4, AC-PL4-5, AC-PL4-6)
5. Given Done T-12, when it is planned or unplanned by hand, then "T-12 is Done: reopen it before planning it" and its plan is kept. [integration] (AC-PL4-6)
6. Given Kai plans T-12 into R-1, then its timeline reads "Planned into R-1 by Kai". [integration] (AC-PL4-3)

### PL-5 · Backlog
1. Given R-1 closes with `--open-items backlog`, M-1 (12 Dec: T-8, T-2), M-2 (5 Dec: T-3, B-3) and direct T-5, then the backlog top reads T-3, B-3, T-2, T-8, T-5; `tasks plan T-9 T-4 --backlog` puts T-9 above T-4; a new item lands at the bottom; a later return lands above an earlier one. [unit] (AC-PL5-4, AC-PL5-2, AC-PL5-9)
2. Given backlog T-10, T-11, T-12, T-13, when `T-13 T-12` run with `--rank top`, `--rank bottom`, `--before T-10` and `--after T-10`, and `T-13 T-11 --rank up`, then the orders are exactly as AC-PL5-12 states (top: T-13, T-12, T-10, T-11; up: T-11, T-10, T-13, T-12). [unit] (AC-PL5-12)
3. Given backlog T-10, T-11, T-12, when `T-10 T-12 --rank up` runs, then nothing changes and "T-10 is already first"; `T-10 T-12 --rank top` gives T-10, T-12, T-11. [unit] (AC-PL5-8)
4. Given T-13 planned in R-1 and T-14 a subtask of T-9, when `tasks plan T-12 T-14 T-13 --rank top` runs, then the error lists T-14, then T-13, with fixes, and nothing moves; `--before T-9` (planned) gets "T-9 is not in the backlog: --before and --after take a backlog item."; `--backlog --rank up` is refused with its text. [integration] (AC-PL5-8, AC-PL5-6)
5. Given T-12, T-13 planned and T-14 fifth in the backlog, when `tasks plan T-13 T-14 T-12 --backlog` runs, then T-13, T-12 are rows 1-2 and T-14 is seventh; `T-14 T-12 --backlog --rank top` gives T-14, T-12; `--backlog --before T-11` lands before T-11; bare `--backlog` on an item already in the backlog records nothing and keeps its rank. [integration] (AC-PL5-10, AC-PL5-11)
6. Given rows 3 and 5 selected on the Backlog tab, when Move up is used, then they are rows 2 and 4; Move to top makes them rows 1 and 2; Move up is disabled when row 1 is selected; a non-planner sees moves only on their own items. [UI e2e] (AC-PL5-7, AC-PL5-5)

### PL-6 · Release views
1. Given open R-1 (20 Dec: M-2 12 Dec, M-1 no date), R-2 (5 Dec: M-3), R-3 (no date) and Released R-4, when the picker order is computed, then it is R-2, M-3, R-1, M-2, M-1, R-3, Backlog; Move to another release for M-2 gives R-2, R-3; the Releases tab lists R-2, R-1, R-3 with closed ones under "Closed", newest close first. [unit] (AC-PL6-7, AC-PL6-5)
2. Given M-1 (no target), M-2 (12 Dec), M-3 (5 Dec) and Done M-4, and a section holding B-3, T-3, T-2, when the release page renders, then milestones show M-3, M-2, M-1, M-4 and the section shows T-2, T-3, B-3. [UI e2e] (AC-PL6-2)
3. Given a non-planner, a planner who isn't a project manager, and the owner, when each opens a Released release page, then the non-planner sees no header or milestone actions and "Plan ▾" only on own items, the planner sees no Reopen, and the owner sees Reopen, whose dialog can't be confirmed without a reason. [UI e2e] (AC-PL6-11, AC-PL6-9)
4. Given the owner on Home, when they open a project then its Releases tab, then each release's progress shows within two clicks; from a release page, "Plan ▾" then R-2 moves the item in two clicks and its timeline records it. [UI e2e] (AC-PL6-6, AC-PL6-12)
5. Given the "New task" form, when the owner picks R-1 in Plan, then the task is on R-1 with "Planned into R-1 by {creator}"; left on Backlog, it lands at the bottom; a subtask form has no Plan field. [UI e2e] (AC-PL6-13)
6. Given an unknown R-9, when the owner opens `/projects/:projectId/releases/R-9`, then "No release R-9 in this project" shows with a link back to the Releases tab. [UI e2e] (AC-PL6-1)

### PL-7 · CLI
1. Given releases and milestones in mixed states, when `releases list`, `releases show R-1` and `milestones list` run, then order and progress match the UI (picker order, closed newest close first, "—" with "Nothing planned yet" at 0 / 0), and items list as T-2, T-3, B-3. [integration] (AC-PL7-3)
2. Given open T-1 and closed T-5 then T-6, when `backlog list --closed` runs, then it lists T-1, T-6, T-5. [integration] (AC-PL7-3)
3. Given T-12 on M-2 of R-1, T-13 directly on R-1 and T-14 unplanned, when `tasks show` runs on each, then it prints "R-1 · M-2", "R-1" and "Backlog". [integration] (AC-PL7-4)
4. Given unknown T-99, R-9, M-9 and P-9, when each is named, then the hint names `tasks list --project chk`, `releases list --project chk`, `milestones list --project chk` and `projects list`. [integration] (AC-PL7-5)
5. Given each §5 refusal, when triggered, then the exact text names its fix (table-driven); `releases delete R-1` gets "Releases can't be deleted: cancel R-1 instead" and nothing changes. [integration] (AC-PL7-2, AC-PL2-11)
6. Given every §5 command, when run with `--json` and `--office` and retried with the same client ID, then it succeeds once with compact output. [integration] (AC-PL7-1)

### PL-8 · Drag and rank (later)
1. Given a backlog item, when the owner drags it into a release, then it is planned there and its timeline records the move. [UI e2e] (PL-8, §4.6)
2. Given the backlog, when the owner drags a row to a new position, then its rank changes to match. [UI e2e] (PL-8, §4.6)
3. Given a non-planner, when they drag another member's item, then it is refused (re-rank rights). [UI e2e] (PL-8, §4.6)

### PL-9 · Feature targeting (later)
1. Given FEAT-n targeted to R-1, when the release page loads, then it lists the feature. [UI e2e] (PL-9, §4.8)
2. Given the release page, when it renders, then features come after the directly planned items. [UI e2e] (PL-9, §4.9)

### PL-10 · Burn-up chart (later)
No checks here: moved to `project-insights-v1.md` as IN-4.

## Gaps
- PL-8, PL-9, PL-10 have no AC IDs; checks above trace to section text only. PL-9 supports only 2 checks; PL-10 has none here (moved to IN-4).
- AC-PL5-7 keyboard moves: the keys are for Eden to design, so the keyboard half can't be tested yet.
- AC-PL1-4 (cross-project move, not built) and AC-PL1-5 (phase 2 satellites): untestable in v1.0.
- AC-PL2-4 points branch: points are Next (AC-TK9-3), so only count-based progress can be tested.
- AC-TK7-6 erasure "including existing backups" is left to Rowan; no checkable backup criterion yet.
- AC-TK3-1 refusal and AC-TK10-4 "Block without --on or --reason" have no exact text.
- TK-5 is v1.0 only "if ada can fit it"; In review group visibility (AC-TK1-2, AC-TK4-1) depends on that call.
- AC-TK4-18/19 budget behaviour depends on `costs-and-budgets-v1` (Raise, Allow once, cost-record moves); fixtures need that PRD.
- AC-TK7-1 to AC-TK7-3 and AC-TK10-2 (private notes, TK-10b) are Next and wait on the M-A decision; not listed.
