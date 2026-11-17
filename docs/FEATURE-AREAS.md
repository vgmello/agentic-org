# Feature areas

These are the categories used by the [single feature checklist](FEATURES.md). Each area below documents what it covers, where its boundary lies, which requirements define it and what implementation evidence needs checking. This preserves the area-by-area explanation alongside the repository requirements.

Reviewed: **2026-11-16**, by Ada. There are **13 categories, 158 feature rows and 0 rows needing verification**. Counts reflect the current checklist after the [first-five](verification/2026-11-16-first-five-features.md) and [remaining-58](verification/2026-11-16-remaining-feature-audit.md) source/test audits. Zero unknown states does not mean zero remaining work or complete test acceptance. Feature delivery statuses and evidence live only in FEATURES.md; the table below is their summary. A category has no independent completion state.

F1a source update: FC-011 is partial because the pure complete-release verifier is present. This internal foundation does not provide installation, onboarding or an authentic signed release; full-row acceptance remains open. Counts below include that scoped update. F1b-socket adds internal OS adapter evidence to already-partial FC-017; no status/category count changes or production handoff claim. Accepted F2a adds separately scoped lossless source/codec evidence to already-partial FC-042; no count change or durable/usable history claim. F2b now adds scoped journal/capture source and storage tests, including recovery of interrupted directory creation and ancestor-flush failure fencing: FC-146 moves Planned to In progress, while FC-042 remains partial. F2b was accepted/integrated at `d55e3c25`/`eacb505d` with current checks and author/fresh reviews; protected host/service composition and usable feature acceptance remain. F3a adds the internal REC-ESC-1 crypto adapter and five targeted tests: FC-016 moves Planned to In progress. F3a was accepted/integrated at `065a0824`/`0d4035d7` after full checks and author/fresh reviews. F3b worker HQ persistence/admission was accepted/integrated at `4abca318`/`55c7d762` after full checks (3074 Rust tests, zero failures), original-author validation and fresh initial/separate final reviews. Root source update, 2026-11-17: F3c checkpoint `1ce54471` adds unaccepted native protected bootstrap/account/session/host-recovery composition and actual temporary-root native runtime evidence. FC-012 and FC-014 move to In progress; neither is a usable browser or installed-service delivery. Current full checks, independent reviews, browser/physical-authentication and release gates remain; full-row scope stays partial. F3c was accepted/integrated at `e5152b4e`/`67860707`. F4 (GW-01, unaccepted until its own review) adds the host-supervised browser gateway and session routes: FC-013 moves Blocked to In progress (domain APIs and the production launcher remain for U1); FC-014 gains gateway session evidence and stays partial.

## Categorization rules

- Every FC- row has exactly one primary area, determined by the capability it delivers, regardless of whether it is Done, Planned, Deferred or another state.
- Preserve FC- identifiers when moving rows. Cross-area dependencies use links; do not count the same feature twice.
- Keep access/download/CSP features with installation and office operations. Keep deferred planning/tracking/subtask workflows with project work. These replace the former add-on groups, which mixed domain and status.
- Shared shell, cost, identity and recovery dependencies do not move a domain feature out of its primary area. Code ownership still follows [AGENTS.md](../AGENTS.md).
- These are **documentation categories**, not an implementation of the deferred product “Areas” feature. Existing requirements and selection gates are unchanged.

## Area summary

The “Needs verification” column identifies gaps in the implementation audit, not confirmed missing features. Definitions for all states are in the [checklist status legend](FEATURES.md#reading-the-status). Totals count differently sized capabilities; they are not a completion percentage.

| Area | Scope | Total | Done | In progress | Planned | Blocked | Deferred | Needs verification |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| [AREA-01 · Application and browser](#area-01) | Shell, navigation, accessibility, search and preferences | 9 | 2 | 3 | 3 | 0 | 1 | 0 |
| [AREA-02 · Installation, access and office operations](#area-02) | Install, accounts/security, services, satellites and office lifecycle | 18 | 1 | 13 | 3 | 1 | 0 | 0 |
| [AREA-03 · Organization, people and teams](#area-03) | Membership, reporting, hiring, profiles, Home and activity | 13 | 1 | 11 | 1 | 0 | 0 | 0 |
| [AREA-04 · Projects, tasks, bugs and requests](#area-04) | Typed work, assignment/history, collaboration and planning gates | 23 | 0 | 16 | 2 | 0 | 5 | 0 |
| [AREA-05 · Feature definitions, knowledge and delivery](#area-05) | Feature/knowledge authoring and release/deployment records | 14 | 0 | 1 | 12 | 0 | 1 | 0 |
| [AREA-06 · Chat and agent messaging](#area-06) | Conversations, asks, handoffs and transport cutover | 12 | 2 | 6 | 3 | 0 | 1 | 0 |
| [AREA-07 · Usage, costs and budgets](#area-07) | Usage, attribution, pricing, cost reads and budget controls | 11 | 0 | 0 | 10 | 0 | 1 | 0 |
| [AREA-08 · Execution, providers and supervision](#area-08) | Runtimes, routing, supervision and workspace isolation | 13 | 1 | 2 | 6 | 0 | 4 | 0 |
| [AREA-09 · Roles, processes and retrospectives](#area-09) | Role/guidance content and deferred learning engines | 9 | 0 | 3 | 0 | 0 | 6 | 0 |
| [AREA-10 · Project health and reports](#area-10) | Health, narratives, analytics and private digests | 5 | 0 | 0 | 0 | 0 | 5 | 0 |
| [AREA-11 · Copilot drafting](#area-11) | Reviewed drafts and suggestion candidates | 17 | 1 | 2 | 0 | 0 | 14 | 0 |
| [AREA-12 · Git, identity, synchronization and recovery](#area-12) | Repositories, canonical records, numbering, sync and restore | 11 | 1 | 4 | 5 | 0 | 1 | 0 |
| [AREA-13 · Session inspection and private memory](#area-13) | Session reads, human takeover and private profile memory | 3 | 0 | 0 | 2 | 0 | 1 | 0 |
| **Total** | | **158** | **9** | **61** | **47** | **1** | **40** | **0** |

## AREA-01

**Application and browser** · [Feature checklist](FEATURES.md#application-and-browser)

**Scope:** Shared browser navigation, responsive layout, accessibility, visual interactions, search/creation entry points, preferences and shell foundations.

**Boundary:** Shared presentation belongs here; domain operations belong to the domain area. Account authorization and browser service composition belong to AREA-02; usage calculations belong to AREA-07.

**Verification focus:** The [first-five audit](verification/2026-11-16-first-five-features.md) resolved all three unknown rows here: shared controls are In progress with concrete UX gaps; Search/New and keyboard preferences are Planned. Follow the recorded gaps before claiming completion. Shell shortcut or layout tests alone do not establish those capabilities.

**Requirements:** [Shell UX](specs/app-shell-ux.md), [Executable migration](design/executable-migration.md).

## AREA-02

**Installation, access and office operations** · [Feature checklist](FEATURES.md#installation-access-and-office-operations)

**Scope:** Standalone installation, named instances, HQ bootstrap, production browser service, account access/grants/recovery, satellites, office lifecycle, verified downloads and release hardening.

**Boundary:** Owns service/access/enrollment behavior. Team membership belongs to AREA-03; durable source restoration and process-safe cache rebuild belong to AREA-12. CSP is classified here with production security, even though it affects every screen.

**Verification focus:** The [remaining-58 audit](verification/2026-11-16-remaining-feature-audit.md) resolved the other three unknown rows here: restart, instance and logging primitives are partial; authorized relocation/removal, update/uninstall and usable diagnostics remain. The [first-five audit](verification/2026-11-16-first-five-features.md) established partial TLS/custody and satellite status/repair UI, with production handoff, CA escrow and invitation/approval integration still missing. Native account/bootstrap composition is now In progress with temporary-root executable evidence; it remains unaccepted. User-service registration, real browser onboarding, physical-authentication and authentic signed-release acceptance remain open.

**Requirements:** [Installation/access UX](specs/server-installation-and-access-ux.md), [Office/team UX](specs/offices-and-teams-ux.md), [Host boundary](design/office-host-boundary.md), [Dogfood acceptance](specs/dogfood-run.md).

## AREA-03

**Organization, people and teams** · [Feature checklist](FEATURES.md#organization-people-and-teams)

**Scope:** Organization structure, people, home teams, lead/reporting relationships, project accountability, HR/hiring, employee lifecycle, profiles, Home/Inbox and activity/presence.

**Boundary:** People and teams are organizational identity; offices are execution locations. Execution routing/holds belong to AREA-08, and account security grants belong to AREA-02. A chart position never proves authority.

**Verification focus:** Check home-team/lead enforcement, reporting and owner-linked identity, project ownership, valid HR bootstrap, actual hire approval/provisioning and runtime effects of employee controls. Verify Inbox clearing and fresh, real activity sources.

**Requirements:** [Organization UX](specs/offices-and-teams-ux.md), [Employee/Home UX](specs/our-org-in-the-app.md), [Organization contract](design/organization-data/01-organization-structure.md), [Employee contract](design/organization-data/04-agent-structure.md).

## AREA-04

The [unaccepted assignment branch checkpoint](verification/2026-11-16-assignment-branch-checkpoint.md) adds scoped FC-041/FC-042 implementation/test evidence beyond the imported origin/v1 audit baseline. Their full scope remains In progress; category/status totals are unchanged.

**Projects, tasks, bugs and requests** · [Feature checklist](FEATURES.md#projects-tasks-bugs-and-requests)

**Scope:** Project work views, typed tasks/bugs/requests, assignment and accountability, lifecycle/history, checklists, output, time, shared collaboration, author-private task notes and explicitly deferred planning/tracking workflows.

**Boundary:** Owns work behavior and task-private notes. Feature definitions and delivery records belong to AREA-05; chat transport belongs to AREA-06; profile memory belongs to AREA-13. Plain checklist steps do not select an execution engine.

**Verification focus:** Check unified task identities and old-reference mappings; assignment, reporter/history and transitions; bug triage/fix verification; request routing/outcomes; persisted checklists/output/time; comments/questions and needs-me visibility. Following/requested-by, backlog planning and subtask cascades remain Deferred.

**Requirements:** [Work UX](specs/work-items-ux.md), [Request UX](specs/feature-requests-ux.md), [Work design](design/tasks-projects-docs.md), [Assignment/history](design/organization-data/12-task-assignment-and-history.md), [Private task notes](design/organization-data/13-private-task-notes.md).

## AREA-05

**Feature definitions, knowledge and delivery** · [Feature checklist](FEATURES.md#feature-definitions-knowledge-and-delivery)

**Scope:** Canonical feature definitions/lifecycle and linked work; scoped knowledge, briefs, ADRs/debt/limitations, document CLI/publication; release/milestone manifests, changelogs, deployment and flag records.

**Boundary:** Owns definitions and records, not deployment execution or flag evaluation. Task execution belongs to AREA-04; richer health/report generation belongs to AREA-10; Git transport/storage mechanics belong to AREA-12.

**Verification focus:** Check real feature storage/editing and task membership; document reader/editor, kinds/templates/search and publication/provenance; release content/changelogs, deployment outcomes/rollback links and intended versus applied flag records. Authored records do not establish automation.

**Requirements:** [Features UX](specs/features-ux.md), [Knowledge UX](specs/decisions-and-tech-debt-ux.md), [Release content](design/organization-data/09-release-content-and-flags.md), [Deployments](design/organization-data/07-release-deployments.md).

## AREA-06

**Chat and agent messaging** · [Feature checklist](FEATURES.md#chat-and-agent-messaging)

**Scope:** Agent CLI messaging, local mailbox delivery, conversations/channels/group DMs, threads, asks, attachments/reactions, handoffs, history, delivery and the protected-source transport cutover.

**Boundary:** Owns conversation behavior and delivery. Assignment semantics belong to AREA-04; execution wake/stop primitives belong to AREA-08; durable cross-office restoration belongs to AREA-12. Shared Git is never chat storage.

**Verification focus:** Check visibility-safe mentions/search/attachments/reactions, drafts/history/pagination and performance, ask resolution/withdrawal, atomic task handoff and actual wake/pause/noninterrupting delivery. Phase A ask/reply is narrower than the selected production conversation contract.

**Requirements:** [Chat UX](specs/chat-ux.md), [Messaging contract](design/organization-data/15-messaging-and-chat.md), [Messaging ADR](adr/0003-agent-messaging-office-first.md).

## AREA-07

**Usage, costs and budgets** · [Feature checklist](FEATURES.md#usage-costs-and-budgets)

**Scope:** Durable usage/corrections, immutable historical attribution, model prices/tokens/USD and coverage, authorized cost views/export, period calculations and selected budget controls.

**Boundary:** Owns monetary/accounting and budget policy semantics. Execution enforcement and verified cleanup require AREA-08; protected archive restoration requires AREA-12. Release caps warn only, and detailed analytics remain Deferred.

**Verification focus:** Check the real usage collector/archive and corrections, immutable consumer/team/task/release snapshots, pricing and unknown/partial coverage, authorized live reads/CSV and account-zone period/bucket math. Budget service gaps remain separately marked Planned.

**Requirements:** [Costs UX](specs/costs-and-insights-ux.md), [Accounting contract](design/organization-data/10-usage-and-cost-accounting.md), [Budget contract](design/organization-data/17-budgets-and-controls.md).

## AREA-08

**Execution, providers and supervision** · [Feature checklist](FEATURES.md#execution-providers-and-supervision)

**Scope:** Native process primitives and runtime adapters, explicit Run, execution/model configuration, connections/pools/permits, turns/attempts/placement, health monitoring, workspace isolation and deferred scheduling/delegation.

**Boundary:** Owns running work safely. Employee identity belongs to AREA-03, budget semantics to AREA-07, human session inspection/takeover to AREA-13, and durable recovery evidence to AREA-12. Automatic scheduling and delegate engines remain Deferred.

**Verification focus:** Check each Claude/Codex/Agy adapter for real launch, streaming, cancellation and sandbox enforcement; check observed progress/health/waits; validate credential and other-task denies in the selected workspace topology. Generic process primitives do not prove full adapters.

**Requirements:** [Run-mode UX](specs/agent-run-modes-ux.md), [Supervision UX](specs/employee-supervision-ux.md), [Session design](design/agent-sessions.md), [Execution contract](design/organization-data/11-model-profiles-and-agent-execution.md), [Providers/pools](design/organization-data/14-model-providers-and-connection-pools.md).

## AREA-09

**Roles, processes and retrospectives** · [Feature checklist](FEATURES.md#roles-processes-and-retrospectives)

**Scope:** Role identity/assignment, scoped process and standard content, process editor UI and the separately deferred inheritance/autonomy/retrospective engines.

**Boundary:** Owns guidance content and proposed learning workflows. Execution configuration belongs to AREA-08; general document tooling belongs to AREA-05. Structural records can be selected while their resolver/execution engine remains Deferred.

**Verification focus:** Check actual role record/assignment persistence and organization/team/project process or standard document storage. Existing content and editor UI do not establish runtime composition, inheritance, agent injection or retrospective execution.

**Requirements:** [Roles UX](specs/roles-ux.md), [Process UX](specs/processes-ux.md), [Process design](design/processes.md), [Retrospective UX](specs/retrospectives-ux.md).

## AREA-10

**Project health and reports** · [Feature checklist](FEATURES.md#project-health-and-reports)

**Scope:** Richer project health snapshots, narrative/status reports, progress/throughput/cycle-time analytics and private away digests.

**Boundary:** Owns derived reporting experiences, not ordinary project work views (AREA-04), report-document storage (AREA-05), accounting facts (AREA-07) or generic copilot drafting (AREA-11). Private digests remain account-only.

**Verification focus:** No rows currently carry Needs verification: these capabilities remain Deferred. Before implementation, resolve their selection gates and metric/context contracts; do not reinterpret ordinary project overview or stored report records as delivery of the richer experience.

**Requirements:** [Project status UX](specs/project-status-ux.md), [Metric definitions](specs/costs-and-insights-ux.md).

## AREA-11

**Copilot drafting** · [Feature checklist](FEATURES.md#copilot-drafting)

**Scope:** The implemented registry/rendering/output-validation library, partial text/bug drafting integration, and individually deferred suggestion/drafting candidates.

**Boundary:** Owns draft values and suggestions, never domain writes. Final task, document, message and hire actions belong to their domain areas. Runtime integration belongs to AREA-08, and actual usage accounting belongs to AREA-07.

**Verification focus:** No rows currently carry Needs verification. Keep library completion separate from native runtime/authorized context integration. Each deferred candidate requires its stated selection and context contract; verify reviewed confirmation before any ordinary domain save/send/hire.

**Requirements:** [Smart-feature design and candidates](design/smart-features.md).

## AREA-12

**Git, identity, synchronization and recovery** · [Feature checklist](FEATURES.md#git-identity-synchronization-and-recovery)

**Scope:** Registered repositories/GitHub/worktrees, canonical source/projection contracts, stable identity/number allocation and imports, sync primitives and protected-source office continuity/recovery.

**Boundary:** Owns common durability/identity/transport mechanics. Domain payload and access rules remain in the relevant area. Organization Git alone cannot restore chat, private notes, accounting, budgets, credentials or execution authority.

**Verification focus:** Check repository/project branch associations and supported GitHub/worktree operations; canonical round-trip/concurrency/tombstones; UUIDv7 and durable scoped allocations; preserved legacy IDs, provenance and unresolved import staging. A draft schema or core sync library is insufficient.

**Requirements:** [Git design](design/projects-git-github.md), [Identity contract](design/organization-data/06-identity-and-numbering.md), [Record contract](design/organization-data/08-record-contracts.md), [Recovery contract](design/organization-data/16-recovery-and-office-continuity.md).

## AREA-13

**Session inspection and private memory** · [Feature checklist](FEATURES.md#session-inspection-and-private-memory)

**Scope:** Authorized browser session/transcript inspection, office-local CLI takeover/PTY handback and the separately deferred human-facing private notes/memory reader.

**Boundary:** Owns human inspection/control of a session and profile memory access. Process execution belongs to AREA-08; author-private task notes belong to AREA-04. A profile placeholder or runtime transcript does not establish a private memory store.

**Verification focus:** Check actual HQ-authorized session/event/transcript reads, privacy and freshness. CLI takeover is Planned pending PTY/lease/handback integration; the private memory reader remains Deferred with confined paths, caps, no-symlink checks and its separate human-only API.

**Requirements:** [Employee/profile UX](specs/our-org-in-the-app.md), [Session design](design/agent-sessions.md), [Memory/supervision gates](specs/employee-supervision-ux.md).

## Maintaining the area guide

The [remaining-58 audit](verification/2026-11-16-remaining-feature-audit.md) now classifies every formerly unknown row. Verification focus paragraphs describe checks still required for **completion**, not rows still awaiting their initial status audit. See the record for scoped evidence, lead adjudications and the unresolved source-guard failure.

FEATURES.md remains the single row-level status/evidence record. Update this guide's summary when rows change status or category; update scope/boundaries when a category changes. Add a category only for a distinct capability domain, not to group backlog states. Keep AREA- identifiers stable, and update checklist/category navigation together.

For a Needs verification row, compare the owning requirements with current code and real persistence, authorization and runtime evidence. Record the implementation/verification links and remaining gap in that checklist row before choosing Done, In progress, Planned or Blocked. A requirement, historical merged task, test-file name or fake-backed screen alone does not establish complete delivery. Deferred requirements retain their decision gates.
