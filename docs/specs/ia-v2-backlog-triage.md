# IA v2 backlog triage (for ada, 01M7880N)

2026-11-12 · Owner: Emery
Sources: PRD `information-architecture-v2.md` rev 4.1 (D1-D35), `ia-v2-stories.md` (IA-01 to IA-19), Rowan's nav freeze rev 5 (`nav-freeze-rev5-ia-v2.md`, `6c98d5e`), ada's BUGS.md and STATUS.md (to 16:16), Avery's and Jules's backlogs, prototype draft 3.1.

- **KEEP:** the screen or component survives in IA v2 (even if it moves). Do the work as queued.
- **RE-POINT:** the work is still needed, but an IA story and AC now define it. Do it inside that story, not as separate polish.
- **RETIRE:** IA v2 removes the screen or behaviour, so the bug can't exist any more. Close it when the named story lands, and don't fix it in the meantime. If the named story slips out of its milestone, a keyboard, a11y or security defect on a shipped surface comes back as KEEP.

**Columns.** Rowan (01M788CH, 01M7884P): no row needs a backend or shared-contract change, and every path move is mechanical in S1a (redirects plus the e2e migration). His per-item notes are in his column, and "—" means he has no note. Eden: her per-row design notes are due at 17:00. Until they arrive her column reads "pending", and I'll send ada a delta if she changes any call.

**Two product decisions in this list** (Emery, 2026-11-12; story wording updated, AC IDs unchanged):
- **D36 · The org chart is one view.** IA-02's vertical tree replaces all three of today's org-chart views (Teams, People and Reporting lines), not just Reporting lines. Teams live as Teams index rows with their hierarchy (D32) and people live in Members (AC-IA02-2), so the canvases, their View menu, Fit and the chart search go. AC-IA02-3 now reads: "the Org chart is a vertical tree with the owner card at its root. It replaces today's Teams, People and Reporting lines views; teams are in the Teams index (D32) and people in Members." Reason: three ways to draw the same org is the technical, agent-first surface the owner asked us to move away from, and it's where most chart bugs live.
- **D37 · Projects keep team assignment.** The project's Team tab shows the assigned teams first (today's chips, with "Add team…" and remove), then the members as Members rows. AC-IA17-2's Team bullet now reads: "the teams assigned to the project (add and remove, as today), then the members as Members rows." Reason: projects are staffed by teams (org → projects → teams), and dropping a shipped capability isn't in IA v2's scope.

| ID | What it is (one line) | Current screen/route | Call | IA story / AC | Reason (one line) | Eden | Rowan |
| --- | --- | --- | --- | --- | --- | --- | --- |
| BUG-146 | "Create organization" button wider than the viewport at 320/200% | `/create` | KEEP | — | Onboarding survives (nav rev 5 sends it to `/home`); reflow defect on a shipped surface. | see "Eden's calls" | KEEP: same path |
| BUG-112 | Projects list intro and sample note clip at 320 and 960/200%; `/offices` header overflows at 320/root40 | `/projects`, `/offices` | KEEP | AC-IA03-2 (Offices moves unchanged) | Both screens survive; reflow defect on a shipped surface. | see "Eden's calls" | KEEP: screens survive |
| BUG-113 | Project-page breadcrumb chevron colour/alignment; Eden's fix is a "← Projects" back link | `/projects/:id` | RE-POINT | IA-17 / AC-IA17-1 | AC-IA17-1 already specifies the header with "← Projects" back; build it there. | see "Eden's calls" | Re-point to IA-17 if its tabs rebuild the header (they do) |
| BUG-118 | "Add team..." accessible name doesn't contain the visible text (WCAG 2.5.3) | `/projects/:id` | KEEP | IA-17 / AC-IA17-2 (Team tab keeps team assignment, D37) | a11y defect on a shipped surface; the "Add team…" control survives on the Team tab (D37). | see "Eden's calls" | KEEP: same path |
| BUG-107 | Search placeholder gray-500 (contrast) and clipped at 320/200% | Search field (org chart toolbar, OC-8) | RETIRE | IA-02 / AC-IA02-3 (D36) | The chart toolbar search goes with the canvases (D36); finding people is IA-13's top-bar search and the Members filter. | see "Eden's calls" | Re-point to IA-13 if that field is replaced; don't fix twice |
| BUG-131 | ProcessSheet facts grid collapses to one glyph per line at 320/root40 | `/processes` sheet | RETIRE (v1.0) | AC-IA03-3 (Processes gated in production) | No v1.0 user can reach it: production shows "Processes isn't available yet" (D29). Reopen when Processes ships. | see "Eden's calls" | Code stays only for e2e builds; no v1.0 user value, so RETIRE or defer |
| BUG-60 | Processes heading squeezed at 320 px; stack heading above Show below 640 | `/processes` | RETIRE (v1.0) | AC-IA03-3 | Same as BUG-131. | see "Eden's calls" | as BUG-131 |
| S1a | Nav rev 5 routes, legacy redirects, `PAGES`, flat v2 NAV, `aria-current`, focus, old-link scan | shell, all routes | RE-POINT | IA-01 / AC-IA01-3, AC-IA01-4; IA-07 / AC-IA07-1 (route) | Re-scoped S1; the IA-M1 opener. Starts after Dialog B/A and chart A merge (STATUS 16:16). | see "Eden's calls" | RE-POINT to IA-01 |
| S1b | Sidebar groups, footer account menu and office line, rail, drawer | shell | RE-POINT | IA-01 / AC-IA01-1, AC-IA01-2 | Defined by nav rev 5 decisions 5-6. | see "Eden's calls" | RE-POINT to IA-01 |
| BUG-119 | rem-sized sidebar takes 720 of 960 px at a 40 px root; main column too narrow (also carries BUG-162) | shell | RE-POINT | IA-01 / AC-IA01-1 via S1b | Already folded into S1b (STATUS 16:16); Rowan's px-width/rail amendment pins the fix. | see "Eden's calls" | RE-POINT to S1b (sidebar rebuilt there) |
| BUG-151 | Shift+Tab leaves a focused row under the sticky top bar (WCAG 2.4.11) | `/offices` | KEEP | AC-IA03-2 | Offices survives unchanged at `/settings/offices`; a11y defect. Re-check after S1b before fixing, since the fix is shell-level (scroll-padding). | see "Eden's calls" | KEEP; re-validate after S1b and the PageHeader |
| BUG-158 | Shared DropdownMenu closed by pointer leaves focus on body | shared primitive | KEEP, raised | IA-01 / AC-IA01-2 (account menu); IA-19 / AC-IA19-2 (New ▾) | Shared primitive. S1b's account menu and IA-19's New ▾ both inherit it, so land it before S1b. | see "Eden's calls" | KEEP and raise priority: S1b's account menu (side=top) inherits it |
| BUG-160 | Ask card option buttons 34 px/left-aligned vs Reply 32 px/centred (regression) | `/chat` | KEEP | IA-09 / AC-IA09-3 (chat asks become the first Inbox source) | Chat and its Ask cards survive. Fix it once, in a shared card action row that IA-09's Inbox card reuses. | see "Eden's calls" | KEEP; one shared action row for Chat's Ask card and IA-09's card |
| BUG-161 | Conversations drawer heading breaks mid-word at 200% / 40 px root | `/chat` | KEEP | — | Chat survives; reflow defect. | see "Eden's calls" | KEEP: Chat survives |
| BUG-164 | Coupled Dialog inset (pad 1rem below 640, Close at pad - 0.5rem) | shared Dialog | KEEP | — | Shared primitive (ada); after Dialog B/A, Rowan's design-system section first. | see "Eden's calls" | KEEP: shared primitive (his #52) |
| Chart cut A (BUG-114/115/117) | Fit remove target size, keyed focus after unlink, ring clearance | `/org-chart`, `/projects/:id` chips | KEEP | — | In flight; finish and merge (ada). The project chips survive (D37). | see "Eden's calls" | Merge before S1a starts, or S1a rebases it |
| BUG-137 | Fit explanation flush to the viewport edge, covers View trigger | `/org-chart` People | RETIRE | IA-02 / AC-IA02-3 (D36) | The People canvas and Fit go (D36). Chart cut B isn't started. | see "Eden's calls" | — |
| BUG-138 | Fit explanation disappears after the blocked item is activated | `/org-chart` People | RETIRE | IA-02 / AC-IA02-3 (D36) | As BUG-137. | see "Eden's calls" | — |
| BUG-139 | Test helper holds any ResizeObserver containing the chart | org-chart e2e | RETIRE | IA-02 (D36) | The spec goes with the People canvas. | see "Eden's calls" | — |
| BUG-103 | People chart scrolls at desktop widths with no cue | `/org-chart` People | RETIRE | IA-02 / AC-IA02-3 (D36) | The People canvas goes. | see "Eden's calls" | — |
| BUG-104 | People "lead" badge read twice | `/org-chart` People | RETIRE | IA-02 / AC-IA02-3 (D36) | The People canvas goes. The tree and Members rows must announce "lead" once (IA-02 test). | see "Eden's calls" | — |
| BUG-105 | Add person and View on separate rows; "Sample data" shown twice | `/org-chart` toolbar | RETIRE | IA-02 / AC-IA02-3 (D36) | The chart toolbar's View menu goes; Add a person moves to "Hire or add" (D33). | see "Eden's calls" | — |
| BUG-106 | Owner region reads "...Owner Sample data of Acme" | `/org-chart` | RE-POINT | IA-02 / AC-IA02-3 (owner card at root) | The owner card survives as the tree's root, so build its accessible name right there. | see "Eden's calls" | — |
| BUG-108 | WebKit ResizeObserver loop error on Reporting lines | `/org-chart` Reporting lines | RETIRE | IA-02 / AC-IA02-3 | AC-IA02-3: the vertical tree replaces the "Reporting lines" view. | see "Eden's calls" | — |
| BUG-109 | org-chart-responsive :102 times out (Reporting tab / Teams screenshot), pre-existing | org-chart e2e | RETIRE | IA-02 (D36) | The spec goes with the three views. | see "Eden's calls" | — |
| BUG-111 | CSP Fit-tooltip flake (:260), unclassified | org-chart e2e | RETIRE | IA-02 (D36) | Fit and its tooltip go. | see "Eden's calls" | — |
| BUG-90 | Org chart on phone starts below the first screen; Fit shrinks text | `/org-chart` | RE-POINT | IA-02 / AC-IA02-3 | Phone layout of the vertical tree (indent per level) is defined there. | see "Eden's calls" | — |
| BUG-156 | Last team deleted live while focus on View: focus to body | `/org-chart` People | RETIRE | IA-02 (D36) | The View menu goes. | see "Eden's calls" | — |
| BUG-157 | Delete team returns focus to the removed button | `/org-chart` Teams | RETIRE | IA-02 (D36); D32 | The chart's Teams view goes. Team delete moves to the Teams index (D32), where focus must land on the next row; carry that as a check there. | see "Eden's calls" | — |
| BUG-159 | With View open, clicking another tab only closes the menu | `/org-chart` | RETIRE | IA-02 (D36) | The View menu goes. | see "Eden's calls" | — |
| BUG-145 | Destructive menu text 4.36:1 on the highlighted item | shared menu token | KEEP | — | Shared primitive; contrast defect. | see "Eden's calls" | — |
| BUG-149 | Add agent sheet team options have no focus indicator | Add agent sheet | KEEP | IA-07 / AC-IA07-3 (Hire flow) | The hire flow survives (copy becomes "Hire"); a11y defect. | see "Eden's calls" | — |
| BUG-129 L1 | Add BUG-129 to the WebKit test grep | org-chart e2e | KEEP | — | Test-only follow-up. | see "Eden's calls" | — |
| BUG-70 A-OC2 | Next org chart look slice | `/org-chart` | RE-POINT | IA-02 / AC-IA02-3 | The org chart's look is now the IA-02 tree; no new look slice on today's chart. | see "Eden's calls" | — |
| F2-02 A-8 | Gateway org chart, read-only and editing parts | `/org-chart` | KEEP | IA-02, IA-05 (org view data) | Data and editing feed IA-02's tree, the Teams index and D32 team editing. Re-scope it to that (D36). | see "Eden's calls" | — |
| Agents Home + Org-chart tab | Rev 4 "Agents as Home" dashboard and Org chart tab | `/agents` | RETIRE | IA-04 (D1), IA-02 (D10) | PRD §9 supersedes nav rev 4; Home is its own page. | see "Eden's calls" | — |
| S2 validation S4 | Content gutters, tab padding, office label truncation | `/agents`, `/org-chart`, `/offices` | RE-POINT | IA-02 / AC-IA02-1 | The S2 switch becomes the Teams view switch; office-label part: verify it is closed by A-OF1. | see "Eden's calls" | — |
| BUG-28 | Double-click Pause: the second click lands on Add agent behind the dialog | `/agents` dialogs | KEEP | IA-07 / AC-IA07-2 | Pause survives in Agent setup and the profile More menu; verify after IA-19 removes the top-bar Add agent. | see "Eden's calls" | — |
| BUG-29 | How it runs › Scheduled dialog cut off at 960x640, can't scroll | `/agents` dialogs | KEEP | IA-07 / AC-IA07-2 | Dialog survives; verify after Dialog A (shared DialogContent scroll). | see "Eden's calls" | — |
| BUG-30 | Roster table overflows at 960x640 | `/agents` roster | RE-POINT | IA-02 / AC-IA02-2; IA-07 / AC-IA07-1 | The roster splits into Members rows and the Agent setup list; both must fit at 960. | see "Eden's calls" | — |
| BUG-31 | Long agent name pushes confirm buttons out of Pause/Offboard dialogs | `/agents` dialogs | KEEP | IA-07 / AC-IA07-2 | BUG-144 family; verify after Dialog A. | see "Eden's calls" | — |
| BUG-32 | Focus drops to body after Pause, Resume or Offboard (WCAG 2.4.3) | `/agents` dialogs | KEEP | IA-07 / AC-IA07-2 | Keyboard defect; the actions survive. | see "Eden's calls" | — |
| BUG-33 | How it runs radios have no visible focus in WebKit | `/agents` dialogs | KEEP | IA-07 / AC-IA07-2 | a11y defect; the control survives. | see "Eden's calls" | — |
| BUG-34 | Add agent sheet selects 20 px next to 36 px inputs in WebKit | Add agent sheet | KEEP | IA-07 / AC-IA07-3 | The hire flow survives. | see "Eden's calls" | — |
| BUG-35 | Contrast below 4.5:1: Offboard button, roster filter counts, sheet errors | `/agents` | KEEP | IA-07 / AC-IA07-2 | Contrast defect; the roster filter-count part retires with the roster (verify). | see "Eden's calls" | — |
| BUG-36 | Disabled Add/Save give no reason; name over 64 chars refused only on submit | Add/Edit agent | KEEP | IA-07 / AC-IA07-3 | The hire and edit forms survive. | see "Eden's calls" | — |
| BUG-37 | S4 bundle: "No left agents", Set runtime action, errors, Offboard copy, `?member=` | `/agents` | RE-POINT (split) | IA-07 / AC-IA07-2 (Set runtime, D20; Offboard copy); IA-02 / AC-IA02-2 (`?member=`, roster) | Split per Eden: the setup parts go to Agent setup and the directory parts go to Members. | see "Eden's calls" | — |
| BUG-55 | Public needs-you count for the nav badge (Avery count, Jules badge) | shell + Chat | RE-POINT | IA-09 / AC-IA09-4; IA-01 / AC-IA01-1 | The Inbox badge and Home read one count. | see "Eden's calls" | — |
| BUG-44 (part 2) | Chat history, draft and Not delivered kept across reload | `/chat` | KEEP | — | Chat survives; stays with F2-08. | see "Eden's calls" | — |
| BUG-62 | Chat Back menu entries carry the next place's title | `/chat` | KEEP | — | Chat survives. | see "Eden's calls" | — |
| BUG-63 | Office stopped: Members and channel admin actions stay enabled, then fail | `/chat` | KEEP | — | Chat survives. | see "Eden's calls" | — |
| BUG-67 | Chat list names truncate at 200% text | `/chat` | KEEP | — | Chat survives; reflow defect. | see "Eden's calls" | — |
| BUG-68 | Chat search stays narrow below 640 px | `/chat` | KEEP | — | In-Chat search survives; verify it isn't replaced by IA-13. | see "Eden's calls" | — |
| BUG-84 | Chat heading collapses at 200% (WCAG 1.4.4); nav takes most of 960 px | `/chat` | KEEP | IA-01 / AC-IA01-1 (nav-width part, via S1b) | a11y defect; the nav-width half goes with S1b. | see "Eden's calls" | — |
| BUG-85 | Back with Members or Thread open: focus to body (SHELL-28) | `/chat` | KEEP | — | Keyboard defect on a shipped surface. | see "Eden's calls" | — |
| BUG-86 | 400-message refresh gives 400 announcements | `/chat` | KEEP | — | Screen-reader defect. | see "Eden's calls" | — |
| BUG-147 | Focused control at a scroll-area edge loses ~4 px of outline | shared (dialog, Chat Send) | KEEP | — | Focus-visibility defect; shared. | see "Eden's calls" | — |
| Chat LOWs (Avery) | Presentation LOWs; distinct Messages landmark names | `/chat` | KEEP | — | Chat survives; landmark names are a11y. | see "Eden's calls" | — |
| No-emoji follow-up (Jules) | D7 render-time stripping of agent-authored text | Chat, all agent text | RE-POINT | IA-16 / AC-IA16-1 | IA-16 defines it. | see "Eden's calls" | — |
| BUG-148 | WebKit: Option+Tab never shows focus in Offices and ProcessSheet sheets | `/offices`, `/processes` | KEEP (Offices half) | AC-IA03-2 | Keyboard defect; Offices survives unchanged. The ProcessSheet half is RETIRE (v1.0), as BUG-131. | see "Eden's calls" | — |
| BUG-166 | WebKit: pointer click on Retry drops focus to body | `/offices` | KEEP | AC-IA03-2 | Focus defect; may fold into BUG-158. | see "Eden's calls" | — |
| BUG-150 L1 | Every caller checks the chevron-alone line end | `/offices` e2e | KEEP | AC-IA03-2 | Test-only follow-up. | see "Eden's calls" | — |
| BUG-61 | Office host reported as legacy "This Mac" instead of the machine name | org-office-host | KEEP | IA-01 / AC-IA01-2 (footer office line) | The footer office line shows "{This Mac \| host}" (nav rev 5 decision 6). | see "Eden's calls" | — |
| Offices guidance (Avery, Jules) | Module-owned refresh coordinator; StatusPill fixture guard | `/offices` | KEEP | AC-IA03-2 | Offices code survives unchanged. | see "Eden's calls" | — |

## In flight (finish and merge, per ada)

- **Jules, Dialog B** (BUG-143/144 prerequisite): adds a `text` prop to DialogTitle, tokenised word spans and 22 caller migrations (200 lines or fewer). It is in progress at candidate `06a5f7ac`, with a test-only head `47d0b96f` (proof across 22 files and 23 sites). Its gate is queued after a test-only selector fix (7 to 8 specs). Next: the remaining phases once, a fresh Sol review, then the Tier B GO request (STATUS 16:16, Jules's backlog).
- **Jules, Dialog A** (BUG-143+144+142: DialogContent scrolling, footer overflow, and the Draft-or-improve outline): parked at candidate `963b8758` plus saved WIP. It stacks on B, the two merge back to back, and Eden validates once (STATUS 15:40). Eden has confirmed the cut A reading (Close stays put, p-6 at every width).
- **Avery, chart cut A** (BUG-114/115/117): committed `d64c5957` (68 non-test lines), and the Projects callback race is fixed. Avery's backlog reads "awaiting Ada GO/merge: checks and fresh Sol ZERO". Cut B (BUG-137/138/139) is queued behind it.
- S1a starts once all three have merged, because its folder moves would conflict with them (STATUS 16:16).

## What this means for sequencing

- **Land BUG-158 before S1b.** S1b's account menu and IA-19's New ▾ both sit on the shared DropdownMenu.
- **Don't start chart cut B** (BUG-137 to 139). D36 retires it, along with BUG-103 to 105, 107, 109, 111, 156, 157 and 159. Fold that effort into IA-02.
- **Drop the Processes polish for v1.0:** BUG-60, BUG-131 and the ProcessSheet half of BUG-148.
- **New e2e tests use the page helpers** (Rowan), because S1a's old-link scan fails any test that uses old paths.
- **Contract risk (Rowan, to ada in 01M7884P):** IA-10 (the Inbox API), IA-12 (the activity read and D35's last activity) and presence (#42) need read contracts that aren't frozen yet. Rowan proposes freezing IA-10 next.

## Eden's calls (01M788EA)

Eden's full rows are in her `knowledge/research/ia-v2-backlog-triage-design.md`: 51 rows, made before this file existed and so before D36 and D37.

**She agrees with:**
- KEEP: 146, 112 (re-measure Offices after IA-03), 118, 151 (shell-wide scroll-padding-top, with or after S1b), 158, 160 (pin the button spec so IA-09 reuses it), 161, 164, 166, chart cut A, the Employees dialogs (28, 29, 31-36) and the Chat and Offices polish.
- RE-POINT: 113 to IA-17; S1a, S1b and 119 to IA-01; 106, A-OC2, F2-02 A-8 and the S2 gutters to IA-02; 30 to IA-07; 55 to IA-09.
- RETIRE: 103, 104, 139, 159 and "Agents Home + Org-chart tab".

**Where she differs, and how I resolved it:**
- **BUG-107, 156, 157 (she says KEEP, for a11y on a shipped surface).** These stay RETIRE under D36: the chart search and the View menu go, and team delete moves to the Teams index, which carries 157's focus rule. The legend's safeguard applies: if IA-02 slips out of IA-M1, they come back as KEEP. She notes 156 may fall out of 158 anyway.
- **BUG-137, 138, 105, 108 (she says RE-POINT to IA-02).** These stay RETIRE: under D36, Fit, the View menu and the canvases don't exist in IA-02, so there's nothing to re-point. The outcome is the same either way: no separate fix, and IA-02 owns the result.
- **BUG-131 and 60 (she says "KEEP, defer").** These are RETIRE (v1.0). It's the same effect, since neither is done in v1.0, and both reopen when Processes ships.
- **BUG-37 (she says split it between IA-07 and IA-02).** I accept her split. The Set runtime and Offboard copy go to IA-07, and the `?member=` and roster parts go to IA-02.

**Design gates she owns:** the large-text sidebar rule before S1b freezes (BUG-119), the §13 Dialog amendment before BUG-164, and the D32 team-editing spec (157, A-8).

**Open items outside ada's list:** Eden lists more. BUG-6 (loses typed text) should go first, and BUG-163 (outline buttons are everywhere in IA v2) should be raised. Her file has the rest. She also notes that BUGS.md still shows done items as open: 62, 63, 65, 66, 71, 81, 82, 90-92, 94-99, 101 and 102.

## Open questions

None. D36 and D37 settle the two that came up while I drafted this list, and the differences with Eden are resolved above.
