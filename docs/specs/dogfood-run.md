---
kind: spec
title: "Dogfood run: fresh HQ, satellite and named instances"
status: for-review
revision: 7
owner: ac-product-designer-eden
created: "2026-11-08"
updated: "2026-11-10"
requested_by: owner
reviewers: [ac-staff-engineer-ada, owner]
builds_on: [server-installation-and-access-ux, our-org-in-the-app, offices-and-teams-ux, work-items-ux, agent-run-modes-ux]
---

# Spec · Dogfood run: fresh HQ, satellite and named instances

**Revision 7 (2026-11-10):** the run tests the whole success test of
[our-org-in-the-app.md](our-org-in-the-app.md) again. New **§3a Manage the team
(HQ)** (rows 3a.1 to 3a.16: hire and approve, an agent approving and rejecting,
Add agent record refused before approval, assign, move, pause and resume, offboard)
and **§3b See what's happening, live (HQ)** (rows 3b.1 to 3b.7: Home, Activity,
Jump to latest, and the paused line while HQ restarts). Every row is now marked
Required or Optional (a new Type column; existing rows are all Required). New
**§7 Required rows and the result**, with the N/A rule, and with the live-update
allowance, bug severities and results file restored from revision 3. The pass
rule moved from §6 to §7, which adds what makes a Required row complete. No
existing row's action, pass condition or requirements changed, and no criterion
was added. §0 records each row's result as §7 defines it. §6's retained-features check no
longer includes processes (Wave A, after v1.0).

**Revision 6 (2026-11-10):** rows 1.1, 1.2, 1.5, 1.6, 1.7 and 2.7 also cite the
screen criteria that now define them: SIGN and RCV in
[server-installation-and-access-ux.md §4a](server-installation-and-access-ux.md#4a-sign-in-first-access-and-recovery-code-rev-4)
and OFC-14 to OFC-25 in [offices-and-teams-ux.md §5a](offices-and-teams-ux.md#5a-create-organization-instance-admin-only-rev-14).
No action or pass condition changed.

**Revision 4 (2026-11-09):** replaces the old desktop/team-import walkthrough.
The product has never been deployed. Start from fresh disposable instances, with
no old HR import or desktop migration. Fresh instances skip live Phase A cutover;
its preservation contract is tested separately with disposable fixtures under §6. Run the real HTTP application and
server; explicitly injected fakes support lower-level tests but do not satisfy
this acceptance run. Commands in the linked design are proposed until implemented.

## 0. Preconditions and evidence

Use the same verified release on two supported machines, one macOS and one Linux.
Run once with macOS HQ/Linux satellite and once with Linux HQ/macOS satellite.
Use new roots and credentials; never tests' access to a user's actual office home.
Record versions, OS/architecture, instance IDs, service configuration and supported
runtime. Choose at least one implemented harness for a separately authorized real
agent smoke test. Keep provider credentials out of test output.

Before running, the release must implement browser authentication, HQ APIs, office
enrollment/sync, instance lifecycle and production key custody on both platforms.
The inspected branch does not yet meet these prerequisites. Missing capabilities
are **blocked**, not passed through mocks or marked ready because assets loaded.
See [SRV criteria](server-installation-and-access-ux.md#6-acceptance-criteria).

For every row record its result (§7), timestamp, exact release and evidence.
Retain failure diagnostics with credentials redacted. A docs review is not evidence
that any of these checks ran.

## 1. Install and access HQ

| Step | Action | Pass condition | Requirements | Type |
| --- | --- | --- | --- | --- |
| 1.1 | Install with no instance name, configure HQ and owner access | One `default` instance, platform-private data, valid release payload, ready service; no Tauri or frontend build tools | SRV-1, SRV-2, SRV-7, SIGN-21, SIGN-22, RCV-13 | Required |
| 1.2 | Open its URL from a browser on the other machine | Authenticate and see the real organization without installing/enrolling the browser machine | SRV-8, SRV-18, SIGN-23 | Required |
| 1.3 | Repeat install/setup without a name | Same instance/office identity, root, endpoint and release pin; no duplicate process, enrollment or configuration reset | SRV-2, SRV-6 | Required |
| 1.4 | Stop the instance, repeat setup, then repeat while running | Start once when stopped; while running return status/URL without restarting it | SRV-6, SRV-15 | Required |
| 1.5 | Attempt unauthenticated and cross-origin mutation/stream access | Refuse without disclosing protected data; browser auth does not grant a local agent owner rights | SRV-7, SIGN-1, SIGN-4 | Required |
| 1.6 | Create a second HQ organization through the same listener as an instance admin; repeat as an organization owner without that right | One gateway listener routes both HQs; owner-only creation refused; host keys never reach gateway/CLI | SRV-24, OFC-14, OFC-19, OFC-22 | Required |
| 1.7 | Provision recovery material, interrupt protected delivery before acknowledgement, resume, and retain the encrypted package/code off HQ | Same CA/code on pending retry; no secret in status/logs; acknowledged delivery is not later redisplayed. Wrong-code/damaged-escrow fixture fails closed and correct code recovers the same CA identity | SRV-25, RCV-1, RCV-6, RCV-8, RCV-10 | Required |

## 2. Named instances and setup failures

| Step | Action | Pass condition | Requirements | Type |
| --- | --- | --- | --- | --- |
| 2.1 | Set up `lab` on the HQ machine, with an available endpoint | Different instance ID, root, credentials, sockets and service name; both instances run | SRV-3 | Required |
| 2.2 | Repeat `lab` setup concurrently; try another name pointing to the same data root/alias | One persisted identity and service; duplicate storage writer refused before database access | SRV-4, SRV-6 | Required |
| 2.3 | Configure an occupied port | Actionable conflict; no silent endpoint change or corruption of another instance | SRV-4 | Required |
| 2.4 | Interrupt setup before/after office provisioning markers, then repeat | Resume recorded operation; no duplicate office, lost key or destructive discard of committed data | SRV-6 | Required |
| 2.5 | Run noninteractive setup with complete inputs, then missing input and unavailable secret provider | Same result as wizard when valid; predictable failures without prompts, secrets or dev-key fallback | SRV-5, SRV-26 | Required |
| 2.6 | Use default/lab and a hostile dev server on the same host; copy/overwrite cookies and try read, mutation, stream, refresh/proof recovery and logout | No cross-origin/instance authority without the matching proof; proof never enters URL/log/redirect; valid paired sessions still work | SRV-23 | Required |
| 2.7 | Open eight HQ tabs under HTTP/1.1, issue commands, close the leader and change login session; repeat without worker/lock coordination | At most one shared stream per origin, command capacity retained, cursor-safe leadership change; fallback uses bounded short polling | SRV-27, SIGN-14, SIGN-20 | Required |

## 3. Enroll and execute at a satellite

| Step | Action | Pass condition | Requirements | Type |
| --- | --- | --- | --- | --- |
| 3.1 | Create invitation at HQ; run satellite setup on the other OS using protected input | CLI identifies destination/execution consent and reports awaiting approval; no public satellite app port | SRV-5, SRV-9 | Required |
| 3.2 | Restart pending setup; approve its code in HQ's browser | Same request/key and one office; verified certificate, completed snapshot and ready status | SRV-6, SRV-9 | Required |
| 3.3 | Repeat satellite setup/start | Reuse enrolled identity and stream positions; no second invitation or office | SRV-6, SRV-10 | Required |
| 3.4 | Provision an employee through the real domain/hiring flow and place it at that office | Stable member ID, server-managed home/configuration, scoped `org-agent`, honest runtime readiness | SRV-13, SRV-14 | Required |
| 3.5 | Run a bounded non-repository task and send an agent message locally | Owning office durably commits work, supervisor accounts for it, HQ browser receives real results | SRV-10, SRV-21 | Required |
| 3.6 | Inspect the session through HQ and pause/resume through supported authorized controls | Routed to the correct office/employee; no second main session or uncontrolled descendants | SRV-11, SRV-17 | Required |

Use a non-repository task for 3.5 until a supported cross-office repository transport
exists. V1's current local-bare-Git design does not make an HQ directory reachable
from another host. This run cannot be cited as validation of cross-office coding;
add a real checkout/commit/transfer run once that separate capability is supported.

## 3a. Manage the team (HQ)

Run after §3, with the employee from 3.4 still placed and able to work. The owner
does every browser step signed in at HQ. Keep a **second browser** on the other
machine signed in as the same owner, with the org chart in one tab and Activity in
another, and never reload it: it shows whether each change arrives live (3a.14).

Use throwaway members and names that say so: teams **Dogfood App** and **Dogfood
Office**, projects **Dogfood Desktop** and **Dogfood Server**, and agents **Dogfood
Lead 1**, **Dogfood QA 1**, **Dogfood QA 2** and **Dogfood QA 3**. Place every
throwaway agent at HQ's office, on the harness chosen in §0. Note each agent's
messaging name when it's hired. Never offboard, pause or move the 3.4 employee here.
Hire every throwaway agent with the focus "Dogfood run: act only when the runner
tells you. Don't approve, reject, start or finish anything on your own." (in
Review, or in the request text for hires by command), and before 3a.5 message
Dogfood Lead 1 the same. Rows 3a.5 to 3a.7 and 3b.4 rely on it. Both no-op tasks (3a.10, 3a.12) also say in their description "Dogfood run: leave
this in To do. Don't start it until told.", so an Always on agent doesn't start or
finish them before 3a.13 and 3b.4 need them in To do. Hire every throwaway agent with **How it runs: Always on**, so between turns it
shows **Idle** (an On call agent shows Asleep instead). Roles are free text in
v1.0 (role files come later, `roles-ux.md`), so no proposal here is classed as
needing a new role, and APPR-2's new-role routing isn't exercised.

| Step | Action | Pass condition | Requirements | Type |
| --- | --- | --- | --- | --- |
| 3a.1 | Create the teams Dogfood App and Dogfood Office, with no lead. From each team, create its project: Dogfood Desktop from Dogfood App, Dogfood Server from Dogfood Office | Both teams show in the org chart; each card lists its one project | TEAM-5, TEAM-8 | Required |
| 3a.2 | From Dogfood App, open **Hire**: "A QA lead for the app". Name it Dogfood Lead 1 and place it at HQ's office. Stop at Review | The HR lead wakes (its chip no longer says Asleep) and proposes. Review shows team, reports to, runtime, model, office, home folder (read-only) and messaging name. Nothing exists yet: no card, no roster row, and Agents › Requests shows the proposal as **Waiting for you** | STAFF-1, HRL-3, APPR-3 | Required |
| 3a.3 | Approve the hire. Then make Dogfood Lead 1 the lead of Dogfood App | Hiring progress in words, then "Dogfood Lead 1 is hired and in Dogfood App." The card appears in Dogfood App, highlighted; Activity has one row, "Dogfood Lead 1 was hired into Dogfood App"; the profile records who requested and approved it. The card then shows Lead, and the team card names it | STAFF-2, APPR-5, MOVE-5 | Required |
| 3a.4 | Hire Dogfood QA 1 into Dogfood App the same way, and approve it | Review suggests Dogfood Lead 1 as its manager (the team's lead). After approval it's in Dogfood App, under Dogfood Lead 1 in Reporting lines | STAFF-1, STAFF-2 | Required |
| 3a.5 | In Dogfood Lead 1's Settings, turn on **Can approve hires**. Then have Dogfood QA 1 run `org-agent hire request "A QA engineer named Dogfood QA 2 for Dogfood App, always on, at HQ's office, with the dogfood-run focus from §3a"` | The roster and quick view show "Approves hires" on Dogfood Lead 1. The request becomes a proposal in Requests with requester Dogfood QA 1 and state **Waiting for Dogfood Lead 1**; Dogfood Lead 1 gets it in its inbox; nothing is hired yet | APPR-1, APPR-2, HRL-4, APPR-8 | Required |
| 3a.6 | Have Dogfood Lead 1 run `org-agent hire approve <id>` with no reason, then again with `--reason "…"` | Without a reason it's refused and nothing changes. With one, Dogfood QA 2 is hired with no human step, Always on, at HQ's office (its profile shows both): its card appears in Dogfood App, highlighted; its profile and Activity say it was requested by Dogfood QA 1 and approved by Dogfood Lead 1; Home shows "Dogfood Lead 1 approved hiring Dogfood QA 2…" with **Open** | APPR-4, APPR-5, APPR-8, STAFF-2 | Required |
| 3a.7 | Have Dogfood QA 2 request another hire ("Dogfood QA 4"). Have Dogfood Lead 1 run `org-agent hire reject <id>` with no reason, then with `--reason "…"` | Without a reason it's refused. With one, Requests shows **Rejected**; no card, roster row or messaging name exists for Dogfood QA 4 | APPR-8, HRL-4 | Required |
| 3a.8 | Open **Add agent record** for Dogfood QA 3 (messaging name, runtime, role and title, team Dogfood App, reports to Dogfood Lead 1, office HQ) and submit. Before approving, call `member.add_agent` directly through HQ's API for Dogfood QA 3 with the owner's session. Then **Reject** the proposal with a reason | Submitting creates only a proposal, **Waiting for you**. The direct call is refused (the error it returns is A96 in `our-org-in-the-app.md` §13), and creates no member, home or messaging binding. After Reject, Requests shows **Rejected**, and Dogfood QA 3 appears nowhere else: not in the chart, the roster or the HQ office's agents | STAFF-4, HRL-3, SRV-30, APPR-6 | Required |
| 3a.9 | On Dogfood QA 2, **Add to team…** › Dogfood Office. Then **Remove from Dogfood Office** (no confirmation expected), and start **Remove from Dogfood App** (its last team) but cancel it. On Dogfood Lead 1, open **Move to team…** › Dogfood Office and cancel; then **Add to project…** › Dogfood Server | Dogfood QA 2 shows in both teams, each with the "+N teams" badge naming the other, then in Dogfood App only. Removing its last team asks first and says it would be in Not in a team; Cancel changes nothing. Dogfood Lead 1's Move dialog says "Dogfood Lead 1 leads Dogfood App. Dogfood App will have no lead.", and Cancel changes nothing. Add to project lists Dogfood Server's one team and adds Dogfood Lead 1 to Dogfood Office in one step | TEAM-2, DEAL-2, MOVE-2, MOVE-3 | Required |
| 3a.10 | On the Dogfood Desktop board, create the task "Dogfood QA: no-op" and assign it to Dogfood QA 1 | The assignee picker shows each agent's status and runtime. The task shows on Dogfood QA 1's Tasks tab | MOVE-4 | Required |
| 3a.11 | **Move to team…** › Dogfood Office on Dogfood QA 1. Press **Undo** on the toast, then move it again | The dialog says "Moves Dogfood QA 1 from Dogfood App to Dogfood Office. Dogfood QA 1 stops working on Dogfood App's projects (Dogfood Desktop) and starts on Dogfood Office's (Dogfood Server)." The card moves in one step, never through Not in a team; the toast reads "Dogfood QA 1 moved to Dogfood Office. **Undo**", and Undo moves it back the same way. | MOVE-1, MOVE-2 | Required |
| 3a.12 | Check that Dogfood QA 2 is Idle, then **Pause** it. While it's paused, have Dogfood QA 1 send it a message, and assign it a new task "Dogfood QA: no-op 2". Then **Resume** it | The dialog says HQ will record Pause and new work waits once the office applies it. The card shows **Paused · stop pending at office** (at HQ's own office this may be too brief to see; 4.8 covers it), then **Paused** once HQ's office confirms. Paused, it isn't woken and the message waits; assigning asks "Dogfood QA 2 is paused. Assign anyway? They'll see it when resumed." Resume is one click, and the message then reaches it | DEAL-3, MOVE-4 | Required |
| 3a.13 | In Reporting lines, note Dogfood Lead 1's manager and who reports to it. **Offboard** Dogfood Lead 1, then Dogfood QA 1. Then try **Add person** with Dogfood QA 1's messaging name | Each dialog lists only what applies: Dogfood Lead 1's names each agent that reports to it and says they will report to its manager, or to the owner if it had none; Dogfood QA 1's says its open task comes back unassigned. Afterwards both are under Settings › Team › Former members with the date, read-only; the task is unassigned; the reporting lines changed as stated. Add person refuses the name inline | DEAL-4, PEOPLE-4 | Required |
| 3a.14 | Throughout 3a.1 to 3a.13, watch the second browser | Every change made in 3a.1 to 3a.13 (hires, lead, teams, task, move, Undo, pause, resume, offboard) shows in its org chart, where the chart shows it (cards, teams, lead and status; tasks aren't on cards), and in its Activity (Pause and Resume are Status changes, hidden by default: turn on the Status filter to see them; consecutive changes by one member collapse into a group such as "… changed team 3 times · Show", and Show lists one row per change), within the live-update allowance (§7) and without a reload. Proposals and rejections add no card | LIVE-1, LIVE-6, STAFF-2, MOVE-1, Success test 3 (`our-org-in-the-app.md`) | Required |
| 3a.15 | Throughout 3a.1 to 3a.13, watch the 3.4 employee | The 3.4 employee keeps working on its own runtime: nothing stops, restarts or pauses it | Success test 3 (`our-org-in-the-app.md`; no criterion ID) | Required |
| 3a.16 | Words check across 3a. Open the HR lead's quick view | The UI never says "deallocate"; it offers Remove from team, Pause, Resume, Offboard and Unassign. Status, runtime, Paused and Lead are words with icons. The HR lead has no Offboard, Move or team action | DEAL-1, HRL-1 | Required |

## 3b. See what's happening, live (HQ)

Run after 3a, with Dogfood QA 2 (kept from 3a) and the 3.4 employee. Watch the
second browser from 3a, without reloading, after turning its Activity Status
filter back off (Activity's default). The owner's messaging handle is `owner`.

| Step | Action | Pass condition | Requirements | Type |
| --- | --- | --- | --- | --- |
| 3b.1 | Give the 3.4 employee a bounded non-repository task. While it works, open Home | Home shows **Needs you**, **Working now** with the 3.4 employee and its current task on one line, and **Latest**: the newest 8 Activity rows with **All activity**. Only real members and tasks; no sample data | LIVE-4 | Required |
| 3b.2 | Have Dogfood QA 2 send the 3.4 employee a short message with `org-agent msg send`. Open the thread from Activity. Then have the 3.4 employee send `owner` a message, and open that thread | The message row appears in Activity within the allowance, and **Open thread** shows the whole thread. The first thread is read-only and says so; the thread addressed to the owner offers **Reply**, and a reply sends | LIVE-2, LIVE-3 | Required |
| 3b.3 | Have Dogfood QA 2 start a turn (send it a message), then let it finish | Its card's status word changes to Working and back, and Working now on Home adds and removes it, each within the allowance. Activity shows no Status row by default; the Status filter shows them | LIVE-6, LIVE-4, PROF-3 | Required |
| 3b.4 | Have Dogfood QA 2 move its task "Dogfood QA: no-op 2" with `org-agent task start <id>`. As the owner, move "Dogfood QA: no-op" on the board, and report a bug in Dogfood Desktop | Each shows as a row in Activity (Tasks and bugs), and the board updates, within the allowance | LIVE-1 | Required |
| 3b.5 | In Activity, scroll down, then have Dogfood QA 2 send another message. Press **Jump to latest** | The list doesn't move, and an "n new · Jump to latest" pill appears (1 new when only that message arrived). Jump to latest goes to the top with the new row there | LIVE-5 | Required |
| 3b.6 | Merge a reviewed branch into the integration branch of a repository the HQ office fetches | A Git row in Activity ("merged … into …") once the office has fetched | LIVE-1 | Optional |
| 3b.7 | With Activity open in both browsers, run `agentic-org stop` on the HQ machine. Move to another screen already loaded, and press **Retry**. While HQ is down, have the 3.4 employee send Dogfood QA 2 a message. Then run `agentic-org start` | Both browsers show "Live updates paused. Showing what was loaded at <HH:MM>." with **Retry**, on every screen; Retry reads "Retrying…" and the line stays while HQ is down. Once HQ is back the line clears by itself, after signing in again if the restart ended the session. The message sent while HQ was down then shows in Activity once, with no gap | LIVE-8, SIGN-17, SIGN-13, SRV-10, SRV-12 | Required |

Row 3b.6 is Optional because this run uses non-repository tasks (§3 note) and
needs a repository the HQ office can fetch; a merge into the integration branch
is still part of the success test, so run it when one is set up.

## 4. Disconnect, retries and event recovery

| Step | Action | Pass condition | Requirements | Type |
| --- | --- | --- | --- | --- |
| 4.1 | Drop the satellite connection while local work is running | Local permitted work continues; HQ shows stale/offline status with last seen | SRV-10, SRV-11 | Required |
| 4.2 | Send a cross-office message and request live control while disconnected | Durable message queues according to its contract; control is unavailable or explicitly queued, never falsely completed | SRV-11 | Required |
| 4.3 | Reconnect, including a repeated work frame/ack | Missing records synchronize exactly once logically; ownership/version checks hold | SRV-10 | Required |
| 4.4 | Drop the browser event connection; make changes; reconnect and test an expired cursor | Recover all changes through a cursor or snapshot, with no snapshot/event gap; stale state was visibly marked | SRV-12 | Required |
| 4.5 | Lose the HTTP response after a committed mutation; resend the same operation ID | Return the recorded result, one logical change; altered payload with same ID is refused | SRV-12 | Required |
| 4.6 | Slow the browser/event consumer and exceed queue limits | Bounded resource use; explicit reconnect/resync behavior; agents and durable writes continue | SRV-12, SRV-17 | Required |
| 4.7 | Reject/revoke an office identity; test an incompatible protocol range | No unauthorized reconnect or half-understood apply; actionable status and preserved local data | SRV-9, SRV-10 | Required |
| 4.8 | Disconnect satellite, Pause at HQ, restart/reconnect it; repeat with a later Resume before reconnect | HQ records paused status with stop pending; current version reconciles before work, office acknowledgement resolves stop, stale pause never overrides later Resume | SRV-28 | Required |
| 4.9 | Change a goal while the office is known offline; then lose a forwarded reply and retry/lookup with the same ID | First is unavailable with no saved/queued goal; uncertain forwarded outcome stays pending, resolves once, and preserves expected-version conflicts | EMP-3, SRV-28 | Required |

## 5. Lifecycle and removal

| Step | Action | Pass condition | Requirements | Type |
| --- | --- | --- | --- | --- |
| 5.1 | Close all browser windows | Services and supervised agent work continue | SRV-8 | Required |
| 5.2 | Stop one instance during work, then restart | No new admission; acknowledged writes durable; owned children stopped/reaped; peer instance unaffected | SRV-15, SRV-17 | Required |
| 5.3 | Lose a required credential and try startup against existing data | Specific fail-closed result; no fresh/replacement key, DB/identity change or dev-store fallback; restore the legitimate test credential and recover | SRV-15, SRV-26 | Required |
| 5.4 | Upgrade only `lab` to a matching test release and inject a failed readiness check | Default remains on its pin; complete helper/assets set; documented recoverable state, no unsafe binary-only schema rollback | SRV-16 | Required |
| 5.5 | Uninstall `lab` | Only its service removed; data retained; default and referenced release payloads remain usable | SRV-16 | Required |
| 5.6 | Explicitly delete the disposable `lab` data | Exact selected stopped instance removed; default data/credentials untouched | SRV-16 | Required |

## 6. Source and feature acceptance

Verify SRV-29/30 with forged home/clear_home inputs and direct pre-approval agent
creation: no external path or unapproved member/home/binding is provisioned. Confirm
the profile displays the launcher's actual managed path. Verify SRV-32 with mixed
local protocol versions and setup build/staging: unsupported versions fail before
dispatch and the staged agentic-org is the org-setup CLI, never the retired shell.

Use disposable Phase A fixtures for SRV-31: unresolved asks/replies, expiry, read/
defer/done state, interrupted import and repeated import. Preserve IDs and state,
require the selected instance/office context, and prove managed agents reject
spool/context fallback. Do not import, mutate or delete the team's actual live
spool in this acceptance run. REC-1 remains separately gated recovery work; row
1.7 verifies escrow material, not completed promotion/fencing.

Verify SRV-22: retained browser code/package uses `apps/org-ui` / `org-ui`, and
server code/Cargo package/worker executable uses `apps/org-server` / `org-server`.
Check workspace, imports, test helpers, release/signing resolution and ownership
references against the [project structure](../design/project-structure.md). Verify
domain/feature moves preserve private SQL, transactions, public boundaries and
licensed UI containment before treating the reorganization as complete.

Verify no Tauri build/runtime dependency, native capture/menu, old-team import or
production mock fallback remains. Exercise the retained roster, team, hiring,
task/message features against their real advertised HTTP capabilities (processes
are Wave A, after v1.0, so they're not part of this check).
Report unsupported features as gaps; do not substitute test fixtures for product
behavior. Native/global capture and old import criteria are retired by the updated
specs. Run repository formatting, relevant Rust tests/Clippy and frontend checks,
tests and build on the implementation, with the deletion ledger reviewed.

## 7. Required rows and the result

**Type.** Every row is **Required** or **Optional**. Required rows decide the
result. An Optional row is still run and recorded; its failure becomes a bug but
doesn't fail the run. §6's checks count as Required.

**Results.** Each row gets one result per platform assignment: **Pass**, **Pass
with notes**, **Fail**, **Blocked** or **N/A**.

- **Blocked:** a capability the row needs isn't implemented (§0). Name the
  missing capability. A row that can't run because an earlier row failed is
  Blocked too, naming that row. A failure doesn't stop the run otherwise:
  carry on with the next row.
- **N/A:** only when this platform assignment can't run the row, with the reason
  recorded beside the result. A missing or unfinished feature is Blocked, never
  N/A. A Required row that is N/A on one assignment must pass on the other; N/A
  on both counts as a Fail.

**Live-update allowance.** The live timings in §3a and §3b are 2 seconds, without
a reload (`our-org-in-the-app.md` decision 8, LIVE-2, LIVE-6). Time each change
from when HQ has recorded it to when it shows in the second browser, so a slow
but correct operation, such as a hire, isn't counted as a slow live update. HQ
has recorded it when it shows in the browser that made it (a change made in the
browser), when the `org-agent` command returns (a change an agent makes, such as
a message, a hire request or approval, or a task move), or when the agent's turn
starts or ends in its own session view (a status change). A change HQ or an
office makes as a result of another (the HR lead provisioning an approved hire,
an office confirming Pause) is timed from when it shows in the browser that's
watching it to when it shows in the second browser. Up to 4 seconds is **Pass with
notes**. Over 4 seconds, or a change that shows only after a reload, is a Fail.
The allowance covers timing only, never a missing, wrong or duplicated change.

**The run passes** only with required rows complete on both platform assignments,
real scoped runtime execution, and no unresolved data-loss/authorization/duplicate
execution findings. Backup recovery, cross-office repository work and optional
runtime coverage require their own evidence; do not infer them from this smoke run.
A Required row is complete when it is Pass or Pass with notes, or N/A under the
rule above. The run is **Pass with notes** when its only exceptions are timings
within the allowance or failed Optional rows.

**Bugs.** Every Fail, and every Pass with notes, becomes a bug in Agentic Org
(`ac`), with the row number and platform assignment in its title (for example
"3a.11 macOS HQ: Move toast has no Undo"):

- **Critical:** a Required row that fails, and any data-loss, authorization or
  duplicate-execution finding, wherever it shows up.
- **Major:** an Optional row that fails, and each timing within the allowance.

**Where results go.** One file per run, `docs/research/dogfood-run-<date>.md` in
this repository, covering both platform assignments: each row's result, time,
release and evidence, the N/A reasons, the Blocked capabilities and the bugs filed,
with credentials redacted. The next run compares against it.
