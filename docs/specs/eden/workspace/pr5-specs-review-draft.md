# PR #5 post-merge review: docs/specs

- PR: #5, `codex/standalone-server-v1-docs`, head `1527ace`, base `7a88278`, merged into `v1` at `4e15a89`
- Scope: the 10 changed files under `docs/specs/`, checked against the 6 unchanged specs at `1527ace` and, for consistency only, the PR's design docs, ADR-0003, ADR-0005 and `standalone-v1-review-decisions.md`
- Spec commits in the PR: `61a8d83` (server pivot), `a4039a0` (supervision, processes, channels), `c5e1a84` (naming), `c025cf8` (review fixes)
- Reviewer: independent subagent for Eden, 2026-11-10. Read-only. Nothing in the app repo was checked out or changed.

## Verdict

**Changes needed: the specs are not yet a safe build and test target for v1.0.** Criterion IDs are unique and none was silently dropped. Two gaps are serious, though. The dogfood run no longer tests the success test that `our-org-in-the-app.md` still points to it for. No spec describes the browser sign-in, instance-admin Create organization or recovery-code screens that SRV-7, SRV-23, SRV-24 and SRV-25 require. Several criteria were also rewritten without revision marks or changelog entries. Some unchanged specs still describe desktop-era offline behaviour.

Findings: **2 high, 11 medium, 7 low.**

## Findings

### HIGH

**H1. The dogfood run no longer tests the success test that our-org still assigns to it**
- Files: `our-org-in-the-app.md` "Success test (the dogfood run)" (items 2 and 3, unchanged in meaning) and `dogfood-run.md` rev 5 (§1–§6).
- Quote (our-org): "2. Home and Activity show, live, what the team is doing … 3. The owner hires a new agent, assigns it to a team, a project and a task, moves an agent to another team, pauses an idle agent and offboards one. Every change shows at once in the chart and Activity."
- What the dogfood run now maps: the only non-SRV criterion is EMP-3 (row 4.9). No row covers LIVE-2/4/5/6 (2-second live updates, Home, Jump to latest), STAFF-1/2, APPR-*, MOVE-1/2, DEAL-1/2/4, HRL-3 or SRV-30's approval-before-provisioning. §6 waves at them: "Exercise the retained roster, team, hiring, task/message and process features against their real advertised HTTP capabilities". The old pass rule was removed with nothing in its place ("1.5 under 5 minutes", the 4 s allowance, bug severities, `docs/research/dogfood-run-<date>.md`). The new rule says "The run passes only with required rows complete on both platform assignments", but no row is marked required and no N/A rule is given.
- Why it matters: SRV-21 makes the dogfood run part of v1.0 acceptance. A tester can pass it without ever hiring, moving, offboarding or watching Activity update live. Meanwhile our-org still says that run proves those flows.
- Minimal fix: add a "§3a Manage the team" table that hires, approves (owner, and an agent with Can approve hires), assigns, moves, pauses/resumes and offboards throwaway members, plus a "Live" table. Map each row to LIVE-2/4/5/6, STAFF-1/2, APPR-1/2/5, MOVE-1/2, DEAL-3/4 and SRV-30. Mark which rows are required, state the N/A rules, and restore the timing allowance and where results are recorded. Or, if that is out of v1.0, rewrite our-org's success test to say which run validates items 2–3.

**H2. v1.0 has no UX spec for browser sign-in, first-owner access, instance-admin Create organization or recovery-code delivery**
- Files: `server-installation-and-access-ux.md` §2 step 3 and §4, plus SRV-7, SRV-23, SRV-24 and SRV-25. `offices-and-teams-ux.md` §5 removed the old "+ Create organization" entry and first-launch cards and replaced them with nothing. Dogfood rows 1.1, 1.5, 1.6, 1.7 and 2.7 exercise these screens.
- Quotes: "HQ instance-admin/owner access, gateway listening/TLS configuration, and protected CA escrow/recovery-code provisioning with off-HQ retention acknowledgement" (§2). "First setup must establish owner access before administration is available" (§4). SRV-25: "protected one-time recovery-code delivery and off-HQ retention acknowledgement".
- What's missing: the sign-in screen and its errors (wrong credential, expired session, missing origin proof that needs reauthentication per `standalone-server.md` §6, sign-out); how first owner access is bootstrapped (in the CLI wizard or a single-use browser exchange?); whether added people (offices §7.4) can sign in at all; the instance-admin **Create organization** screen described in `docs/design/create-organization.md` (name, "which HQ instance will store it", Starting, the lost-response lookup); and the recovery-code screen (display once, acknowledge off-HQ retention, interrupted delivery, "recovery material not ready" state). "Instance admin" also appears in no glossary or who-can-do-what list (offices §1 and decision 15 know only the owner).
- Why it matters: every tester and the front-end developer will hit these screens first. As written, the copy, states and accessibility will be improvised, and dogfood 1.7 ("acknowledged delivery is not later redisplayed") has no specified UI to check.
- Minimal fix: add a "§4a Sign in, first access and recovery code" flow to `server-installation-and-access-ux.md` (or a short new spec), and add "Create organization (instance admin)" to offices §5. Each needs its states (empty, loading, error, success, expired session) and copy. Add "Instance admin" to the offices glossary with what only it can do (SRV-24).

### MEDIUM

**M1. Criteria rewritten without revision marks**
- `agent-run-modes-ux.md` RUN-10: its meaning changed from "Pausing … stops its schedule" to "HQ durably accepts Pause with pending office stop…" with no mark. The rev 7 changelog lists only RUN-14/15.
- `our-org-in-the-app.md` HRL-3: the existing "*(rev 8)*" mark was deleted and no new one added. DEAL-3 was fully rewritten with no mark. STAFF-4 uses a non-standard "*(standalone amendment)*". Rev 18's changelog doesn't list PROF-2, PROF-4, STAFF-4, HRL-3 or DEAL-3 as "Criteria rewritten (same ID)", which this spec's own convention requires.
- `offices-and-teams-ux.md` OFC-3 and OFC-4: rewritten (Copy link → Copy invitation, plus a new check-code verification rule) with no mark, while the neighbouring OPEN/JOIN/OFC criteria got "*(rev 12)*". `standalone-v1-review-decisions.md` finding 9 confirms they changed.
- Why it matters: anyone with tests keyed to these IDs can't tell the behaviour changed.
- Fix: add "*(rev 8)*" to RUN-10, "*(rev 19)*" to HRL-3, DEAL-3 and STAFF-4, and "*(rev 13)*" to OFC-3/4. List them as "Criteria rewritten (same ID)" in the matching changelog entries.

**M2. Revisions bumped with no changelog entry; the new specs have no changelog at all**
- Commit `c025cf8` bumped agent-run-modes 7→8, chat 6→7, feature-requests 4→5, offices 12→13, our-org 18→19 and dogfood 4→5 (plus employee-supervision 1→2 and server-installation to 3). None of those spec revisions has a changelog entry. Their changes included RUN-10, DEAL-3, EMP-3, LIVE-8 copy in offices and server, and removing ⌥⌘N from the mock-up.
- `server-installation-and-access-ux.md` (rev 3) and `employee-supervision-ux.md` (rev 2) have no Changelog section. `dogfood-run.md` deleted its rev 1–3 history and shows only "Revision 4".
- Changelog entries are out of order or misplaced: chat rev 6 sits after rev 2; agent-run rev 7 after rev 2; feature-requests rev 4 sits above the intro while rev 2/3 sit below "The request"; roles rev 2 sits inside "§0 Decisions" after decision 7, not under "## Changelog".
- Fix: add a one-paragraph entry for each missing revision naming the rewritten criteria. Add Changelog sections to the two new specs. Restore the dogfood rev 1–3 entries as history. Move each entry to the top of its spec's changelog.

**M3. Pending and outcome states have no shared vocabulary across specs**
- `server-installation-and-access-ux.md` §4: "**Queued**, **Accepted** and **Completed** … rejected and unknown outcomes remain visible".
- `employee-supervision-ux.md`: "**Pending delivery** … 'Delivered' means transport acceptance", and separately "stays Pending confirmation".
- `our-org-in-the-app.md` §9 and DEAL-3: "**Paused · stop pending at office**". `agent-run-modes-ux.md` table: "show stop pending at office".
- `offices-and-teams-ux.md` decision 13 and §6.5: "Pending" and "Blocked".
- Why it matters: a Playwright test asserts exact labels. Is "Pending confirmation" the same state as server §4's unknown outcome? Is "stop pending" an Accepted-not-Completed state? Nobody can say, and the card status vocabulary (our-org §5: Working · Waiting · Idle · Needs you · Paused · Offline) doesn't include the compound label.
- Fix: add one table in `server-installation-and-access-ux.md` §4 listing each operation state, its exact label, icon and meaning (for example Saved / Pending delivery / Delivered / Pending confirmation / Unavailable / Rejected). Have EMP, DEAL-3, RUN-10 and offices §6.5 point to it.

**M4. employee-supervision-ux: the Supervision section lacks most states and disabled reasons**
- §1 lists Health and Memory examples, but there's no empty state (no goal set, no internal work, no checkpoint yet), no loading state, and no can't-load or error state for the section.
- The offline case is described only in prose ("an unavailable satellite shows stale status"). The Health list ("Responding · last event 20 s ago", "Waiting for approval", "Waiting on tool · deadline 14:30", "Asleep", "Unresponsive · probe timed out") omits Paused, "waiting on a … message" (decision 8), "Started by a person · supervision unavailable" (decision 2) and stale/offline.
- **Edit goal** is "for an authorized caller", but there's no rule for unauthorized viewers (hidden or disabled with a reason). There's no copy for "rejected as unavailable" when the office is known to be offline, and none for the expected-version conflict that EMP-3 tells testers to test.
- Fix: add a states list to §1 (empty, loading, can't load, offline/stale, person-started) with exact copy. Say whether Edit goal is disabled, and what its reason says, when the caller isn't authorized and when the office is known to be offline ("Studio Mac is offline. Goals can't change until it reconnects."). Add the conflict message ("The goal changed since you opened it. Review the new version.").

**M5. server-installation-and-access-ux: the CLI wizard, status and join have no user-facing copy or accessibility rules**
- §2 lists the wizard's steps, §5 lists status labels ("Installed, Service running, HQ ready, Awaiting approval, Synchronizing, Office ready"), and offices §5 says "Identity mismatch, expiry, rejection, incompatible versions and connectivity failures have distinct explanations and repair actions". None of them gives copy. The old §5.3 error table, with exact copy and actions, was deleted and not replaced, so JOIN-7 now has nothing to check against.
- JOIN-5 reads "The joining CLI displays the check code accessibly", which is undefined (the old JOIN-5 said groups of four, read character by character).
- No statement covers terminal accessibility: status shown as words not color, screen-reader-friendly output, and `--json` or plain output.
- Fix: add a CLI copy table for each wizard step's prompt, progress line, the 5 join failures and each status label. Define the check-code display (two groups of four, plain text, no color-only cue). Add one accessibility line for CLI output.

**M6. Many new and changed criteria are vague or bundle unrelated behaviour**
- Vague: SRV-11 "shows last-known data and freshness accurately". SRV-15/SRV-17 "within documented deadlines", but no deadlines are documented in the spec or in `standalone-server.md` §4 ("within deadlines"), so dogfood 5.2 can't be judged. SRV-20 "deletes superseded paths" doesn't name the removal inventory as its oracle. EMP-13 "accessibly". JOIN-5 "accessibly". PROC-28 "concise `org-agent` command help".
- Bundled: SRV-32 mixes protocol negotiation, HTTP/launch/sync version spaces, a Cargo package name and artifact staging. SRV-22 is a source-tree rename, not observable behaviour. SRV-3 mixes instance isolation with cookie/proof isolation, which SRV-23 already covers. EMP-3 has 7 behaviours. EMP-10 lists 5 lifecycle events. OFC-4 mixes "New invitation cancels the previous one" with check-code verification. RUN-14 delegates to all of "EMP-1..14".
- Hard to verify: EMP-7 "code never mandates one delegate type for a semantic task category" and CHAT-21 "driven by an overridable process" describe agent behaviour no browser or CLI test can observe.
- Fix: split SRV-32 into a protocol criterion and a build/packaging criterion, and move SRV-22 to the project-structure acceptance. Give SRV-15/17 numeric deadlines or a link to where they're defined. Split OFC-4 and EMP-3 (offline rejection vs delivery points vs retry). State EMP-7 and CHAT-21 as checkable facts (for example "the seeded process file exists, and editing it changes the delivered guidance text with no deployment").

**M7. The owner's v1.0 scope isn't visible in the specs**
- Per the owner's decision relayed in this review's brief (v1.0 = SRV-1..32 plus the dogfood run; delegates, channels, processes and repository work are Wave A). That decision isn't recorded in docs/specs at `1527ace`.
- The new CHAT-19..23, PROC-27..30, EMP-4..9/11/12 (delegates, specialists, memory helpers), RUN-14/15 and ROLE-11 carry no release tag. `dogfood-run.md` §6 still requires "Exercise the retained … process features", and row 3.5 requires that the "supervisor accounts for it". SRV-20 requires "supervision invariants remain covered".
- Why it matters: implementers will build Wave A behaviour now, and testers will fail v1.0 for missing Wave A features.
- Fix: add a "Release" line to each new criterion block (v1.0 or Wave A), or a scope table in `server-installation-and-access-ux.md` §0. Drop "process features" from dogfood §6, or tag it Wave A.

**M8. Mac-only shortcuts in browser specs for a Linux-supported v1**
- `feature-requests-ux.md` decision 2 and RQS-2 "press ⌘Enter", the mock-up "[Send to Eden ⌘↵]" and "macOS dictation works in the field". `chat-ux.md` §1 "⌘Enter to send". `work-items-ux.md` ⌘K and ⌘S (unchanged).
- Why it matters: dogfood §0 requires a Linux machine and row 1.2 opens HQ from the other machine's browser. A tester on Linux can't press ⌘.
- Fix: add one line to the server spec or the design system: "⌘ means Ctrl on Linux and Windows browsers". Rewrite RQS-2 as "⌘Enter (Ctrl+Enter off macOS)".

**M9. chat-ux: the new channel Question has no presentation or full flow**
- §1 Message markers are "❓ ask · 📣 announcement · 🔁 hand-off". The ❓ already means Ask, and the §2 kinds table has no **Question** row, so how a Question renders, and how answering or replying looks, is undefined.
- §4.1 offers "**Join** or **Request invitation**", but the invitation request has no flow (who receives it, its states, decline). ADR-0003 §6.8 makes manual channels "invitation-only" by default, so this path is the common one.
- CHAT-20 says the question is asked "in a thread", but ADR-0003 §6.8 makes it a channel root with replies "under its root".
- `@here` (§1) isn't reconciled with §4.1's "Only an addressed mention … produces a run trigger": does @here wake every member?
- Fix: add a Question row to §2 with its own marker (for example "💬 Question · nonblocking") and its reply behaviour. Add a "Request invitation" bullet with its states. Change CHAT-20 to "posts a channel Question; replies form its thread". Say whether @here wakes.

**M10. Unchanged specs still have the browser queue organization changes while HQ is unreachable**
- `work-items-ux.md` §12: "Later phases (a Mac whose office can't reach HQ) … Items created in the app … the office queues the create and the item shows **Pending** … Blocked with Retry or Discard (offices-and-teams §6.5)". Also the §3 states line: "banner from offices-and-teams §3.2".
- `offices-and-teams-ux.md` §7.4: "On an office that can't reach HQ, it shows as **Pending**", and §7.5 "On an office that can't reach HQ it appears **Pending** (§6.5)".
- These contradict the rewritten offices decision 13 and §6.5 ("It cannot save new organization changes until HQ acknowledges them … This is distinct from an unsaved browser draft"), server spec §4 and RQS-17. §6.5 no longer has the Pending/Blocked banner, and §3.2 no longer has a banner, so both references are orphaned.
- Fix: in work-items §12 replace the "Later phases" block with "The browser talks only to HQ. While it can't reach HQ: Live updates paused (LIVE-8), writes disabled with the reason. Agent-created items at a satellite follow offices §6.5." In offices §7.4/7.5, say Pending applies only to changes made by agents at a disconnected office. Point work-items §3 at LIVE-8.

**M11. feature-requests: capture while disconnected is half-specified**
- §1 still says "**Esc** with text asks 'Keep as a draft?' (**Keep** saves it as an unrouted request…)", which is impossible while HQ is unreachable. §1.1 now uses "draft" for page-only text ("Reopening keeps an unsent draft in the current page"), so "draft" means two things.
- §1.1 says "Send/Just save cannot report success without durable HQ acknowledgment", but doesn't say whether the buttons are disabled with a reason (as Message instead is) or fail after the attempt, and gives no failure copy.
- Fix: rename the page-only text to "unsent text". Make Esc offer only **Discard** or **Back** while disconnected. Disable Send and Just save while HQ is unreachable with "HQ can't be reached. Your text is kept on this page; copy it or retry when connected.", matching Message instead.

### LOW

**L1. Leftover Mac/desktop wording.** `agent-run-modes-ux.md` §3 table "The Mac is asleep … (the Mac was asleep)" and RUN-8 "(Mac asleep, office stopped)". `offices-and-teams-ux.md` decision 2 "Offices run under the login service" (now launchd/systemd user services, with Linux lingering caveats) and §3.3 `storage_error` "Keychain". `our-org-in-the-app.md` §9 Offboard "Remy's records stay in the HR workspace". §2a "**Import:** the HR toolkit's own agent … maps onto the built-in HR lead" and §1 "Import must show this". Fix: say "the office machine was asleep or stopped", "the instance's user service" and "credential store", and drop the HR-workspace and import sentences.

**L2. our-org §5 "No home folder (this agent runs elsewhere)"** contradicts SRV-13, PROF-4 and "Add agent record … No external home path … is accepted". Every managed agent has a server-managed home. Fix: drop this example or replace it with "Home not provisioned yet".

**L3. Offices glossary not updated.** §1 still defines "Invitation link: The one-time link…" and "Check code … typed at HQ", and §3.1 still has "Invited: An invitation link exists" and "A computer used the link". Review decision 9 requires "Copy invitation/New invitation" consistently. The glossary row "This instance" doesn't match the mock-up and screen-reader label "HQ instance". Fix: rename the term to "Invitation", and pick either "This instance" or "HQ instance" and use it in §1, §6.1 and OFC-13.

**L4. Offices §3.1 "Unreachable"** is now "HQ cannot be reached by the browser or the joining CLI". But server §4 says "If HQ is unavailable, its web application is unavailable", and an open page shows LIVE-8 instead. No browser screen can show this label. Fix: limit Unreachable to the CLI and status output, or remove the browser half.

**L5. Offices §6.2 lost validation and choices.** The office name rule ("1–64 characters, unique … 'There's already an office called Studio Mac.'") and the invitation expiry choices (1/4/24 h) were deleted; §3.1 still shows "expires in 21 h". Fix: restore both lines.

**L6. Misplaced LIVE-8 line.** In offices, "Connection-loss copy follows LIVE-8 exactly…" sits at the end of §6.4 Remove office. It belongs in §6.5. Move it.

**L7. Front matter of `employee-supervision-ux.md`.** It has `owner: owner`, but the project brief has Eden owning docs/specs. Its `builds_on` omits `server-installation-and-access-ux`, though its preamble depends on it. Dogfood §0 "The inspected branch does not yet meet these prerequisites" will go stale. Fix: set `owner: ac-product-designer-eden` (or record the exception), add the builds_on entry, and date the dogfood sentence ("as of 1527ace").

## Checks run

1. **IDs.** I extracted all `- <PREFIX>-n ·` criteria at base and head (443 at head, 16 specs).
   - No duplicates within a spec or across specs.
   - No ID removed. IMP-1..11 and MENU-1..6 are kept and marked retired, "not reused".
   - 58 new IDs (SRV-1..32, EMP-1..14, CHAT-19..23, PROC-27..30, RUN-14/15, ROLE-11) collide with no existing prefix.
   - 48 existing criteria changed. 5 lack a revision mark (RUN-10, HRL-3, DEAL-3, OFC-3, OFC-4) and 1 has a non-standard mark (STAFF-4), as in M1.
   - About 45 cross-references checked and all resolve at `1527ace`. That covers links to the design docs, ADR-0005, the review-decisions file, `#6-acceptance-criteria`, ADR-0003 §6.4/§6.8, LIVE-8, EMP-3, PROF-4, STAFF-4, HRL-3, DEAL-3, REC-1, RUN-6/12, PROC-29, A30 and A76. The exception is two content-orphaned section references (work-items → offices §6.5 and §3.2), covered in M10.
2. **Testability.** I read all 58 new and 48 changed criteria. 15 are flagged as vague, bundled or not observable (M6). Retired stubs weren't assessed.
3. **Consistency.** I compared the changed specs against all 6 unchanged specs and against `standalone-server.md` §6, `create-organization.md`, ADR-0003 §6.8 and the Phase A cutover, `processes.md` (the 24 KiB cap is consistent) and `standalone-v1-review-decisions.md`. I grepped every head spec for desktop-era terms (This Mac, menu bar, tray, native notification, ⌥⌘/⌘, Keychain, Login Items, session panel, Open session, HR workspace, import) and triaged each hit, covered in M3, M8, L1, L2, L3 and L4.
4. **Breakage.** I checked unchanged-spec references into rewritten sections (offices §3.2, §4, §5, §6.5; our-org §4; feature-requests §1.1). The broken ones are work-items §12/§3 and offices §7.4/§7.5 (M10), and the our-org success test (H1). No unchanged criterion depends on a retired IMP or MENU criterion.
5. **UX quality of the new specs.** I checked each flow in both specs for states (empty, loading, error, success, offline/pending), status not shown by color alone, disabled reasons, plain copy and accessibility.
   - Employee supervision states status in words and times, not color alone. It lacks empty, loading and error states and disabled reasons (M4).
   - Server installation has no user-facing screens or copy for sign-in, create organization or recovery code (H2), or for the CLI (M5). Its status labels are words.
6. **Front matter and changelog.** All 10 changed specs checked. 6 have a revision bump with no changelog entry, the 2 new specs have no changelog, the dogfood history was deleted, and 4 specs have entries out of order (M2). The `updated` dates match their last commit.

Not checked: whether the design docs are internally correct (out of scope), and the owner's v1.0 decision text itself (it's not in the repo; I took it from the brief).
