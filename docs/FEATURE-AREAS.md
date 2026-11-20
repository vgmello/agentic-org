# Feature areas

These are the categories used by the [single feature checklist](FEATURES.md). Each area below documents what it covers, where its boundary lies, which requirements define it and what implementation evidence needs checking. 

There are **13 categories, 189 feature rows and 0 rows needing verification**. Zero unknown states does not mean zero remaining work or complete test acceptance. Feature delivery statuses and evidence live only in FEATURES.md; the table below is their summary. A category has no independent completion state.

## Categorization rules

- Every FC- row has exactly one primary area, determined by the capability it delivers, regardless of whether it is Done, Planned, Deferred or another state.
- Preserve FC- identifiers when moving rows. Cross-area dependencies use links; do not count the same feature twice.
- Keep access/download/CSP features with installation and office operations. Keep deferred planning/tracking/subtask workflows with project work. Categories never group rows by status.
- Shared shell, cost, identity and recovery dependencies do not move a domain feature out of its primary area. Code ownership still follows [AGENTS.md](../AGENTS.md).
- These are **documentation categories**, unrelated to the product's areas feature, which is removed from v1.

## Area summary

The “Needs verification” column identifies gaps in the implementation audit, not confirmed missing features. Definitions for all states are in the [checklist status legend](FEATURES.md#reading-the-status). Totals count differently sized capabilities; they are not a completion percentage.

| Area | Scope | Total | Done | In progress | Planned | Blocked | Deferred | Needs verification |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| [AREA-01 · Application and browser](#area-01) | Shell, navigation, accessibility, search and preferences | 9 | 2 | 5 | 1 | 0 | 1 | 0 |
| [AREA-02 · Installation, access and office operations](#area-02) | Install, accounts/security, services, satellites and office lifecycle | 23 | 1 | 18 | 1 | 1 | 2 | 0 |
| [AREA-03 · Organization, people and teams](#area-03) | Membership, reporting, hiring, profiles, Home and activity | 13 | 1 | 12 | 0 | 0 | 0 | 0 |
| [AREA-04 · Projects, tasks, bugs and requests](#area-04) | Typed work, assignment/history, collaboration and planning gates | 23 | 0 | 16 | 2 | 0 | 5 | 0 |
| [AREA-05 · Feature definitions, knowledge and delivery](#area-05) | Feature/knowledge authoring, project documentation and release/deployment records | 17 | 0 | 1 | 15 | 0 | 1 | 0 |
| [AREA-06 · Chat and agent messaging](#area-06) | Conversations, asks and handoffs | 15 | 2 | 6 | 5 | 0 | 2 | 0 |
| [AREA-07 · Usage, costs and budgets](#area-07) | Usage, attribution, pricing, cost reads and budget controls | 11 | 0 | 1 | 9 | 0 | 1 | 0 |
| [AREA-08 · Execution, providers and supervision](#area-08) | Runtimes, routing, supervision and workspace isolation | 20 | 1 | 5 | 10 | 0 | 4 | 0 |
| [AREA-09 · Roles, processes and retrospectives](#area-09) | Role/guidance content, development processes and deferred learning engines | 18 | 0 | 3 | 10 | 0 | 5 | 0 |
| [AREA-10 · Project health and reports](#area-10) | Health, narratives, analytics and private digests | 5 | 0 | 0 | 0 | 0 | 5 | 0 |
| [AREA-11 · Copilot drafting](#area-11) | Reviewed drafts and suggestion candidates | 17 | 1 | 2 | 0 | 0 | 14 | 0 |
| [AREA-12 · Git, identity, synchronization and recovery](#area-12) | Repositories, canonical records, numbering, sync and restore | 15 | 1 | 5 | 6 | 0 | 3 | 0 |
| [AREA-13 · Session inspection and private memory](#area-13) | Session reads, human takeover and private profile memory | 3 | 0 | 0 | 1 | 0 | 2 | 0 |
| **Total** | | **189** | **9** | **74** | **60** | **1** | **45** | **0** |

## AREA-01

**Application and browser** · [Feature checklist](FEATURES.md#application-and-browser)

**Scope:** Shared browser navigation, responsive layout, accessibility, visual interactions, search/creation entry points, preferences and shell foundations.

**Boundary:** Shared presentation belongs here; domain operations belong to the domain area. Account authorization and browser service composition belong to AREA-02; usage calculations belong to AREA-07.

**Verification focus:** Shared controls have concrete UX gaps; Search/New and keyboard preferences are not built. Shell shortcut or layout tests alone do not establish those capabilities.

**Requirements:** [Shell UX](specs/app-shell-ux.md), [Executable migration](design/executable-migration.md).

## AREA-02

**Installation, access and office operations** · [Feature checklist](FEATURES.md#installation-access-and-office-operations)

**Scope:** Standalone installation, named instances, setup and registration links, organization creation, production browser service, account access/administration/recovery, satellites, office lifecycle, verified downloads and release hardening.

**Boundary:** Owns service/access/enrollment behavior. Team membership belongs to AREA-03; durable source restoration and process-safe cache rebuild belong to AREA-12. CSP is classified here with production security, even though it affects every screen.

**Verification focus:** Setup creates the instance and prints a one-time registration link; it creates no account, organization or office and asks for no password. Every account is registered in the browser through a one-time link and is tied to a person. A signed-in administrator creates the organization and becomes its owner. Restart, instance and logging primitives are partial; authorized relocation/removal, update/uninstall and usable diagnostics remain. TLS/custody primitives and satellite status/repair UI are partial; production handoff, CA escrow and invitation/approval integration are missing. On macOS, setup reserves the instance and registers its per-user service, the service host creates its registry at first start, and `admin reset` issues a replacement registration link (FC-011, FC-012). Browser onboarding (the registration page), physical authentication, SSH refusal for `admin reset` and authentic signed-release acceptance remain.

**Requirements:** [Installation/access UX](specs/server-installation-and-access-ux.md), [Office/team UX](specs/offices-and-teams-ux.md), [Host boundary](design/office-host-boundary.md), [Dogfood acceptance](specs/dogfood-run.md).

## AREA-03

**Organization, people and teams** · [Feature checklist](FEATURES.md#organization-people-and-teams)

**Scope:** Organization structure, people, home teams, lead/reporting relationships, accounts tied to persons, project accountability and project teams, the built-in HR lead and onboarding agents through HR, employee lifecycle, profiles, Home/Inbox and activity/presence.

**Boundary:** People and teams are organizational identity; offices are execution locations. Execution routing/holds belong to AREA-08, and account security grants belong to AREA-02. A chart position never proves authority.

**Verification focus:** Check home-team/lead enforcement, reporting, the account-to-person link (the owner is a person), the top-level team and project teams made by default, the HR lead made at organization creation and run only when called, the check of who may hire and the hirer's confirmation before any provisioning, and runtime effects of employee controls. Verify Inbox clearing and fresh, real activity sources.

**Requirements:** [Organization UX](specs/offices-and-teams-ux.md), [Employee/Home UX](specs/our-org-in-the-app.md), [Organization contract](design/organization-data/01-organization-structure.md), [Employee contract](design/organization-data/04-agent-structure.md).

## AREA-04

**Projects, tasks, bugs and requests** · [Feature checklist](FEATURES.md#projects-tasks-bugs-and-requests)

**Scope:** Project work views, typed tasks/bugs/requests, assignment and accountability, lifecycle/history, checklists, output, time, shared collaboration, author-private task notes and explicitly deferred planning/tracking workflows.

**Boundary:** Owns work behavior and task-private notes. Feature definitions and delivery records belong to AREA-05; chat transport belongs to AREA-06; profile memory belongs to AREA-13. Plain checklist steps do not select an execution engine.

**Verification focus:** Check unified task identities and office-minted references; assignment, reporter/history and transitions; bug triage/fix verification; request routing/outcomes; persisted checklists/output/time; comments/questions and needs-me visibility. Following/requested-by, backlog planning and subtask cascades remain Deferred.

**Requirements:** [Work UX](specs/work-items-ux.md), [Request UX](specs/feature-requests-ux.md), [Work design](design/tasks-projects-docs.md), [Assignment/history](design/organization-data/12-task-assignment-and-history.md), [Private task notes](design/organization-data/13-private-task-notes.md).

## AREA-05

**Feature definitions, knowledge and delivery** · [Feature checklist](FEATURES.md#feature-definitions-knowledge-and-delivery)

**Scope:** Canonical feature definitions/lifecycle and linked work; scoped knowledge, briefs, ADRs, shortcut provenance and feature limitations, document CLI/publication; the project documentation tree with its index, issued document versions, code links and derived convergence; release/milestone manifests, changelogs, deployment and flag records.

**Boundary:** Owns definitions and records, not deployment execution or flag evaluation. Task execution belongs to AREA-04; richer health/report generation belongs to AREA-10; Git transport/storage mechanics belong to AREA-12. Development processes, which declare a project's document kinds and starting sections, belong to AREA-09.

**Verification focus:** Check real feature storage/editing and task membership; document reader/editor, kinds/templates/search and publication/provenance; that the index lists exactly what the tree holds after every write, that an issued version cannot change and that an agent is refused another project's tree; release content/changelogs, deployment outcomes/rollback links and intended versus applied flag records. Authored records do not establish automation.

**Requirements:** [Features UX](specs/features-ux.md), [Knowledge UX](specs/decisions-and-tech-debt-ux.md), [Development processes and project documentation](design/organization-data/18-development-processes-and-project-documentation.md), [Release content](design/organization-data/09-release-content-and-flags.md), [Deployments](design/organization-data/07-release-deployments.md).

## AREA-06

**Chat and agent messaging** · [Feature checklist](FEATURES.md#chat-and-agent-messaging)

**Scope:** Agent CLI messaging, local mailbox delivery, conversations/channels/group DMs, threads, asks, attachments/reactions, handoffs, history and delivery.

**Boundary:** Owns conversation behavior and delivery. Assignment semantics belong to AREA-04; execution wake/stop primitives belong to AREA-08; durable cross-office restoration belongs to AREA-12. Shared Git is never chat storage.

**Verification focus:** Check visibility-safe mentions/search/attachments/reactions, drafts/history/pagination and performance, ask resolution/withdrawal, atomic task handoff and actual wake/pause/noninterrupting delivery. The existing command-line ask/reply is narrower than the selected production conversation contract.

**Requirements:** [Chat UX](specs/chat-ux.md), [Messaging contract](design/organization-data/15-messaging-and-chat.md), [Messaging ADR](adr/0003-agent-messaging-office-first.md).

## AREA-07

**Usage, costs and budgets** · [Feature checklist](FEATURES.md#usage-costs-and-budgets)

**Scope:** Durable usage/corrections, immutable historical attribution, model prices/tokens/USD and coverage, authorized cost views/export, period calculations and selected budget controls.

**Boundary:** Owns monetary/accounting and budget policy semantics. Execution enforcement and verified cleanup require AREA-08; protected archive restoration requires AREA-12. Release caps warn only, and detailed analytics remain Deferred.

**Verification focus:** Check the real usage collector/archive and corrections, immutable consumer/team/task/release snapshots, pricing and unknown/partial coverage, authorized live reads/CSV and account-zone period/bucket math. Budget service gaps remain separately marked Planned.

**Requirements:** [Costs UX](specs/costs-and-insights-ux.md), [Accounting contract](design/organization-data/10-usage-and-cost-accounting.md), [Budget contract](design/organization-data/17-budgets-and-controls.md).

## AREA-08

**Execution, providers and supervision** · [Feature checklist](FEATURES.md#execution-providers-and-supervision)

**Scope:** Native process primitives and runtime adapters, the agent command line's contract, explicit Run, execution/model configuration, connections/pools/permits, turns/attempts/placement, health monitoring, workspace isolation and deferred scheduling/delegation.

**Boundary:** Owns running work safely. Employee identity belongs to AREA-03, budget semantics to AREA-07, human session inspection/takeover to AREA-13, and durable recovery evidence to AREA-12. Starting an agent on an ask or an assignment is selected (ADR-0028); schedules, always-on agents and delegate engines remain Deferred.

**Verification focus:** Check each Claude/Codex/Agy adapter for real launch, streaming, cancellation and sandbox enforcement; check observed progress/health/waits; validate credential and other-task denies in the selected workspace topology. Generic process primitives do not prove full adapters.

**Requirements:** [Run-mode UX](specs/agent-run-modes-ux.md), [Supervision UX](specs/employee-supervision-ux.md), [Session design](design/agent-sessions.md), [Execution contract](design/organization-data/11-model-profiles-and-agent-execution.md), [Providers/pools](design/organization-data/14-model-providers-and-connection-pools.md).

## AREA-09

**Roles, processes and retrospectives** · [Feature checklist](FEATURES.md#roles-processes-and-retrospectives)

**Scope:** Role identity/assignment, scoped process and standard content, process editor UI, v1 process resolution and agent instruction, development processes, a primitive separate from organization processes (roles, skills, tabs, a project's choice of one, seed roster, role-named hire requests, import, the built-in desired state and feature delivery processes), and the separately deferred enforcement/autonomy/retrospective engines.

**Boundary:** Owns guidance content and proposed learning workflows. Execution configuration belongs to AREA-08; general document tooling, including the project documentation tree, issued versions and code links, belongs to AREA-05; the hire flow itself belongs to AREA-03. Resolution is selected; a development process is content, and mechanical enforcement, approvals and execution engines remain Deferred.

**Verification focus:** Check actual role record/assignment persistence and organization/team/project process or standard document storage. Existing content and editor UI do not establish runtime composition, inheritance, agent injection or retrospective execution.

**Requirements:** [Roles UX](specs/roles-ux.md), [Process UX](specs/processes-ux.md), [Process design](design/processes.md), [Development process design](design/development-processes.md), [Desired state design](design/desired-state-specifications.md), [REQ-06](specs/req-06-desired-state-spec-authoring-and-reconciliation.md), [REQ-07](specs/req-07-composable-process-packages-and-team-blueprints.md), [Retrospective UX](specs/retrospectives-ux.md).

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

**Scope:** Registered repositories, local Git caches and worktrees (hosting-provider integrations are Deferred to v2), canonical source/projection contracts, a replaceable storage engine, stable identity/number allocation, sync primitives and protected-source office continuity/recovery.

**Boundary:** Owns common durability/identity/transport mechanics. Domain payload and access rules remain in the relevant area. Organization Git alone cannot restore chat, private notes, accounting, budgets, credentials or execution authority.

**Verification focus:** Check repository/project branch associations and supported plain-Git/worktree operations; canonical round-trip/concurrency/tombstones; UUIDv7 stable IDs, office-minted task references and durable HQ allocations; storage reached only through engine-independent contracts. A draft schema or core sync library is insufficient.

**Requirements:** [Git design](design/projects-git-github.md), [Identity contract](design/organization-data/06-identity-and-numbering.md), [Record contract](design/organization-data/08-record-contracts.md), [Recovery contract](design/organization-data/16-recovery-and-office-continuity.md).

## AREA-13

**Session inspection and private memory** · [Feature checklist](FEATURES.md#session-inspection-and-private-memory)

**Scope:** Authorized read-only browser session/transcript inspection (no browser terminal, ADR-0004), office-local CLI takeover and handback and the separately deferred human-facing private notes/memory reader.

**Boundary:** Owns human inspection/control of a session and profile memory access. Process execution belongs to AREA-08; author-private task notes belong to AREA-04. A profile placeholder or runtime transcript does not establish a private memory store.

**Verification focus:** Check actual HQ-authorized session/event/transcript reads, privacy and freshness. CLI takeover is Deferred: paused by the owner on 2026-11-20 and moved to late v1, only if needed; the private memory reader remains Deferred with confined paths, caps, no-symlink checks and its separate human-only API.

**Requirements:** [Employee/profile UX](specs/our-org-in-the-app.md), [Session design](design/agent-sessions.md), [Memory/supervision gates](specs/employee-supervision-ux.md).

## Maintaining the area guide

Verification focus paragraphs describe checks still required for **completion**.

FEATURES.md remains the single row-level status/evidence record. Update this guide's summary when rows change status or category; update scope/boundaries when a category changes. Add a category only for a distinct capability domain, not to group backlog states. Keep AREA- identifiers stable, and update checklist/category navigation together.

For a Needs verification row, compare the owning requirements with current code and real persistence, authorization and runtime evidence. Record the implementation/verification links and remaining gap in that checklist row before choosing Done, In progress, Planned or Blocked. A requirement, a merged task, a test-file name or a fake-backed screen alone does not establish complete delivery. Deferred requirements retain their decision gates.
