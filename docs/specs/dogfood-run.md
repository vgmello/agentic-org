---
kind: spec
title: "Dogfood run: fresh HQ, satellite and named instances"
status: current
owner: lead
created: "2026-11-08"
updated: "2026-11-17"
requested_by: owner
builds_on: [server-installation-and-access-ux, our-org-in-the-app, offices-and-teams-ux, work-items-ux, agent-run-modes-ux]
---

# Spec · Dogfood run: fresh HQ, satellite and named instances

The acceptance run of a release: fresh disposable instances, the real HTTP
application and server, an HQ and a satellite on two operating systems. Injected
fakes support lower-level tests but never satisfy this run. Selected-model data
checks are in §6a. Undo, advanced checklists, retrospectives, automatic
scheduling and memory are not in v1 and are not checked.

## Which sections apply when

The run grows with the [implementation plan](../implementation-plan.md)'s milestones.

| Milestone | What is run | Sections |
| --- | --- | --- |
| One: one office on a Mac | One macOS machine, HQ only. Hires are placed at HQ. | §1, §2 (rows that need one machine), §3a, §3b, §6, §6a |
| Two: Linux | The same on Linux, including in a container | The same sections on Linux |
| Three: a second office | The full run below, with a satellite and both operating systems. This is the release acceptance run. | All sections, including §3, §4 and §5 |

In milestones one and two, a row that needs a second machine or a satellite is not
run and is recorded as not yet applicable, never as passed.

## 0. Preconditions and evidence

Use the same verified release on two supported machines, one macOS and one Linux.
Run once with macOS HQ/Linux satellite and once with Linux HQ/macOS satellite.
Use new roots and credentials; never tests' access to a user's actual office home.
Record versions, OS/architecture, instance IDs, service configuration and supported
runtime. Choose at least one implemented harness for a separately authorized real
agent smoke test. Keep provider credentials out of test output.

Before running, the release must implement browser authentication, HQ APIs, office
enrollment and sync, instance lifecycle and the host's credential custody on both
platforms. Missing capabilities are **blocked**, not passed through mocks or marked
ready because assets loaded.
See [SRV criteria](server-installation-and-access-ux.md#6-acceptance-criteria).

For every row record its result (§7), timestamp, exact release and evidence.
Retain failure diagnostics with credentials redacted. A docs review is not evidence
that any of these checks ran.

### Product validation for a delivered UI slice

In addition to the real-service walkthrough, compare each merged UI slice with
its first parent in isolated throwaway worktrees. Record the exact commits and
the cumulative capabilities delivered in each build. Apply a requirement only
when its capability is in that build; a deliberately unavailable feature is
**not applicable** to that comparison, with the reason, rather than a regression.
This does not waive a Required row of the release run (§7).

Use Chromium, Firefox and WebKit, one browser and one Playwright worker at a time.
Check 1280×800, 800×600, 390×844 and 320×640, at normal and 200% text, with both a
new organization and representative populated data. Verify the computed root
font size for the enlarged-text run and that content text actually scales;
resizing the viewport alone does not test text enlargement. Check production
unavailable routes against the production build as well as supported mock flows.
Never use a user's organization or credentials as fixtures.

For each criterion record **met / partly met / not met**, before and after, with
screenshots of every judged screen/state. Keep counts per build and per engine;
do not pool them. Report a finding's severity, type, whether it predates the
merge, reproduction steps, expected behavior and requirement, actual behavior,
and evidence paths. Recompute summary counts from the raw results. A clean
screenshot or a passing criterion does not by itself establish product fit.

Walk the owner jobs in [the shell requirements](app-shell-ux.md) end to end as
far as the delivered capabilities allow: identify what needs a decision, respond,
assign or capture work, see who owns it, and inspect its result. Check that names,
navigation, counts and resolution agree across screens, and that operational
setup stays out of everyday work. Try long names, many rows, invalid/duplicate
input, rapid repeat actions, back/forward, reload during a flow and resizing.
Include empty, loading, error, success and disconnected states, keyboard/focus,
contrast, target size, reduced motion and text reflow from the shared shell rules.
Record product-fit and information-architecture failures even when the individual
screen's older acceptance criteria pass. Recheck affected cases after a fix.

## 1. Install and access HQ

`agentic-org setup` is the one initial HQ command. Verify automatic component preparation and service registration, with `agentic-org serve` as the service entry point; there is no separate install step and no satellite choice. Repeated setup resumes the same HQ and service. Include the offline local-bundle and missing-service-manager cases of SRV-33 and SRV-34.

| Step | Action | Pass condition | Requirements | Type |
| --- | --- | --- | --- | --- |
| 1.1 | Run HQ setup without an instance name; open the printed registration link and register the administrator; create an organization in the browser with its slug and remote organization repository/state branch | One default instance, private roots, verified release and ready service. Setup asks for no username, password, organization or team and prints one registration link. The organization asks for no team and makes one top-level team with the creator's person as lead and the built-in HR lead as member; canonical fields round-trip. No local-only organization Git, teamless HR or frontend build tools. Agent and remote connectivity/publication readiness are separate; offline validation cannot mark either externally verified | SRV-1, SRV-2, SRV-7, SRV-35, SRV-36, SIGN-21, SIGN-22, SIGN-23 | Required |
| 1.2 | Open its URL from a browser on the other machine | Authenticate and see the real organization without installing/enrolling the browser machine | SRV-8, SRV-18, SIGN-23 | Required |
| 1.3 | Repeat install/setup without a name | Same instance/office identity, root, endpoint and release pin; no duplicate process, enrollment or configuration reset | SRV-2, SRV-6 | Required |
| 1.4 | Stop the instance, repeat setup, then repeat while running | Start once when stopped; while running return status/URL without restarting it | SRV-6, SRV-15 | Required |
| 1.5 | Attempt unauthenticated and cross-origin mutation/stream access | Refuse without disclosing protected data; browser auth does not grant a local agent owner rights | SRV-7, SIGN-1, SIGN-4 | Required |
| 1.6 | Create a second organization through the same listener as the administrator; repeat signed in as an invited account that owns an organization but isn't the administrator | One web listener routes both organizations; creation by the other account is refused; host keys never reach web/CLI | SRV-24, OFC-14, OFC-19, OFC-22 | Required |
| 1.7 | After creating the organization (1.1), at the HQ machine run `agentic-org recovery release`, interrupt it after the code is shown and before `yes`, run it again without and then with `--redeliver`, confirm, and retain the encrypted package/code off HQ; then run it again, and run `agentic-org recovery status` | Same CA/code on pending retry; without `--redeliver` no code, with it the same code; each run that shows the code asks once to confirm it's you; the browser shows Shown, not confirmed, then "Recovery code shown on <date> (shown again once, on <date>) · collected on <date>."; `recovery status` lists both showings with the account and time, and no code; no secret in status/logs; acknowledged code is not shown again, even with `--redeliver`. Wrong-code/damaged-escrow fixture fails closed and correct code recovers the same CA identity | SRV-25, RCV-1, RCV-6, RCV-8, RCV-10, RCV-18, RCV-19, RCV-23, RCV-24, RCV-25 | Required |

## 2. Named instances and setup failures

| Step | Action | Pass condition | Requirements | Type |
| --- | --- | --- | --- | --- |
| 2.1 | Set up `lab` on the HQ machine, with an available endpoint | Different instance ID, root, credentials, sockets and service name; both instances run | SRV-3 | Required |
| 2.2 | Repeat `lab` setup concurrently; try another name pointing to the same data root/alias | One persisted identity and service; duplicate storage writer refused before database access | SRV-4, SRV-6 | Required |
| 2.3 | Configure an occupied port | Actionable conflict; no silent endpoint change or corruption of another instance | SRV-4 | Required |
| 2.4 | Interrupt setup before/after office provisioning markers, then repeat | Resume recorded operation; no duplicate office, lost key or destructive discard of committed data | SRV-6 | Required |
| 2.5 | Run noninteractive setup with complete inputs, then missing input and unavailable secret provider; later collect the recovery code at the machine | Same result as wizard when valid, except the recovery code, which stays waiting until `agentic-org recovery release` at the machine; predictable failures without prompts, secrets or a development-key fallback | SRV-5, SRV-26, RCV-20 | Required |
| 2.6 | Use default/lab and a hostile dev server on the same host; copy/overwrite cookies and try read, mutation, stream, refresh/proof recovery and logout | No cross-origin/instance authority without the matching proof; proof never enters URL/log/redirect; valid paired sessions still work | SRV-23 | Required |
| 2.7 | Open eight HQ tabs under HTTP/1.1, issue commands, close the leader and change login session; repeat without worker/lock coordination | At most one shared stream per origin, command capacity retained, cursor-safe leadership change; fallback uses bounded short polling | SRV-27, SIGN-14, SIGN-20 | Required |
| 2.8 | Run complete local-bundle HQ setup with --offline while recording attempted external requests; supply a syntactically valid remote organization URL/branch and local credential references, then later authorize online operation | Zero external requests: no Git fetch/auth, provider/model discovery or update checks. HQ may be locally ready; registration remains remote and connectivity/publication/provider readiness stays unverified or pending. Missing local prerequisites fail without fetching. Later authorized online verification/publication updates its own status without changing organization identity; remote-auth failure cannot fabricate success | SRV-34, SRV-35; data 11 AE-17 | Required |

## 3. Enroll and execute at a satellite

Satellite onboarding starts from HQ's Add office UI or an HQ-issued command on the destination. Initial agentic-org setup is HQ-only; it must not offer a join choice. Reuse the destination installation/service internals without running initial HQ setup there. Verify the helpers `agentic-org-office` and `agent-os` and no web child on a satellite-only installation.

| Step | Action | Pass condition | Requirements | Type |
| --- | --- | --- | --- | --- |
| 3.1 | Create invitation at HQ; run the HQ-issued enrollment flow on the other OS using protected input | CLI identifies destination/execution consent and reports awaiting approval; no public satellite app port | SRV-5, SRV-9 | Required |
| 3.2 | Restart pending enrollment; approve its code in HQ's browser | Same request/key and one office; verified certificate, completed snapshot and ready status | SRV-6, SRV-9 | Required |
| 3.3 | Repeat satellite enrollment/start | Reuse enrolled identity and stream positions; no second invitation or office | SRV-6, SRV-10 | Required |
| 3.4 | Provision an agent through approved hiring with one home team, separate execution config/profile/model and local connection binding; place it at the satellite | Stable employee ID/handle, valid home team and generation, managed paths and scoped agent-os. Missing config/auth/model/permit shows specific not-ready state; a ready route shares the canonical connection quota | SRV-13, SRV-14, SRV-36, SRV-37 | Required |
| 3.5 | Explicitly execute one bounded team-scoped non-repository task and send a local message | Required accountable team, project_id null, one T- reference in that team scope; turn/attempt/actual consumer usage retained, incomplete usage labelled. Office durably commits before ack; HQ receives authorized real results | SRV-10, SRV-21, SRV-37 | Required |
| 3.6 | Inspect the session through HQ and pause/resume through supported authorized controls | Routed to the correct office/employee; no second main session or uncontrolled descendants | SRV-11, SRV-17 | Required |

Use a team-scoped non-repository task for 3.5 until a supported cross-office code
transport exists. The data contract allows local-only code repositories, but an
HQ filesystem path is not another host's remote. Organization metadata Git must
already be remote. This run proves no cross-office coding unless real checkout,
commit and transport evidence is added. §6a.3 checks repository associations/cache
semantics separately.

## 3a. Manage the team (HQ)

In the full run, do this after §3, with the employee from 3.4 still placed and able to
work; in a single-office run, do it after §1 with a hire placed at HQ. The owner
does every browser step signed in at HQ. Keep a **second browser** on the other
machine signed in as the same owner, with the org chart in one tab and Activity in
another, and never reload it: it shows whether each change arrives live (3a.14).

Use throwaway members and names that say so: teams **Dogfood App** and **Dogfood
Office**, projects **Dogfood Desktop** and **Dogfood Server**, and agents **Dogfood
Lead 1**, **Dogfood QA 1**, **Dogfood QA 2** and **Dogfood QA 3**, and one person, **Dogfood Person 1**. Place every
throwaway agent at HQ's office, on the harness chosen in §0. Note each agent's
messaging name when it's hired. Never offboard, pause or move the 3.4 employee here.
Hire every throwaway agent with the focus "Dogfood run: act only when the runner
tells you. Don't approve, reject, start or finish anything on your own." (in
Review, or in the request text for hires by command), and before 3a.5 message
Dogfood Lead 1 the same. Rows 3a.5 to 3a.7 and 3b.4 rely on it. Both no-op tasks (3a.10, 3a.12) also say in their description "Dogfood run: leave
this in To do. Don't start it until told." Use explicit bounded turns; no row
requires automatic scheduling. Observe the actual
ready/idle state supported by the execution backend. Identity profiles have one
required home team and optional role/reporting. Role files are structural
data; optional descriptive role_text is separate. Preserve hiring approval checks
without inferring roles or authority from free text.

| Step | Action | Pass condition | Requirements | Type |
| --- | --- | --- | --- | --- |
| 3a.1 | Create Dogfood App and Dogfood Office teams with no lead; create Dogfood Desktop and Dogfood Server projects with the corresponding owner_team_id | Both teams show; each project has one required accountable owner team. No Area is created as an implicit requirement | TEAM-5, data 01/02 | Required |
| 3a.2 | From Dogfood App open Hire for Dogfood Lead 1, select its one home team, optional role/reporting, execution profile/model and HQ placement, then stop at Review | Review separates identity, reporting, execution and placement; proposed managed path/handle are read-only. No employee, home or messaging binding before approval; Requests is Waiting for you | STAFF-1, HRL-3, APPR-3, SRV-36, SRV-37 | Required |
| 3a.3 | Approve the hire. Then make Dogfood Lead 1 the lead of Dogfood App | Hiring progress in words, then "Dogfood Lead 1 is hired and in Dogfood App." The card appears in Dogfood App, highlighted; Activity has one row, "Dogfood Lead 1 was hired into Dogfood App"; the profile records who requested and approved it. The card then shows Lead, and the team card names it | STAFF-2, APPR-5, MOVE-5 | Required |
| 3a.4 | Hire Dogfood QA 1 into Dogfood App the same way, and approve it | Review suggests Dogfood Lead 1 as its manager (the team's lead). After approval it's in Dogfood App, under Dogfood Lead 1 in Reporting lines | STAFF-1, STAFF-2 | Required |
| 3a.5 | Add a person, Dogfood Person 1, to Dogfood Office and make them its lead; give them **Invite to sign in** and register them. Then have Dogfood QA 1 run `agent-os hire request "A QA engineer named Dogfood QA 2 for Dogfood Office, at HQ's office, with the focus: Dogfood run: act only when the runner tells you. Don't approve, reject, start or finish anything on your own."` | Dogfood QA 1 leads no team and has no Allow hiring permission. The request becomes a proposal in Requests with requester Dogfood QA 1 and state **Waiting for Dogfood Person 1** (the person leading the new agent's home team); Dogfood Person 1 sees it in Needs you; nothing is hired yet. No agent in this organization can approve it, because none leads a team with Allow hiring on | APPR-1, APPR-2, HRL-4, APPR-8, SIGN-25 | Required |
| 3a.6 | Signed in as Dogfood Person 1, approve the proposal with no reason, then with a reason | Without a reason it's refused and nothing changes. With one, Dogfood QA 2 is hired with one home team Dogfood Office, HQ placement and the dogfood-run focus; separate execution readiness remains visible: its card appears in Dogfood Office, highlighted; its profile and Activity say it was requested by Dogfood QA 1 and approved by Dogfood Person 1; the owner's Home shows "Dogfood Person 1 approved hiring Dogfood QA 2…" with **Open** | APPR-4, APPR-5, APPR-8, STAFF-2 | Required |
| 3a.7 | Have Dogfood QA 2 request another hire ("Dogfood QA 4") for Dogfood Office. As Dogfood Person 1, reject it with no reason, then with a reason | Without a reason it's refused. With one, Requests shows **Rejected**; no card, roster row or messaging name exists for Dogfood QA 4 | APPR-8, HRL-4 | Required |
| 3a.8 | Start a Hire for Dogfood QA 3 (home team Dogfood App) and stop at Review. Before approving, attempt direct employee provisioning through the employee-creation operation with the owner's session. Then **Reject** the proposal with a reason | The proposal is **Waiting for you**. The direct call is refused (the error it returns is A96 in `our-org-in-the-app.md` §13), and creates no member, home or messaging binding; there is no other way to create an agent. After Reject, Requests shows **Rejected**, and Dogfood QA 3 appears nowhere else: not in the chart, the roster or the HQ office's agents | STAFF-4, HRL-3, SRV-30, APPR-6 | Required |
| 3a.9 | Try adding a second home team and clearing Dogfood QA 2's home team through the UI and the employee operation; cancel moving Dogfood Lead 1; add Dogfood Office as a contributor to Dogfood Desktop | Second simultaneous home membership and teamless result refused; cancelled move changes nothing, including lead appointment. Contributor changes do not move employees or change project owner. Any lead transfer explicitly updates the direct-member appointment | Data 01/02/04 | Required |
| 3a.10 | Create a task on Dogfood Desktop and atomically assign team/individual at expected revision to Dogfood App/Dogfood QA 1 | Required accountable team remains the project owner; assigned pair is separate and reporter immutable. Task has a T- reference minted by the creating office, retained assignment event and the employee Tasks tab entry | MOVE-4, data 03/12 | Required |
| 3a.11 | Move Dogfood QA 1's single home team from Dogfood App to Dogfood Office and inspect task/workspace state | One atomic home-team/path move preserving ID/handle and history; optional reporting is explicit. Existing project accountability/assignment is not silently rewritten and worktree is retained. Show a textual confirmation or toast | Data 04/12; MOVE-1 | Required |
| 3a.12 | Check that Dogfood QA 2 is Idle, then **Pause** it. While it's paused, have Dogfood QA 1 send it a message, and assign it a new task "Dogfood QA: no-op 2" on the Dogfood Desktop board. Then **Resume** it | The dialog says HQ will record Pause and new work waits once the office applies it. The card shows **Paused · stop pending at office** (at HQ's own office this may be too brief to see; 4.8 covers it), then **Paused** once HQ's office confirms. Paused, it isn't woken and the message waits; assigning asks "Dogfood QA 2 is paused. Assign anyway? They'll see it when resumed." Resume is one click, and the message then reaches it | DEAL-3, MOVE-4 | Required |
| 3a.13 | Offboard Dogfood Lead 1 and Dogfood QA 1 using the supported explicit reconciliation; try reusing Dogfood QA 1's handle | Profiles inactive with IDs/handles/history retained, active assignments reconciled and lead removed. Reporters/history do not change. Any reporting reassignment names a valid active employee or null; no implicit teamless owner fallback. Reusing retained handle refused | DEAL-4, PEOPLE-4, data 04/08/12 | Required |
| 3a.14 | Throughout 3a.1 to 3a.13, watch the second browser | Each of these changes from 3a.1 to 3a.13 (hires, lead, teams, task, move, pause, resume, offboard) shows in its org chart, where the chart shows it (cards, teams, lead and status; tasks aren't on cards), and in its Activity (Pause and Resume are Status changes, hidden by default: turn on the Status filter to see them; consecutive changes by one member collapse into a group such as "… changed team 3 times · Show", and Show lists one row per change), within the live-update allowance (§7) and without a reload. Proposals and rejections add no card and aren't counted as Activity rows here. Project creation (3a.1) is checked in the chart only: Activity has no row for it (decision 7 and LIVE-1 list team and member changes, not project changes) | LIVE-1, LIVE-6, STAFF-2, MOVE-1, Success test 3 (`our-org-in-the-app.md`) | Required |
| 3a.15 | Throughout 3a.1 to 3a.13, watch the 3.4 employee | The 3.4 employee keeps working on its own runtime: nothing stops, restarts or pauses it | Success test 3 (`our-org-in-the-app.md`; no criterion ID) | Required |
| 3a.16 | Check words/icons throughout; inspect HR identity and its chosen home team separately from its bootstrap-management restrictions | No deallocate wording or meaning by color alone. HR has a real required home team and separate execution readiness; restricted destructive bootstrap controls do not imply a teamless employee | DEAL-1, data 04 | Required |

## 3b. See what's happening, live (HQ)

Run after 3a, with Dogfood QA 2 (kept from 3a) and the 3.4 employee. Watch the
second browser from 3a, without reloading, after turning its Activity Status
filter back off (Activity's default). Select an authorized owner-linked employee contact with a valid home team for the
message step; its handle is not inferred from the account's login. Owner browser
read authority follows existing chat policy, not any field on the employee record.

| Step | Action | Pass condition | Requirements | Type |
| --- | --- | --- | --- | --- |
| 3b.1 | Give the 3.4 employee a bounded non-repository task. While it works, open Home | Home shows **Needs you**, **Working now** with the 3.4 employee and its current task on one line, and **Latest**: the newest 8 Activity rows with **All activity**. Only real members and tasks; no sample data | LIVE-4 | Required |
| 3b.2 | Send between the two agents, inspect Activity/thread, then send to the selected owner contact and reply where authorized | Activity is filtered to authorized messages. Open thread shows visible content only, preserving directed recipient restrictions and hidden-parent previews. Reply is available only with an authorized recipient/actor; no thread-participant union grants read access | LIVE-2, LIVE-3, SRV-39 | Required |
| 3b.3 | Explicitly start a bounded turn for Dogfood QA 2, then let it finish | Card and Working now reflect the real turn within allowance; Activity default Status filtering works. Turn completion does not close the task, and a message starts no work | LIVE-6, LIVE-4, PROF-3, data 11/15 | Required |
| 3b.4 | Start the task through the task status command, move another task on the board and report a bug type in Dogfood Desktop | Activity and board update once within allowance; all task types have T- references, required accountable team and durable ordered history. Bug detail is kept on the task | LIVE-1, data 03/12 | Required |
| 3b.5 | In Activity, scroll down, then have Dogfood QA 2 send another message. Press **Jump to latest** | The list doesn't move, and an "n new · Jump to latest" pill appears (1 new when only that message arrived). Jump to latest goes to the top with the new row there | LIVE-5 | Required |
| 3b.6 | Merge a reviewed branch into the integration branch of a repository the HQ office fetches | A Git row in Activity ("merged … into …") once the office has fetched | LIVE-1 | Optional |
| 3b.7 | With Activity open in both browsers, run `agentic-org service stop` on the HQ machine. Move to another screen already loaded, and press **Retry**. While HQ is down, have the 3.4 employee send Dogfood QA 2 a message. Then run `agentic-org service start` | Both browsers show "Live updates paused. Showing what was loaded at <HH:MM>." with **Retry**, on every screen; Retry reads "Retrying…" and the line stays while HQ is down. Once HQ is back the line clears by itself, with no sign-in (under ADR-0013 a restart does not end a browser session). The message sent while HQ was down then shows in Activity once, with no gap | LIVE-8, SIGN-17, SIGN-13, SRV-10, SRV-12 | Required |

Row 3b.6 is Optional because this run uses non-repository tasks (§3 note) and
needs a repository the HQ office can fetch; a merge into the integration branch
is still part of the success test, so run it when one is set up. When none is
set up, record 3b.6 as **Blocked** with that reason (§7).

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
| 4.9 | Lose the reply to a forwarded Pause; then retry and look up with the same operation ID | The uncertain outcome stays Pending confirmation, resolves once, and preserves expected-version conflicts | SRV-12, SRV-28 | Required |

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

Verify SRV-29 and SRV-30 with forged path inputs and direct agent creation before
approval: no external path and no unapproved member, home or binding is
provisioned. Confirm the profile displays the launcher's actual managed path.
Verify SRV-32: an unsupported local protocol version fails before dispatch, and
the staged `agentic-org` is the CLI from `apps/agentic-org`.

For SRV-31, person-started commands require the selected instance and office
context, and managed agents use only their injected binding. Row 1.7 verifies
escrow material, not recovery after losing HQ (REC-1).

Verify SRV-22: retained browser code/package uses `apps/agentic-org-ui` / `agentic-org-ui`, and
worker source/package/executable use apps/agentic-org-office / agentic-org-office. Verify agentic-org-web and agent-os helper names and agentic-org serve host composition.
Check workspace, imports, test helpers, release/signing resolution and ownership
references against the [project structure](../design/project-structure.md). Verify
domain/feature moves preserve private SQL, transactions, public boundaries and
licensed UI containment before treating the reorganization as complete.

Verify no production mock fallback exists. Exercise the roster, team, hiring,
task, message and process features against their real HTTP capabilities.
Report unsupported features as gaps; do not substitute test fixtures for product
behavior. Run repository formatting, relevant Rust tests/Clippy and frontend checks,
tests and build on the implementation, with the deletion ledger reviewed.

## 6a. Selected data and recovery checks

Use disposable authored-source fixtures and explicitly supported restore
entry points. No such command is invented here; a missing restore service
makes its row **Blocked** with that capability named. The draft SQL alone cannot
pass these checks. Each row below is Required for a complete selected-model run.

| Step | Action | Pass condition | Requirements | Type |
| --- | --- | --- | --- | --- |
| 6a.1 | Save/reload bootstrap and an employee move | Explicit slug/remote repo/state_branch and home-team/path consistency; offline saves retain remote registration with unverified connectivity/publication until authorized online. Valid direct-member lead/subtree relationship. Owner security grants unchanged. IDs are UUIDv7 and unchanged across reload; no regenerated identities | SRV-35, SRV-36; data 01/04/06 | Required |
| 6a.2 | Create a task type change and cross-team assignment; replay with same ID, then conflicting expected revision; close/archive and reload | All types retain stable ID/T- number. Accountable team is project's owner or team scope; nullable assigned team/assignee pair atomic, including independently null assigned-team/assignee cases. Reporter unchanged, ordered history retained. T- references resolve correctly | Data 03/05/06/12 | Required |
| 6a.3 | Associate one registered repo to two projects with different integration branches, plus a second repo/local-only code registration | Many repos per project and branches round-trip per association; one bare cache per office/repo. Local code supported, local organization Git rejected. Feature definition is feature.md; supporting docs require project and at most one same-project feature/release/deployment scope | SRV-35, SRV-41; data 01/02/03 | Required |
| 6a.4 | Persist acknowledged changes/messages/allocations/receipts; stop processes and remove only the verified disposable projection; rebuild from sources | Same domain versions, actor/fingerprint results, origin/incarnation high-waters and allocations. Hash-verified snapshots, trusted HQ/CA pins and highest epoch checked. Unknown operation side effects reconciled before retry | SRV-38; data 06/16 | Required |
| 6a.5 | Repeat restore with a missing journal/blob, corrupt hash, older authority checkpoint or missing credential; simulate a stale surviving child | Explicit recovery-required/unavailable result with missing coverage; no invented receipt, lower high-water or duplicate execution. A restored PID is never signalled without ownership/machine/boot/start proof; prior cleanup/fence and placement generation verified before start | SRV-26, SRV-37, SRV-38; data 11/16 | Required |
| 6a.6 | Send A→B then B→C; inspect as each participant and authorized owner; retry message/state writes | Message bodies/counts/previews/search/attachments are visible only to each message's audience. Content verified by its normalized canonical-JSON hash. One logical delivery | SRV-39; data 15/16 | Required |
| 6a.7 | Save a private task note and a shared comment, rebuild shared caches and inspect as author/another employee/owner | Author-private separate durable DB survives; no admin exception or existence leak through shared receipts/search/export. Shared comment remains shared under its own policy | SRV-38, SRV-39; data 05/13 | Required |
| 6a.8 | Create partial/unavailable usage, a budget extension and pending/held manual/security/budget reasons; restart/rebuild then release only the budget reason | Protected policy revisions/period/extension-use/notices/holds restored; no invented zero spend/capacity, second extension or held acknowledgement. Other holds remain; release caps warn only. Owner/project-manager cost scope and only the own-agent CLI current-task/daily/monthly amount exception preserved | SRV-40; data 10/17 | Required |
| 6a.9 | Run concurrent turns across two profiles/offices sharing a registered connection; fail one route; restart/relocate with retained work | One canonical subscription quota/permit context, ordered eligible routes and actual attempts/usage. Fresh model/auth evidence required, safe process reuse/switch, no unsafe replay or duplicate placement. Handoff keeps task worktrees and local/unpushed checkpoints; target Git/credential/other-task/managed-state denies verified | SRV-37, SRV-41; data 11/14 | Required |

Restore rows do not demonstrate automatic promotion or zero backup data loss.

## 7. Required rows and the result

**Type.** Every row is **Required** or **Optional**. Required rows decide the
result. An Optional row is still run and recorded; its failure becomes a bug but
doesn't fail the run. §6's checks and every §6a row count as Required.

**Results.** Each row gets one result per platform assignment: **Pass**, **Pass
with notes**, **Fail**, **Blocked** or **N/A**.

- **Blocked:** a capability the row needs isn't implemented (§0). Name the
  missing capability. A row that can't run because an earlier row failed is
  Blocked too, naming that row. So is an Optional row that can't run in this
  setup, such as 3b.6 when no repository is set up: record the reason. A
  failure doesn't stop the run otherwise: carry on with the next row.
- **N/A:** only when this platform assignment can't run the row, with the reason
  recorded beside the result. A missing or unfinished feature is Blocked, never
  N/A. A Required row that is N/A on one assignment must pass on the other; N/A
  on both counts as a Fail.

**Live-update allowance.** The live timings in §3a and §3b are 2 seconds, without
a reload (`our-org-in-the-app.md` decision 8, LIVE-2, LIVE-6). Time each change
from when HQ has recorded it to when it shows in the second browser, so a slow
but correct operation, such as a hire, isn't counted as a slow live update. HQ
has recorded it when it shows in the browser that made it (a change made in the
browser), when the `agent-os` command returns (a change an agent makes, such as
a message, a hire request or approval, or a task move), or when the agent's turn
starts or ends in its own session view (a status change). A change HQ or an
office makes as a result of another (the office provisioning an approved hire,
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
within the allowance or failed Optional rows. A Blocked Optional row doesn't
change the run's result: it isn't an exception, and it files no bug.

**Bugs.** Every Fail, and every Pass with notes, becomes a bug in Agentic Org
(`ac`), with the row number and platform assignment in its title (for example
"3a.11 macOS HQ: Home-team move loses task workspace"):

- **Critical:** a Required row that fails, and any data-loss, authorization or
  duplicate-execution finding, wherever it shows up.
- **Major:** an Optional row that fails, and each timing within the allowance.

**Where results go.** One file per run, `docs/research/dogfood-run-<date>.md` in
this repository, covering both platform assignments: each row's result, time,
release and evidence, the N/A reasons, the Blocked capabilities and the bugs filed,
with credentials redacted. The next run compares against it.
