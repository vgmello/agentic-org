# Product review of the v1 implementation plan

Owner: Eden (`ac-product-designer-eden`) · 2026-11-10 · for ada (backlog #34). Plan read at its 2026-11-10 version; specs at `docs/specs` on `v1` `4e15a89`.
Focus: product slices and acceptance placement. Not effort or architecture.
Plan: `ac-staff-engineer-ada/knowledge/plans/v1-implementation-plan.md` (455 lines). Specs: `docs/specs/` at `4e15a89`.
Scope note (the owner, 2026-11-10): v1.0 = SRV-1..32 plus the dogfood run. Repository work, delegates, channels and processes stay in Wave A. O-1 and O-2 take the plan's defaults. Nothing below asks to move Wave work into v1.0.

## Summary

- **Counts and ranges are right.** All 48 criterion groups (443 IDs, 17 retired) match the plan's table. No group has a wrong range or a retired ID in use.
- **Placement has gaps.** CHAT has no milestone exit at all, DEAL is in no exit, and PROC-20..30 sit in no wave. 3 high, 6 medium and 8 low findings.
- **Two slices don't hold together as written.** M4's demo and exit need M3's running agent, but the plan runs them in parallel. M2 promises Hire (STAFF-1..3), but Hire's Suggestion step is the HR lead's draft, which can't run until M3.
- **Dogfood coverage is complete.** Every row in sections 1–5 and every section 6 check maps to a task by M5.
- **O-7:** M2 can ship the default, but only with small spec changes. Ten criteria or flow texts need new wording, and nine interim states are missing. Most of these states are also needed later, whenever the HR lead can't run (no runtime installed, signed out), so they should be permanent.

## A. Mapping findings

### High

**A-H1. CHAT has no milestone exit, and channel criteria outside CHAT-19..23 aren't placed.**
- Evidence: the M3 exit (plan line 179) lists no CHAT IDs, yet the M3 demo is "Message an agent from the browser". F2-08 (line 317) says only "CHAT criteria for direct messages, asks and hand-offs". Line 85 says "channels in Wave A; bridges deferred". Line 439 names channels as CHAT-19 to CHAT-23 only.
- In `chat-ux.md`, more criteria are about channels: CHAT-2 ("#general, one channel per project and one per team exist automatically"), CHAT-10 ("Announcements are limited per agent per channel"), CHAT-11 ("mute an agent in a channel"), CHAT-16 (channel and group DM visibility), and the channel halves of CHAT-1 and CHAT-8. CHAT-17 is the bridge criterion, but the plan never names it. CHAT-9 includes `/request` (requests are Wave B). CHAT-13 (search) is placed nowhere.
- Fix: add explicit CHAT IDs to the M3 exit. Suggested split:
  - M3: CHAT-3, 4, 5, 6, 7, 12, 14, 15 and 18, plus the DM and thread halves of CHAT-1 and CHAT-8.
  - Wave A (channels): CHAT-2, 10, 11 and 16, plus CHAT-19..23.
  - Deferred: CHAT-17.
  - CHAT-9: /task, /status, /wake and /remind in M3; /bug once bugs are wired; /request in Wave B.
  - CHAT-13: state where it goes.

**A-H2. M4's exit and demo depend on M3's running agent, but the plan runs M3 and M4 in parallel.**
- Evidence:
  - The M4 exit (line 180) includes "Dogfood sections 3 and 4", and the demo is "pause its agent at HQ… watch the stop arrive".
  - Dogfood 3.4 needs "honest runtime readiness" (SRV-14, M3). Row 3.5 needs a real agent to "run a bounded non-repository task". Row 3.6 needs session inspection and pause/resume (PROF-2, F1-10, M3). Row 4.1 needs "local work is running". Row 4.8 pauses a running agent.
  - The flowchart (lines 186–188) has M2 → M4 with no M3 edge. B3-11 (line 283) depends only on B2-10 and B1-15, not on B3-07, B3-08 or F1-10.
- Fix: make M4's agent rows (3.4–3.6, 4.1, the agent half of 4.8, and the demo's Pause) depend on M3. Either add the M3 → M4 edge for those rows, or split M4: M4a covers enrollment, sync and offline status with no agent (3.1–3.3, 4.2–4.7), and M4b adds agents at the satellite after M3.

**A-H3. M2 promises Hire, but Hire's Suggestion step is the HR lead's draft, which can't run until M3.**
- Evidence:
  - The M2 exit (line 178) has "STAFF", and F2-07 (line 316) maps STAFF-1..4 to M2. O-7 (line 161) covers only Add agent record.
  - `our-org-in-the-app.md` §7: "Suggestion (the HR lead's proposal): role, title…". §3 decision 9: "The HR lead proposes". §7.1: a hire request "is drafted by the HR lead into a hire proposal".
  - STAFF-1 to STAFF-3 test Hire's Review and Done steps, which come after that Suggestion.
- Fix: widen O-7 to cover Hire as well as Add agent record (see section B). Until the HR lead runs, Hire needs a "fill in the details yourself" path. Otherwise, move STAFF-1..3 to M3 and keep only STAFF-4 in M2.

### Medium

**A-M1. DEAL is in no milestone exit.**
- Evidence: the M2 exit (line 178) and the M3 exit (line 179) list no DEAL IDs. DEAL appears only in the work items: F2-03 (DEAL-1, 2, 4, 5, M2) and B1-12 and F1-10 (DEAL-3, M3). The dogfood run depends on it: rows 3.6 and 4.8 cover Pause.
- Fix: add DEAL-1, 2, 4 and 5 to the M2 exit, and DEAL-3 to the M3 exit.

**A-M2. HRL is split three ways without saying so.**
- Evidence:
  - B3-04 (line 276, M2) delivers "the built-in HR lead and owner bootstrap" and HRL-3. The M3 exit (line 179) lists HRL-1 to HRL-5. Line 81 says "HR lead… in M3".
  - The HR lead record exists from M2, yet HRL-1 ("Every organization has one built-in HR lead from its creation… can't be offboarded… chosen as anyone's manager") and HRL-2 ("never appears… in member, lead and manager pickers") limit the M2 org chart and pickers (TEAM, PEOPLE, REPORT).
  - HRL-5 ("its runs and cost show under 'HR lead' in Insights") needs Insights, which is Wave B.
- Fix:
  - HRL-1, plus HRL-2 without its Chat part, in M2 with B3-04.
  - HRL-2's Chat part, HRL-3's wake and HRL-4 in M3.
  - HRL-5: the run mode in M3, the Insights part in Wave B.
  - Fix the line 81 wording.

**A-M3. Home and Activity land in M3 (and Needs you is part of Home), but M2 and M4 criteria need them.**
- Evidence: F2-09 (line 318, M3) delivers "Needs you, Working now and the Activity feed". These M2 criteria depend on it:
  - STAFF-2: "in Activity ('Cameron was hired into App')".
  - APPR-5: "in Activity, and the owner sees it on Home".
  - TRI-5: "A Critical bug appears in the owner's Needs you".
  - FIX-4: "reaches the owner's Needs you only when the owner reported the bug".

  These M4 criteria, which runs alongside M3, depend on it too:
  - OFC-5: "A pending join appears in HQ Needs you".
  - OFC-10: "a needs-you item until every one is handled".
- Fix: either move the Needs you list into M2 (split F2-09), or mark the Activity, Home and Needs you parts of these criteria as completing in M3 and make M4's Needs you items depend on F2-09.

**A-M4. PROC-20..30 (11 IDs) are in no wave.**
- Evidence: the plan places processes by phase: "R1 to R3 in Wave A; R4 and R5 in Wave B" (lines 86 and 439–440). The phase table in `docs/design/processes.md` §8 maps only PROC-2..19 to R2–R5. PROC-1 is covered implicitly by R1's contract (§2.1).
  - PROC-20..26 cover autonomy, consults and Decisions rows.
  - PROC-27..30 cover delegation guidance and delivery limits.
  - PROC-20 also reaches into M2's Hire Review ("only the owner sets (agent Settings, Hire's Review)").
- Fix: name PROC-20..30 in a wave: Wave A next to delegates, or with R3. State what Hire's Review shows for autonomy before then (see section B, state S9).

**A-M5. LIVE-1 in M3 includes kinds whose sources arrive in Wave A.**
- Evidence: LIVE-1 lists "git merges, sessions and decisions". Git merges need `org-git` (Wave A, line 439), and Decisions rows need PROC-26 (Wave A). The spec's own success test (our-org §"The goal", bullet 2) names "merges into `v1`".
- Fix: say the M3 version of LIVE-1 covers messages, status, tasks and bugs, team changes and sessions. Git and Decisions show as unavailable kinds until Wave A, in line with the spec's existing "A source offline" quiet-row pattern (§6 States).

**A-M6. The APPR group is placed whole in M2, but parts of it need M3 or Wave work.**
- Evidence:
  - APPR-1: "The same holds for Can approve processes… 'Approves processes'". Processes are Wave A.
  - APPR-2 and APPR-4: "needs a new role… 'Waiting for you: new role'". Role files are Wave B (`roles-ux.md` decision 4; A75 notes `AgentProfile.role` is free text).
  - APPR-7: the cost limit needs the proposal's "estimated cost per month at API prices" (§7.1).
  - APPR-8: `org-agent hire request|approve|reject`. An agent approving needs agent identity (the session token, B3-06, M3), and `hire request` needs the HR lead to draft it.
- Fix: list the M2 subset in the exit, and place the rest (see section B for the wording).

### Low

**A-L1. F2-02 (M1) claims criteria whose data arrives in M2.**
F2-02's "Done when" (line 311) lists "TEAM, PEOPLE, REPORT, OWNER and MOVE criteria" in M1. But:
- MOVE-1..5 need agents (B3-04, M2), and MOVE-2, 3 and 5 also need projects (B1-10, M2). MOVE-4 needs the assignee picker (tasks, M2).
- TEAM-8 and TEAM-13..15 need projects.

The M1 exit itself is fine. Fix: F2-02 in M1 covers people and teams only, and the rest is verified in M2.

**A-L2. Some M2 work-item criteria reach into later waves without naming an unavailable state.**
- CHK-8: checklists stored "in `docs/.templates/checklists/`… includes the steps of the project's processes" (needs a repository and PROC-12).
- TSK-3: "Waiting for a decision with a doc… When that ADR is accepted" (decisions, Wave B).
- TSK-6: Start now (runs, M3).
- PRJ-1: tabs Features, Debt, Docs and Insights (Wave B).
- PRJ-3: "the Brief's last update" (DOC-6, Wave B).
- WORK-1: pill tabs Debt and Requests (Wave B).
- BUG-1: "a doc's menu".

Fix: for each, list which part shows the SRV-18 unavailable state in M2, and where the rest completes.

**A-L3. Some M2 criteria have parts that need satellites or running agents.**
- TEAM-4: "The office filter (each office, and Unplaced)". Unplaced only exists after M4 (OFC-10).
- PEOPLE-6 depends on the office filter.
- OFC-9 is in F1-07 (line 299, M2), but its first sentence is about an unreachable satellite (M4).
- PEOPLE-7 ("the agent's activity says so") needs running agents and Activity (M3).

Fix: name these halves as finishing in M4 and M3.

**A-L4. STAFF-3 (M2) needs harness detection from M3.**
STAFF-3 says "Choosing a runtime that isn't installed or signed in warns in Review". Detection is SRV-14 (M3 exit) and B3-05's probes (M3). F1-09 (M2) lists "runtime readiness reasons" but cites SRV-14 too. Fix: put the probe that STAFF-3 uses into M2, or move STAFF-3 to M3. See state S6 in section B.

**A-L5. EMP-13 is split without saying how.**
F2-10 (line 319, M3) claims "the v1.0 part of EMP-13", while line 439 puts "EMP-4 to EMP-14" in Wave A. The spec's Supervision section has Health, Current goal, Internal work, Memory and Controls (`employee-supervision-ux.md`, lines 94–108). Fix: "EMP-13 Health, Current goal and Controls in M3; Internal work and Memory in Wave A".

**A-L6. RUN loose ends.**
- Line 83 puts RUN-15 in Wave A, but the Wave A row (line 439) doesn't list it.
- RUN-12 ("One-off agents can be started from a job (review, task)") is in M3, but review one-offs need the reviewer setting (processes R3, Wave A).
- Both F2-03 (M2) and F1-10 (M3) claim "the RUN criteria that have a screen", so two developers think they own the same screens.
- RUN-1 ("chosen at hire") lands in M3, but Hire ships in M2 (see S8 in section B).

Fix: list RUN IDs per task.

**A-L7. Plan line 435 says the waves aren't touched by the dogfood run, but dogfood §6 is.**
Dogfood §6 says: "Exercise the retained roster, team, hiring, task/message and process features against their real advertised HTTP capabilities. Report unsupported features as gaps." Fix: keep the scope, but list the v1.0 gaps the dogfood report will name (processes, channels, git activity, repository work), so X-07's evidence log has them up front.

**A-L8. Spec sources are out of step (for X-07, not a plan error).**
- Eden's working copies are older than `docs/specs` for 9 specs. For example, our-org is rev 18 there and rev 19 in the repo; dogfood-run is rev 3 there and rev 5 in the repo.
- Eden's copy of `features-ux.md` is newer: it is rev 3 with FTR-1..14, while the repo has rev 2 with FTR-1..9.
- `models-ux.md` (MOD-1..8) exists only in Eden's folder.
- our-org §7 says "v1 §3.6 holds", but `desktop-app-v1.md` §3.6 (the base Hire flow, with its states) isn't in the repository (our-org line 24 says so). F2-07's developer can't read it.

The plan correctly maps `docs/specs`. Fix: before F2-07 starts, publish the parts of v1 §3.6 that Hire relies on into `our-org-in-the-app.md`. Land FTR rev 3 and models-ux before Wave B.

## B. O-7

**Verdict: M2 can ship the default, but not with the spec as written.** Ten criterion and flow texts name the HR lead as the one who drafts, wakes or provisions, and these are written into the M2 criteria. Nine screen states the interim flow needs aren't covered. The approval rule itself is unaffected: nothing is provisioned before an authorized approval, and direct API calls can't bypass it. So SRV-30 passes in M2.

The default also has to cover **Hire**, not only Add agent record (see A-H3).

**Recommendation.** Write the interim as a permanent state: "when the HR lead can't run". The spec needs that state anyway. In v1.0 the HR lead can still fail to start (runtime missing or signed out), and the spec has no state for that. The old v1 §3.6 had this pattern: "Claude Code missing…: the field is disabled with the reason, and 'Pick a role from the catalog' still works". Writing it once covers M2 and v1.0.

I'd also suggest that Add agent record **always** builds its proposal from the submitted fields. Its fields are already complete, so the HR lead has nothing to draft. That removes the dependency for Add agent record for good. The owner and Eden decide.

### Spec text that needs to change

| # | Where | Current wording | Minimal change |
| --- | --- | --- | --- |
| 1 | STAFF-4 | "submits name, messaging name, runtime, role/title, team, reports-to and office through the HR lead proposal" | "…as a hire proposal (drafted by the HR lead when it can run; otherwise built from the submitted fields)" |
| 2 | HRL-3 | "Hire, Add agent record… and Staff a team wake the HR lead (on call by default), and nothing is provisioned before the proposal is approved…" | "…wake the HR lead when it can run (until then, or when it can't start, the proposal is built from the submitted fields). Nothing is provisioned before…" The approval sentence stays. Also remove "Staff a team" or point to its flow: no spec in docs/specs defines it |
| 3 | §7 Add agent record | "submits the same HR lead proposal and approval workflow as Hire" | "submits the same proposal and approval workflow as Hire" |
| 4 | §7 Suggestion | "Suggestion (the HR lead's proposal): role, title…" | Add: "When the HR lead can't run, Suggestion is a form the requester fills in, with every field marked Your choice needed (state S1)" |
| 5 | §7.1 The route | "is drafted by the HR lead into a hire proposal" | "…is drafted by the HR lead (or, when it can't run, built from the request's fields) into a hire proposal" |
| 6 | §7.1, APPR-4 | "Approved means the HR lead provisions at once" / "the HR lead provisions at once, with no human step" | "the office provisions at once (by the HR lead's run when it can run)" |
| 7 | §2a | "asleep until a Hire, an Add agent or a provisioning request wakes it"; "does the provisioning cleanup when the owner offboards an agent" | Add: "When it can't run, the office provisions and cleans up directly, and nothing waits on the HR lead". This also covers Offboard (DEAL-4, M2) |
| 8 | §3 decision 9 | "The HR lead proposes, and the approver… sees exactly what will be created" | "The HR lead proposes (or the requester, when it can't run)…" |
| 9 | APPR-8 | "`org-agent hire request\|approve\|reject` work with the same words as the app" | "…`hire request` needs the HR lead; when it can't run, it refuses with 'The HR lead can't draft requests yet. Use Add agent record.' (state S7)". Agent approve/reject needs agent identity: place it in M3 |
| 10 | APPR-2, APPR-4 | "A proposal that needs a new role goes straight to the owner" | "…Until role files exist (`roles-ux.md`), no proposal is classed as needing a new role." Or mark the new-role half as arriving with roles |

### States the interim flow needs that the spec doesn't cover

| # | Where | Missing state | Suggested copy or behaviour |
| --- | --- | --- | --- |
| S1 | Hire › Describe and Suggestion | The HR lead can't run (not yet supported, runtime missing, signed out). Today the screen would sit on "HR is reading…" forever | Describe field disabled with the reason: "The HR lead can't run on this server yet." Primary action: **Fill in the details yourself**, which opens Suggestion as an empty form with every field marked Your choice needed. No "HR's reason" line |
| S2 | HR lead chip, quick view and Built-in group (HRL-2) | Spec text: "HR lead · built-in · Asleep · wakes when called". In M2 that is untrue | "HR lead · built-in · Not running · hires are filled in by hand", or the run-modes failure wording once runs exist ("can't start: <reason>") |
| S3 | Hire requests tab, empty state | "When someone asks the HR lead for a new agent, it shows here." This misleads when it can't run | "No hire requests. Hires and agent records waiting for approval show here." |
| S4 | Proposal routed to an agent approver who can't act | In M2 no agent can run or sign in as itself. Later, the approver may be Paused, offboarded or at an offline office. The spec only has "waiting on an agent for over a day is shown to the owner" (APPR-6) | Show it to the owner at once with the reason: "Waiting for ada · ada can't run yet, so you can approve it." This covers Paused and offline approvers too. An offboarded approver re-routes up the line |
| S5 | Approved but not provisioned | The states run Waiting → Approved by ada → Hired. There's no state for provisioning in progress, or for provisioning that fails after an asynchronous approval with nobody watching | "Hiring…" and "Approved, not hired: <step that failed> · **Try again**", plus a Needs you item for the owner. The old v1 §3.6 failure rules apply: name what was already created, and never show a partial hire |
| S6 | Hire Review runtime warning (STAFF-3) | Before harness probes exist (M3), the office can't tell whether a runtime is installed or signed in | "Can't check Codex on <instance> yet. Cameron will be hired; check runtime setup there before it starts." **Hire anyway** stays |
| S7 | `@hr` and `org-agent hire request` | What a requester sees when the HR lead can't run | CLI: the refusal in row 9 above. Chat: the composer says "HR lead can't run yet. Use Hire or Add agent record." Chat is M3, so this matters only if the HR lead's run fails |
| S8 | Hire › Done, and run mode at hire | Done's primary action is **View session**, and Review sets the run mode (`agent-run-modes-ux.md` "At hire (Review step)"). Neither exists before M3 | Done: primary **View profile** until sessions exist. Review: omit How it runs and give new agents the default mode when runs land (say which: On call, per RUN-1 and U19) |
| S9 | Hire › Review rows for later features | Autonomy (PROC-20, Wave A), "the role file if it's new" (Wave B) and "estimated cost per month" (§7.1). Nothing says what Review shows before they exist | Omit the rows until their features land. Autonomy defaults to Asks first, which is the spec's own default. Show no cost line rather than a made-up one |

## Checks run

1. **Criterion inventory.** Extracted every `XXX-n ·` ID from the 16 spec files under `docs/specs` at `4e15a89`. The result is 48 groups and 443 IDs. 17 are retired (IMP-1..11, MENU-1..6, confirmed as retired in the spec text). Rows not starting with "- ID ·" were checked by hand. REQ-n and FEAT-n are example item numbers, and REC-1 is a decision, not a criterion. These match the plan's line 48 counts.
2. **PR #5 delta.** Compared the IDs at `7a88278` (385) with those at `4e15a89` (443): 58 added, matching line 48.
3. **Ranges.** Compared all 48 group counts with the plan's "Specs and designs" table (lines 77–93). All match, including the 52 IDs across 9 decisions groups and the 20 IDs across 4 costs groups.
4. **Placement.** Traced each group to a milestone exit, a B/F row, a wave or a deferral. Read every criterion line in our-org, offices-and-teams, work-items, agent-run-modes, employee-supervision, chat and SRV. Checked processes PROC-1/20..30, ROLE-10/11 and the PROC-to-R-phase map in `docs/design/processes.md` §8.
5. **Dogfood.** Mapped each row of dogfood-run.md §§1–5 (33 rows) and the §6 checks to tasks and milestones. All are scheduled by M5. Ordering problem: A-H2.
6. **O-7.** Grepped all specs for HR lead, Add agent, Staff a team, hire, STAFF-, APPR- and HRL-. Read our-org §§2a, 3, 5–9 and 11 in full, the related parts of agent-run-modes, chat and roles, SRV-30, and Eden's `desktop-app-v1.md` §3.6 for the base Hire flow.
7. **Source drift.** Compared the ID sets and revision numbers of Eden's working copies with `docs/specs` (A-L8).
8. **Removal inventory.** Read §3 (production mock audit), §5 and §6 for which flows must be real and which may be unavailable.

Not checked: engineering effort, task sizes, the architecture, and STATUS.md beyond the M0 rows.
