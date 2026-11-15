# Proposed features and the project docs model

Status: proposal for the owner, 2026-11-12. Owner: Emery (Senior Product Manager).
Answers the owner's two asks: "Come up with a list of new things, features you think we should add" and "How are we surfacing project documentation in the project UI? ... Where is the PRD, how is it represented? ... What is the best thing to do?"

Labels: [F] a fact, with its source as file:line. [O] my opinion or recommendation. Product repo facts are on `origin/v1` of `/Users/vgmello/projects/agentic-consulting/app` at `4acb522a`.

Status vocabulary, used in both this file and the page:
- **Exists:** built on `origin/v1`.
- **Designed:** a spec or design on `origin/v1` covers it; not built.
- **Partly designed:** part of the proposal is in a spec or design on `origin/v1`; the rest is new.
- **Decided:** settled in one of my PRDs, with stories and a release; not built.
- **New:** in no spec, design or PRD yet.

## 0. Summary

- [O] **Git stays the source of truth for documents. The UI reads, renders and links them.** This is already the design rule: "persistent in the repository, transitional in the database" [F] `docs/design/tasks-projects-docs.md:40`.
- [O] **PRD model: one living Overview per project, one PRD per feature, and releases that point at features.** No giant project PRD, and no PRD per version.
- [O] **Once a project is live, its home leads with the Overview, not the work lists.** Projects get a lifecycle: Building → Live → Archived.
- [O] **Proposed project tabs: Overview · Work · Backlog · Releases · Docs · Team · Activity · Conversations.** Feature PRDs live in Docs, as a typed section with their status and progress. This is a proposed amendment to IA-17 and D39, not settled (§2.5).
- [F] Most of the docs machinery is designed but not built. The project page today has Overview, Tasks and Bugs; Features, Debt, Docs, Insights and Settings show as "available in a later release" `apps/org-ui/src/routes/(shell)/projects/[id]/+page.svelte:22,50,64`.
- [F] Releases were out of scope in the specs (`docs/specs/work-items-ux.md:619`). They are now decided in my planning PRD: keys, releases, milestones and the backlog, PL-1 to PL-7 in v1.0 (`projects-planning-v1.md:3, :215`). Eden designs the pages (`projects-planning-v1.md:230`).
- [O] What is still missing from the design is the **project lifecycle**, the **living Overview doc**, **release notes**, **comments on docs** and **two-way links between PRDs, tasks and releases**.

## 1. What already exists or is designed (research)

### 1.1 The docs model in the W7 design (`docs/design/tasks-projects-docs.md`)

- [F] Docs are Markdown with YAML front matter in the project repository under `docs/`, with templates in `docs/.templates/<kind>.md`. Every doc has a stable `id`; links are by id. The office keeps an index. Writes are commits on a branch, never the default branch (:18).
- [F] Features are repository docs, `docs/features/<n>-<slug>.md`. `feature_id` on a task is the database's source of truth for the task-to-feature link; the doc's `tasks` list is regenerated on each status commit. A feature becomes In progress and Delivered automatically, with Undo (:17).
- [F] Where each thing lives: feature definitions and other docs in the repository; work items, feature records and the docs index in `office.db`; the index is a rebuildable cache, never a source of truth (:25-40).
- [F] Feature record: `{ number (FEAT-n), path, title, status: proposed | planned | in_progress | delivered | retired, delivered_at?, retired_reason?, replaced_by? }` (:139).
- [F] The Docs tab reads the project's `integration_branch` (:145).
- [F] Reserved numbering: numbered kinds ADR, FEAT, DEBT, LIM, EXC, EVAL take the highest number in the index or in `doc_numbers`, plus one, reserved in the same transaction as the request (:449, :779). Numbered kinds use their number as id (`ADR-0004`) (:782); other kinds get `doc-<ULID>` (:988). Requests are `REQ-n`, org-wide, in the database (:11, decision 3).
- [F] Links: `fixes`, `part_of`, `from_request`, `reported_from`, `relates_to` (the "Why" link to an ADR, design or evaluation), `pays_back` (task to DEBT). There is no "blocks" link (:124-135).
- [F] The brief is `docs/brief.md`; it can't be deleted (`doc.delete` refuses "the brief", :797; :974 C10).
- [F] Merging docs branches is left to a merge flow not yet designed; until then a person merges (:776).
- [F] A project without a repository has no docs; the Docs tab says so (:319).

### 1.2 UX specs on `origin/v1` (`docs/specs/`)

| Spec | What it already designs [F] |
| --- | --- |
| `decisions-and-tech-debt-ux.md` rev 9 | The repo layout (`brief.md` pinned, `adr/`, `design/`, `evaluations/`, `specs/`, `plans/`, `reviews/`, `features/`, `retros/`, `processes/`, `debt/`, `limitations/`, `exceptions/`, `reports/`) (:60-80). The **Docs tab, grouped by kind, with Search docs** (:120-143). ADR lifecycle Proposed → Accepted / Rejected, proposed ADRs reach Needs you, Supersede (:235-240). "Why?" blocks on tasks and bugs (:242-248). |
| `features-ux.md` rev 2 | Request (transient) → Feature (persistent, in repo) → Tasks (:26-33). Statuses Proposed · Planned · In progress · Delivered · Retired (:37). A **Features tab, first after Overview** (:39), with a feature page showing tasks, bugs, cost, retros and history (:69-88). |
| `feature-requests-ux.md` rev 5 | Capture and routing of requests; "Scoped" means a Proposed feature the owner approves (:33-43). |
| `project-status-ux.md` rev 4 | The project's **Overview tab is the Status page** (:37); periodic status reports saved as `docs/reports/<date>-status.md` (:30). |
| `work-items-ux.md` rev 9 | Project tabs **Overview · Features · Tasks · Bugs · Debt · Docs · Insights · Settings** (:73, :483 PRJ-1). Bugs with triage. Milestones and releases out of scope (:619). |
| `retrospectives-ux.md` rev 4 | Retros on feature delivered or interval, evidence first, action items as changes, saved in `docs/retros/` (:28-39). |
| `costs-and-insights-ux.md` rev 5 | Project budgets with an 80% warning and a 100% Needs you item (:105-111). |
| `processes-ux.md` rev 6 | Autonomy levels per agent: Asks first and Full autonomy, stored as a level so more can be added (:144-152). |
| `agent-run-modes-ux.md` rev 9 | Run modes: Always on, On call, **Scheduled**, One-off (:40-50). |
| `employee-supervision-ux.md` rev 2 | Inspect an employee; recovery and memory (headings :92, :138). |

### 1.3 The UI on `origin/v1` (`apps/org-ui/src/routes`)

- [F] Pages: onboarding (create, welcome), agents, chat, offices, org-chart, processes, projects, `projects/[id]`, work, and a generic `[section]` page.
- [F] The project page has three live tabs (Overview, Tasks, Bugs) and five disabled ones (Features, Debt, Docs, Insights, Settings) (`projects/[id]/+page.svelte:22-23,50-54`). Bugs have a full sheet with triage, fix, verify and close (`lib/components/BugSheet.svelte:19,53`).

### 1.4 My own PRDs

- [F] IA v2 project page: "Overview · Work · Team · Activity · Conversations (plus Backlog and Releases after Work, once PL-5/PL-6 ship, D39)" (`information-architecture-v2.md:66`). D39 amends IA-17's tab list (:257). Search across projects, work, members (D16, :220). Activity filters (D24, :237). Every Needs-you source maps to one Inbox kind (D26, :244). Notes and memory read-only, sequenced (D6 :203, D18 :222). Project health set by hand in v1.0 (D30, :248). Following (D38, :256).
- [F] Planning v1 (`projects-planning-v1.md`), decided: keys `P-n`, `R-n`, `M-n`, `T-n`, `B-n` (:61-67); releases with an optional target that reads "Target passed" in neutral grey once passed, never an alarm (:99, AC-PL2-2 :184); "Due-date alarms" is a non-goal (:48); PL-1 to PL-7 in v1.0 (:215); PL-9 feature targeting is Next (:210, :216).
- [F] Tasks v1: In review as a status with a reviewer on the same task (T4, `tasks-v1.md:168`); every change is a `WorkEvent` with actor and server time (T8, :247); "Done is the assignee's claim" (:263); per-owner "last seen" per task (:327).
- [F] ada's STATUS tracks the v1.0 plan (SRV-1..SRV-32, dogfood run, then Wave A) and has no item for project docs UI or a digest (`ac-staff-engineer-ada/knowledge/tracking/STATUS.md:10`).

## 2. The answer on docs and PRDs

### 2.1 Where documents live

- [O] **Git is the source of truth.** Agents write docs through git, on branches, landing through the merge flow. This is already decided [F] `tasks-projects-docs.md:18`.
- [O] **The UI reads and renders them** from the integration branch, and every doc shows its path, its last commit and a link to the file. Pending changes show as "Waiting to merge" [F] the overlay at `tasks-projects-docs.md:779`.
- [O] **The owner acts in the UI:** comment on a doc, approve or reject a proposed one, and ask for changes. Approval writes the status and who decided in a commit, as ADRs already do [F] `decisions-and-tech-debt-ux.md:237`. Comments on docs are new behaviour. Where they are stored is engineering's call.

### 2.2 One typed library

[O] The project's **Docs** tab is one typed library using the design's kinds and ids:

| Type | Id or path | How many | Lifetime |
| --- | --- | --- | --- |
| **Overview** | `docs/brief.md` (see 2.6) | one per project | living: what it is, who it's for, how it works now, current version |
| **Feature PRD** | `FEAT-n` | one per feature | proposed → planned → in progress → delivered → retired |
| **Decision** | `ADR-n` | one per decision | proposed → accepted / rejected → superseded |
| **Spec** (UX or technical design) | `doc-<ULID>` | as needed | living, versioned in git |
| **Runbook / how-to** | `doc-<ULID>` | as needed | living |
| **Release notes** | `docs/releases/R-n.md`; its doc id follows the design (`tasks-projects-docs.md:782`); Rowan decides | one per release (`R-n`) | drafted at release, then frozen |
| Also already designed | `EVAL-n`, `DEBT-n`, `LIM-n`, `EXC-n`, retros, status reports | | as their specs say |

- [O] **Features live in Docs**, as a typed section that shows each feature PRD with its status and progress. There is no separate Features tab. This departs from the designed Features tab (`features-ux.md:39`) and is part of the amendment in §2.5.

### 2.3 The PRD model

[O] **Not one giant project PRD. Not a PRD per version. One living Overview per project, plus one PRD per feature.**

- **A release (`R-n`) lists the features it delivers.** The release page shows those PRDs with their status and progress.
- **Release notes are drafted at release time** from the delivered features and done tasks, approved by the owner, then frozen in `docs/releases/R-n.md`.
- **When a feature ships, its PRD becomes the as-built record:** status Delivered, linked to its release.
- **Changing a delivered feature** is a new feature PRD ("Extends FEAT-7") or a revision with history. Never a silent rewrite. The designed request flow already supports "Adds to FEAT-0003" [F] `features-ux.md:35`.

[O] **Why:**
- A version is a slice of features. A PRD per version duplicates the feature text and goes stale the day after release.
- A single project PRD grows without bound, mixes shipped and planned, and nobody can tell what is true now.
- A per-feature doc keeps its own history in git, and answers "why does it work this way?" long after go-live.
- The Overview is the one page a newcomer, the owner or a new agent reads first. It stays short because the detail lives in the feature PRDs it links.

### 2.4 Links both ways, and the human loop

- [O] A task shows its feature and the PRD. A PRD shows its tasks, progress and releases. A release shows its features and its notes.
- [O] Doc status changes and approvals appear in **Activity** (the Decisions filter, D24 [F] `information-architecture-v2.md:237`).
- [O] A PRD or release notes waiting for the owner's approval is an **Inbox** Approval, as proposed ADRs and scoped requests already are [F] `decisions-and-tech-debt-ux.md:238`, D26 `information-architecture-v2.md:244`.
- [O] **Search covers docs**: titles, ids and body text, grouped with Projects, Work and Members (extends D16 [F] `information-architecture-v2.md:220`; the Docs tab already has Search docs [F] `decisions-and-tech-debt-ux.md:123`).

### 2.5 The project page: tabs and lifecycle

[O] **Proposed tabs: Overview · Work · Backlog · Releases · Docs · Team · Activity · Conversations**, with Insights and Settings later.

- [F] Today's IA reads "Overview · Work · Team · Activity · Conversations (plus Backlog and Releases after Work, once PL-5/PL-6 ship, D39)" (`information-architecture-v2.md:66`; IA-17 at `ia-v2-stories.md:152-156`).
- [O] The change is one new tab, **Docs**, after Releases. Features are not a separate tab: feature PRDs live in Docs as a typed section with their status and progress.
- [O] **This is a proposed amendment to IA-17 and D39, not settled.** I'll record it as a new D-row in `information-architecture-v2.md` once the owner agrees.

[O] Projects get a lifecycle: **Building → Live (→ Archived)**. The app suggests Live when the first release ships; the owner confirms (§4).

| | Building | Live |
| --- | --- | --- |
| Overview tab leads with | progress: features in flight, Needs you, flow (the Status page) | **the Overview doc**, current version and release notes, recent changes, open bugs; Status tiles below |
| Backlog and Releases | the main planning view | still tabs; the current release shows on Overview |
| Docs | a tab | a tab, and the Overview doc on the Overview tab |

### 2.6 Where the research changes or sharpens my decisions

1. **[F] "Overview" is already two things in the design.** The project's Overview **tab** is the Status page (`project-status-ux.md:37`), and the project already has a pinned living doc, `docs/brief.md`, that can't be deleted (`tasks-projects-docs.md:797`, `:974` C10; `decisions-and-tech-debt-ux.md:64`). **[O] Adjustment:** the living Overview doc *is* `docs/brief.md`, shown as "Overview" in the Docs library, with its template extended (what it is, who for, how it works now, current version). No second doc. The Overview tab shows the brief at the top in Live and the Status tiles below it.
2. **[F] A feature record has one `delivered_at`** (`tasks-projects-docs.md:139`), and a Delivered feature goes back to In progress when a task is reopened or linked (`features-ux.md:37`). Feature targeting is PL-9, Next: "A feature can be targeted to a release; the release page lists it" (`projects-planning-v1.md:210`). **[O] Proposed amendment to PL-9:** a feature shows every release it shipped in, so an extended feature keeps "shipped in R-1" after it ships again in R-3. Prefer a new extending feature for any change bigger than a fix. Rowan decides how it is stored.
3. **[F] Releases were out of scope in the specs** (`work-items-ux.md:619`). **They are now decided** in my planning PRD, PL-1 to PL-7 in v1.0 (`projects-planning-v1.md:215`). Rowan designs the records; Eden designs the pages (`projects-planning-v1.md:229-230`).
4. **[F] Comments exist on tasks; none are designed on docs.** Doc comments are new.

None of these contradict a decision. They change where the Overview lives, add a Docs tab in place of a Features tab, and amend PL-9.

## 3. Feature proposals

Status uses the vocabulary at the top: **Exists**, **Designed**, **Partly designed**, **Decided**, **New**.

### Now (next after v1.0)

| # | Feature | Owner's problem | Proposal | Why it matters for long-lived agent teams | Status |
| --- | --- | --- | --- | --- | --- |
| N1 | **Project planning: backlog, releases, milestones** | "What ships next, and when?" has no answer in the app. | Already scheduled for v1.0. Keys (`P-n`, `R-n`, `M-n`, `T-n`, `B-n`), releases with an optional target, milestones under a release, a ranked backlog view, and the plan verbs in the CLI. Listed first because the docs model builds on releases. | Agents work fast and in parallel. Without a release shape, work is a pile of done tasks with no story. | Decided: PRD `projects-planning-v1.md`, PL-1 to PL-7 in v1.0 |
| N2 | **Project docs and PRDs in the UI** | "Where is the documentation? Where is the PRD?" | Section 2: Docs tab with features inside it, typed library, Overview as the brief, one PRD per feature, release notes, two-way links, doc comments, search. | Docs are the memory that outlives every agent session and every offboarding. | Partly designed: Docs tab, features, ADRs designed; Overview template, release notes, doc comments and links to releases New |
| N3 | **Delivery pipeline on the task** | "Is this task really shipped, or just marked Done?" | A strip on each task: branch → review verdict → merge → validation, from git and the review records. Done means merged and validated, or says what is missing. | "Done is the assignee's claim" (`tasks-v1.md:263`); the pipeline makes it checkable without reading logs. | New (merge flow left for later, `tasks-projects-docs.md:776`; In review decided, T4 `tasks-v1.md:168`) |
| N4 | **"While you were away" digest** | After a day away, the owner can't tell what changed. | On return, Home shows one card: what shipped, decisions made, new bugs, what waits on you, drafted from events, with links. Also a daily or weekly summary, scoped to the org or a team (absorbs L5). | Agents keep working when the owner is away. The digest is how the owner catches up in two minutes. | New (builds on "last seen", `tasks-v1.md:327`, and Following, D38) |
| N5 | **Decision log, org-wide, linked to work** | "Why did we do this, and where was it decided?" | One Decisions view across projects and the handbook: ADRs with status, owner, date, the work they led to. Decisions made in chat can be promoted to an ADR in one step. | Agents make many small calls. Without a log, the same question is re-decided, and the owner can't audit. | Partly designed: per project (`decisions-and-tech-debt-ux.md:120-248`); org-wide view and promote-from-chat New |
| N6 | **Retros in the product** | Lessons from one delivery don't reach the next. | Build the designed retro: triggered by feature delivered or interval, evidence first, action items as changes applied to guidance, processes and templates. | It is how a team of agents gets better over months instead of repeating mistakes. | Designed (`retrospectives-ux.md`); requirements in progress |

Dropped from Now: **Bug catalog and triage in the UI** already exists. The project Bugs tab and the bug sheet with triage, fix, verify and close are on `origin/v1` [F] `projects/[id]/+page.svelte:59`, `lib/components/BugSheet.svelte:19,53`. An org-wide bug view is covered by the IA v2 Work page (Tasks · Bugs · Requests) [F] `information-architecture-v2.md:67`.

### Next

| # | Feature | Owner's problem | Proposal | Why it matters | Status |
| --- | --- | --- | --- | --- | --- |
| X1 | **Project lifecycle: Building → Live** | A live project's page is a list of closed tasks. | Section 2.5: a lifecycle field, and a Live Overview tab that leads with the Overview doc, current version, recent changes and open bugs. | Most of a product's life is after launch. The page should match that. | New |
| X2 | **Autonomy and approval policies** | "What may this agent do without asking me?" is two levels today. | Extend the designed levels with per-action rules for an agent or team: merge, spend over an amount, hire, change a process, contact outside. Each rule is Ask, Notify or Free. | Trust is granted in steps. Coarse levels force all or nothing. | Partly designed: two levels (`processes-ux.md:144-152`); per-action rules New |
| X3 | **Agent track record** | "Can I trust this agent with more?" | On the profile: tasks done, reopened, review rounds and findings, time in each status, cost per task, over time. Facts only, with links. | It is the evidence for X2 and for coaching. Retros already gather the same facts (`retrospectives-ux.md:33`). | New (figures derivable from events, T8 `tasks-v1.md:247`) |
| X4 | **Cost and token budgets with alerts** | Spend can grow unseen. | Build the designed project budgets; add per-agent budgets and a daily spike alert. | Long-running teams spend continuously; a budget is the owner's brake. | Partly designed: project budgets (`costs-and-insights-ux.md:105-111`); per agent New |
| X5 | **Task dependencies and the release critical path** | "What is blocking the release?" | A `blocks` link between items, shown on both. The release page shows its critical path and what blocks it. A passed target shows the neutral "Target passed", with no alarm, as the planning PRD sets (non-goal "Due-date alarms", `projects-planning-v1.md:48`; AC-PL2-2 :184). | Parallel agents step on each other without explicit order. | New (no `blocks` link, `tasks-projects-docs.md:124-135`) |
| X6 | **Recurring and scheduled work** | Weekly checks and reports are retyped each time. | A task template with a schedule creates the task and assigns it. Uses the designed Scheduled run mode to wake the agent. | Operations work after go-live is mostly recurring. | Partly designed: Scheduled run mode (`agent-run-modes-ux.md:48`); recurring tasks New |
| X7 | **Agent memory viewer and correction** | An agent acts on something wrong it "remembers". | Build the read-only tab, then let the owner flag or correct an entry, which the agent applies and records. | Memory drives behavior. Seeing and correcting it is supervision. | Partly designed: read-only decided (D6 `information-architecture-v2.md:203`, D18 :222); correction New |

### Later

| # | Feature | Owner's problem | Proposal | Why it matters | Status |
| --- | --- | --- | --- | --- | --- |
| L1 | **Project goals tied to releases** | Work isn't tied to outcomes. | A few goals per project with a measure; releases and features name the goal they serve; status reports report on them. | Keeps an autonomous team pointed at outcomes, not output. | New |
| L2 | **Project and task templates** | Every new project is set up by hand. | Save a project (tabs, docs set, processes, team) or a task with steps as a template. | Faster, consistent starts. | Partly designed: doc templates (`tasks-projects-docs.md:18`); project and task templates New |
| L3 | **Mobile push for Inbox approvals** | Approvals wait while the owner is away from the desk. | Push an Inbox Approval to the phone with Approve and Deny. | Agents wait less on the owner. | New (needs a push channel; owner sign-in on the gateway) |
| L4 | **Org knowledge base** | Conventions are scattered across files. | One browsable view of the handbook: processes, guidance, rules, with which agents use each. | The same rules reach the owner and every agent. | Partly designed: handbook repo (`docs/design/processes.md:20`); the browsable view New |
| L5 | **Team standups, summarised** | The owner can't attend agent standups. | A daily per-team summary: done, next, blocked, with links, from events and agents' notes. Ships as the team scope of N4 (§4). | A rhythm for teams that never sleep. | New |

[O] My additions are folded in: doc comments and release history (in N2), promote-from-chat (in N5) and per-agent spike alerts (in X4). I added no new rows.

## 4. Decisions I made (tell me if you disagree)

1. **Release notes are approved by the owner before they freeze.** They reach the Inbox as an Approval, as proposed ADRs do (`decisions-and-tech-debt-ux.md:238`).
2. **Live is suggested, the owner confirms.** The app suggests Live when the first release ships; nothing switches on its own.
3. **N4 and L5 ship as one feature:** one digest, scoped to the org or a team.

## Sources

- `origin/v1:docs/design/tasks-projects-docs.md` (:11, :17-18, :25-40, :124-139, :145, :319, :449, :776-782, :797, :974, :988)
- `origin/v1:docs/specs/decisions-and-tech-debt-ux.md` (:60-80, :120-143, :235-248)
- `origin/v1:docs/specs/features-ux.md` (:26-39, :69-88)
- `origin/v1:docs/specs/feature-requests-ux.md` (:33-43)
- `origin/v1:docs/specs/project-status-ux.md` (:30, :37)
- `origin/v1:docs/specs/work-items-ux.md` rev 9 (:73, :483, :619)
- `origin/v1:docs/specs/retrospectives-ux.md` (:28-39)
- `origin/v1:docs/specs/costs-and-insights-ux.md` (:105-111)
- `origin/v1:docs/specs/processes-ux.md` (:144-152)
- `origin/v1:docs/specs/agent-run-modes-ux.md` (:40-50)
- `origin/v1:docs/design/processes.md` (:20)
- `origin/v1:apps/org-ui/src/routes/(shell)/projects/[id]/+page.svelte` (:22-23, :50-64)
- `origin/v1:apps/org-ui/src/lib/components/BugSheet.svelte` (:19, :53)
- `knowledge/prds/projects-planning-v1.md` (:3, :48, :61-67, :99, :184, :210, :215-216, :229-230)
- `knowledge/prds/information-architecture-v2.md` (:66-67, D6 :203, D16 :220, D18 :222, D24 :237, D26 :244, D30 :248, D38 :256, D39 :257)
- `knowledge/prds/ia-v2-stories.md` (IA-17 :152-156)
- `knowledge/prds/tasks-v1.md` (T4 :168, T8 :247, :263, :327)
- `projects/ac/agents/ac-staff-engineer-ada/knowledge/tracking/STATUS.md` (:10)
