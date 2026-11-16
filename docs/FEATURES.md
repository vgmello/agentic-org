# Feature checklist

The single product feature/status index, categorized by the [documented feature areas](FEATURE-AREAS.md). Each row is a small capability with a short description linked to its full requirement. Supporting ADRs and data contracts remain authoritative; this file does not duplicate them. **An unchecked row is not complete.**

Reviewed: **2026-11-16**, by Ada. Evidence baseline: `origin/v1` at `b1256c9771b15c0b47198828164e5efe78db2e96`; reconciled requirements at `b4f8e8b3456d1dc9dd5cf43bd3db3dd13d403a6e`. This initial inventory covers all 17 UX specs, the selected organization-data domains and supporting designs, including deferred candidates. It is a capability checklist, not one row per acceptance criterion or database field.

## Reading the status

| Status | Meaning |
| --- | --- |
| Done | Implementation is documented for the **exact scope of the row**, with linked evidence. Checked. A completed library does not complete its product integration. |
| In progress | Partial implementation or UI/fake-test coverage exists; the remaining gap is stated. Does not imply an agent is actively assigned. |
| Planned | Selected requirement with an explicitly documented implementation gap. No delivery claimed. |
| Blocked | Cannot complete until the named prerequisite is delivered. |
| Deferred | Not selected for implementation now, or explicitly gated pending a decision. |
| Needs verification | Selected feature, but current implementation evidence has not been reconciled sufficiently to call it complete, partial or absent. This is **not** a claim that no code exists. |

Done reflects the linked source evidence, **not a fresh execution of all acceptance tests**. Test-file links establish coverage exists, not that the tests were rerun for this documentation change. In particular, the [UI README][UI] states that production/ordinary development has unavailable domain adapters and no real sign-in/setup behavior; explicit test fakes cannot establish production completion. The [executable status] documents the blocked production browser composition. No complete standalone product release is claimed.

`FC-` identifiers are stable checklist row IDs, not product F- records or task IDs. The Check column uses `[x]` only for Done. Counts describe these differently sized rows; they are not a product completion percentage.

## Status totals

[First-five verification audit](verification/2026-11-16-first-five-features.md): three partial implementations and two planned capabilities confirmed on 2026-11-16; none complete. Remaining Needs verification rows: **58**.

| Status | Rows |
| --- | ---: |
| Done | 9 |
| In progress | 22 |
| Planned | 26 |
| Blocked | 3 |
| Deferred | 40 |
| Needs verification | 58 |
| **Total** | **158** |

## Feature tables

Features have one primary documentation category. See the [area guide](FEATURE-AREAS.md) for each category's scope, boundaries and verification focus. Categories are independent of delivery status; deferred features stay with their domain.

[Application and browser](#application-and-browser) | [Installation, access and office operations](#installation-access-and-office-operations) | [Organization, people and teams](#organization-people-and-teams) | [Projects, tasks, bugs and requests](#projects-tasks-bugs-and-requests) | [Feature definitions, knowledge and delivery](#feature-definitions-knowledge-and-delivery) | [Chat and agent messaging](#chat-and-agent-messaging) | [Usage, costs and budgets](#usage-costs-and-budgets) | [Execution, providers and supervision](#execution-providers-and-supervision) | [Roles, processes and retrospectives](#roles-processes-and-retrospectives) | [Project health and reports](#project-health-and-reports) | [Copilot drafting](#copilot-drafting) | [Git, identity, synchronization and recovery](#git-identity-synchronization-and-recovery) | [Session inspection and private memory](#session-inspection-and-private-memory)

### Application and browser

Category: [AREA-01 — scope and verification](FEATURE-AREAS.md#area-01).

Full requirements: [Application and browser](specs/app-shell-ux.md).

| Check | ID | Feature / short description | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| [x] | FC-001 | [Executable and package naming](design/executable-migration.md) | Done | Source/package rename and desktop-shell removal are implemented; deployment is tracked separately. |
| [x] | FC-002 | [Explicit unavailable browser states](specs/app-shell-ux.md) | Done | [UI] documents ordinary development/release gating; [production test] covers the boundary. |
| [ ] | FC-003 | [Navigation, sidebar and contextual drawers](specs/app-shell-ux.md) | In progress | [UI] and [browser tests] cover shell interactions; live domain adapters remain unavailable. |
| [ ] | FC-004 | [Responsive layout and keyboard accessibility](specs/app-shell-ux.md) | In progress | [browser tests] cover responsive, focus, landmarks and accessibility; full release matrix remains open. |
| [ ] | FC-005 | [Shared forms, feedback and visual rules](specs/app-shell-ux.md) | In progress | [Audit](verification/2026-11-16-first-five-features.md#fc-005): Shared primitives and selected focus/menu/form checks pass; skip-link, form errors/pending feedback, loading and reduced-motion gaps remain. |
| [ ] | FC-006 | [Search, New and scoped lookup](specs/app-shell-ux.md) | Planned | [Audit](verification/2026-11-16-first-five-features.md#fc-006): No global Search/New controls, /search route or authorized grouped lookup composition; implement selected entry points, references and shortcuts. |
| [ ] | FC-007 | [Device preferences and keyboard settings](specs/app-shell-ux.md) | Planned | [Audit](verification/2026-11-16-first-five-features.md#fc-007): No /preferences Keyboard switch or required device-storage key/fallback; implement persistence and shortcut/hint coupling. |
| [ ] | FC-008 | [Account timezone persistence](specs/costs-and-insights-ux.md) | Planned | Account-zone contract/persistence remains gated; browser-local time is not equivalent. |
| [ ] | FC-009 | [Native desktop shell](adr/0005-standalone-server-v1.md) | Deferred | Tauri is deferred to v2. |

### Installation, access and office operations

Category: [AREA-02 — scope and verification](FEATURE-AREAS.md#area-02).

Full requirements: [Installation, access and office operations](specs/server-installation-and-access-ux.md).

| Check | ID | Feature / short description | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| [x] | FC-010 | [Existing host and office launch entry points](design/executable-migration.md) | Done | Existing host lifecycle and worker entry points remain implemented; excludes setup and production web composition. |
| [ ] | FC-011 | [macOS/Linux installation and named instances](specs/server-installation-and-access-ux.md) | Planned | [Executable status] states setup/service registration unavailable; installer acceptance still required. |
| [ ] | FC-012 | [HQ bootstrap and user-service registration](specs/server-installation-and-access-ux.md) | Planned | setup refuses without creating state; protected bootstrap is unfinished in [executable status]. |
| [ ] | FC-013 | [Production browser service and domain APIs](design/gateway-api-v1.md) | Blocked | Blocked on authenticated host handoff/backend composition; web executable refuses startup [executable status]. |
| [ ] | FC-014 | [Owner first access, sign-in and sign-out](specs/server-installation-and-access-ux.md) | Blocked | Requires protected account/session backend; [UI] claims no real sign-in/setup behavior. |
| [ ] | FC-015 | [Session expiry and organization switching](specs/server-installation-and-access-ux.md) | Planned | Target authenticated session flow; session-foundation tests do not establish a real identity backend [UI]. |
| [ ] | FC-016 | [Recovery code and owner access recovery](specs/server-installation-and-access-ux.md) | Planned | Implement escrow, release/redelivery, confirmation and recovery flow; target requirement only. |
| [ ] | FC-017 | [TLS, CA custody and privilege separation](design/office-host-boundary.md) | In progress | [Audit](verification/2026-11-16-first-five-features.md#fc-017): TLS, custody and admission tests pass; production handoff/authorization, CA escrow and real signed macOS/Linux credential acceptance remain incomplete. |
| [ ] | FC-018 | [Create and open organization](specs/offices-and-teams-ux.md) | In progress | [browser tests] exercise explicit fakes; [UI] states creation/open gateway adapters unavailable. |
| [ ] | FC-019 | [Office list, readiness and freshness](specs/offices-and-teams-ux.md) | In progress | Office/status refresh tests exist in [browser tests]; real browser data path remains gated [UI]. |
| [ ] | FC-020 | [Satellite invite, join approval and repair](specs/offices-and-teams-ux.md) | In progress | [Audit](verification/2026-11-16-first-five-features.md#fc-020): Status/repair-instruction UI exists, but Add office is disabled and worker rejects non-HQ creation; invitation/approval/drain/repair integration remains. |
| [ ] | FC-021 | [Restart, relocate and remove office](specs/offices-and-teams-ux.md) | Needs verification | Verify authorization, cleanup and workspace preservation against current implementation. |
| [ ] | FC-022 | [Updates, uninstall and instance lifecycle](specs/server-installation-and-access-ux.md) | Needs verification | Reconcile installer/service lifecycle and retained data behavior; no complete release evidence recorded. |
| [ ] | FC-023 | [Logs, doctor and support diagnostics](specs/server-installation-and-access-ux.md) | Needs verification | Verify usable commands and secret-safe diagnostics against installation criteria. |
| [ ] | FC-024 | [Fresh-HQ and satellite release acceptance](specs/dogfood-run.md) | Blocked | Depends on real installation, authenticated browser APIs and both-platform service/recovery validation. |
| [ ] | FC-153 | [Account administration and organization ownership grants](specs/server-installation-and-access-ux.md) | Planned | Target account create/link/unlink/revoke and owner grants require protected backend; [UI] claims no real sign-in/setup behavior. |
| [ ] | FC-154 | [Verified downloads and signed offline bundles](specs/server-installation-and-access-ux.md) | Planned | [Executable status] explicitly lists downloads, signed bundles and update activation as pending integration. |
| [ ] | FC-155 | [Browser CSP and production asset security](design/rust-browser-bff-and-static-sveltekit.md) | In progress | [UI] documents production-static CSP harness and browser security checks; authenticated production service remains blocked. |

### Organization, people and teams

Category: [AREA-03 — scope and verification](FEATURE-AREAS.md#area-03).

Full requirements: [Organization, people and teams](specs/offices-and-teams-ux.md).

| Check | ID | Feature / short description | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| [ ] | FC-025 | [Team directory and organization chart](specs/offices-and-teams-ux.md) | In progress | Chart/view tests exist in [browser tests]; selected hierarchy and live adapter require reconciliation [UI]. |
| [ ] | FC-026 | [Add, edit and remove people](specs/offices-and-teams-ux.md) | In progress | [People tests] cover UI behavior; production adapter and selected membership enforcement remain to verify. |
| [ ] | FC-027 | [Create, move and archive teams](specs/offices-and-teams-ux.md) | In progress | [Team tests] cover UI behavior; live mutation, cycle checks and lead constraints remain to verify. |
| [ ] | FC-028 | [Single home team and subtree lead authority](design/organization-data/01-organization-structure.md) | Needs verification | Selected relationship contract; draft schema is not evidence of deployed membership/authorization enforcement. |
| [ ] | FC-029 | [Reporting lines and owner-linked employee](design/organization-data/04-agent-structure.md) | Needs verification | Verify identity/reporting independently of chart placement; account grants are separate. |
| [ ] | FC-030 | [Project accountability and contributing teams](design/organization-data/02-project-structure.md) | Needs verification | Audit selected owner/contributor model against current services and UI. |
| [ ] | FC-031 | [Built-in HR identity and home team](specs/our-org-in-the-app.md) | Needs verification | Verify explicit valid home team during bootstrap; special chart position does not satisfy identity requirements. |
| [ ] | FC-032 | [Employee roster, cards and profiles](specs/our-org-in-the-app.md) | In progress | Roster/chart tests exist in [browser tests]; execution, activity and cost adapters remain separate. |
| [ ] | FC-033 | [Hire proposal, approval and provisioning](specs/our-org-in-the-app.md) | Needs verification | Reconcile manual fallback and authorized approval with real provisioning; UI proposals do not establish execution. |
| [ ] | FC-034 | [Edit, move, pause, resume and offboard employees](specs/our-org-in-the-app.md) | Needs verification | Verify runtime effects, independent holds and safe placement changes, beyond profile UI. |
| [ ] | FC-035 | [Home and unified action inbox](specs/our-org-in-the-app.md) | Needs verification | Verify real questions, approvals, problems and correct clearing/dismissal semantics. |
| [ ] | FC-036 | [Live activity and truthful employee presence](specs/our-org-in-the-app.md) | Needs verification | Historical activity-port work does not prove live aggregate/recency integration; verify data sources and freshness. |
| [x] | FC-037 | [Retired existing-team import is gated](specs/our-org-in-the-app.md) | Done | Import removed from v1 per [UI]; [retired import test] records regression coverage. |

### Projects, tasks, bugs and requests

Category: [AREA-04 — scope and verification](FEATURE-AREAS.md#area-04).

Full requirements: [Projects, tasks, bugs and requests](specs/work-items-ux.md).

| Check | ID | Feature / short description | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| [ ] | FC-038 | [Project overview, work and team views](specs/work-items-ux.md) | In progress | Project/profile/work tests exist in [browser tests]; [UI] has no production project/work adapter claim. |
| [ ] | FC-039 | [Task board, list, detail and filters](specs/work-items-ux.md) | In progress | Tasks/work tests exist in [browser tests]; real selected-model mutations remain to verify. |
| [ ] | FC-040 | [Unified task, bug and request identity](design/organization-data/03-features-and-work.md) | Needs verification | Audit typed T- records and explicit old B-/REQ- mappings; selected schema is not an applied migration. |
| [ ] | FC-041 | [Accountable team and independent assignment](design/organization-data/12-task-assignment-and-history.md) | Needs verification | Verify nullable team/person assignment and ownership separately against services. |
| [ ] | FC-042 | [Immutable reporter and task activity history](design/organization-data/12-task-assignment-and-history.md) | Needs verification | Verify preserved creator, ordered durable events and post-archive history against selected contract. |
| [ ] | FC-043 | [Priority, status and task transition rules](specs/work-items-ux.md) | Needs verification | Verify selected lifecycle service and permissions; tests for older UI flows may cover a different model. |
| [ ] | FC-044 | [Report bugs with severity and reproduction](specs/work-items-ux.md) | In progress | Bug tests exist in [browser tests]; production adapter and selected task mapping remain open. |
| [ ] | FC-045 | [Duplicate linking and bug triage](specs/work-items-ux.md) | Needs verification | Verify current transition service, retained provenance and explicit human choices. |
| [ ] | FC-046 | [Fix task, verification, close and reopen](specs/work-items-ux.md) | Needs verification | Verify authorized real lifecycle and linked fix evidence rather than mock workflow alone. |
| [ ] | FC-047 | [Capture and list feature requests](specs/feature-requests-ux.md) | In progress | Request tests exist in [browser tests]; production handler and selected task identity require integration. |
| [ ] | FC-048 | [Request routing and scoper selection](specs/feature-requests-ux.md) | Needs verification | Verify leading-mention rules, authorized suggestions and labelled manual fallback. |
| [ ] | FC-049 | [Request approval, decline and merge](specs/feature-requests-ux.md) | Needs verification | Verify lifecycle, identity preservation and immutable outcome history against current service. |
| [ ] | FC-050 | [Plain task checklists](specs/work-items-ux.md) | Needs verification | Verify persistence and basic step changes separately from deferred advanced execution. |
| [ ] | FC-051 | [Task output and completion evidence](specs/work-items-ux.md) | Needs verification | Verify linked files/commits/documents and required completion evidence in actual task service. |
| [ ] | FC-052 | [Task work and waiting time](specs/work-items-ux.md) | Needs verification | Verify durations and waiting spans; distinguish measured work from calendar age. |
| [ ] | FC-053 | [Author-private task notes](design/organization-data/13-private-task-notes.md) | Planned | Data index explicitly states runtime/access/UI unimplemented; shared comments do not satisfy privacy. |
| [ ] | FC-054 | [Shared comments, watchers and owner questions](design/organization-data/05-operational-data.md) | Needs verification | Verify durable state and access; private notes remain a separate store and capability. |
| [ ] | FC-055 | [Needs-me and owner-wait views](specs/work-items-ux.md) | Needs verification | Verify authenticated reader scoping and live owner-wait/question sources; tracking/requested-by semantics are separate deferred rows. |
| [ ] | FC-056 | [Areas and advanced checklist execution](specs/work-items-ux.md) | Deferred | Not selected; basic checklist data does not select orchestration or an Areas engine. |
| [ ] | FC-057 | [Undo and organization-wide event history](design/organization-data/README.md) | Deferred | Explicitly unselected; task history is a distinct selected capability. |
| [ ] | FC-156 | [Following and requested-by tracking semantics](design/tasks-projects-docs.md) | Deferred | Section 7.2 gates AC-TK1–3 pending decisions and verified attribution; watcher data alone does not select these views. |
| [ ] | FC-157 | [Backlog ranking and planning lifecycle](design/tasks-projects-docs.md) | Deferred | Section 7.2 retains planning requirements pending lifecycle/storage decisions. |
| [ ] | FC-158 | [Subtask and cascade execution engine](design/tasks-projects-docs.md) | Deferred | Section 7.2 requires hierarchy/cascade decisions; simple checklist data does not implement this engine. |

### Feature definitions, knowledge and delivery

Category: [AREA-05 — scope and verification](FEATURE-AREAS.md#area-05).

Full requirements: [Feature definitions, knowledge and delivery](specs/features-ux.md).

| Check | ID | Feature / short description | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| [ ] | FC-058 | [Canonical feature definition and lifecycle](specs/features-ux.md) | Needs verification | Verify feature.md storage, owner, stable identity and planned/in-progress/live/retired states. |
| [ ] | FC-059 | [Feature list, page and editing](specs/features-ux.md) | Needs verification | Verify current UI/backend against FTR criteria; no feature completion inferred from docs existence. |
| [ ] | FC-060 | [Feature task membership and affected bugs](specs/features-ux.md) | Needs verification | Verify explicit membership confirmation, typed links and derived progress. |
| [ ] | FC-061 | [Knowledge library, reader and Markdown editor](specs/decisions-and-tech-debt-ux.md) | Needs verification | Verify authorized real storage, required project scope and optional feature/release/deployment scope. |
| [ ] | FC-062 | [Knowledge kinds, templates and scoped search](specs/decisions-and-tech-debt-ux.md) | Needs verification | Verify title/ID/body search, metadata and new kind/template behavior against current commands. |
| [ ] | FC-063 | [Project briefs and capability requirements](specs/decisions-and-tech-debt-ux.md) | Needs verification | Verify canonical authored records and publication; this repository's specs are not product implementation. |
| [ ] | FC-064 | [ADRs, designs, evaluations and Why links](specs/decisions-and-tech-debt-ux.md) | Needs verification | Verify document publication/history and provenance backlinks in the application. |
| [ ] | FC-065 | [Debt, limitation and exception records](specs/decisions-and-tech-debt-ux.md) | Needs verification | Structural records selected; verify actual authoring and scope before claiming complete. |
| [ ] | FC-066 | [Agent document CLI and publication](specs/decisions-and-tech-debt-ux.md) | Needs verification | Verify supported commands against current CLI; legacy UX syntax is not proof of implementation. |
| [ ] | FC-067 | [Releases, milestones and explicit content manifests](design/organization-data/09-release-content-and-flags.md) | Needs verification | Verify F-/R-/M- identity, explicit membership and release backlinks; no automatic inference from current tasks. |
| [ ] | FC-068 | [Release product changelogs](design/organization-data/09-release-content-and-flags.md) | Needs verification | Verify authored release content, independently of deployment technical changes. |
| [ ] | FC-069 | [Deployment records, environments and rollback links](design/organization-data/07-release-deployments.md) | Needs verification | Verify stored attempts, outcomes and actual commit snapshots; no deployment runner implied. |
| [ ] | FC-070 | [Flag definitions and intended/applied snapshots](design/organization-data/09-release-content-and-flags.md) | Needs verification | Verify record persistence separately from unselected flag evaluation or deployment execution. |
| [ ] | FC-071 | [Automatic feature transitions and release automation](specs/features-ux.md) | Deferred | Earlier transition/retro proposals do not activate a lifecycle or deployment engine. |

### Chat and agent messaging

Category: [AREA-06 — scope and verification](FEATURE-AREAS.md#area-06).

Full requirements: [Chat and agent messaging](specs/chat-ux.md).

| Check | ID | Feature / short description | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| [x] | FC-072 | [Phase A agent messaging CLI](../apps/agent-os/README.md) | Done | [Agent CLI] documents send/ask/reply, inbox/read/wait, defer/done/notify/due and relay; legacy spool transport only. |
| [x] | FC-073 | [Local mailbox delivery library slice](../crates/org-mailbox/README.md) | Done | [Mailbox] documents send/inbox/read/wait against in-memory testkit; no server/SQL/cross-office integration claimed. |
| [ ] | FC-074 | [Chat layout, threads and readable messages](specs/chat-ux.md) | In progress | Chat/followup/layout/accessibility tests exist in [browser tests]; production conversation adapter remains unavailable [UI]. |
| [ ] | FC-075 | [Protected conversation sources and memberships](design/organization-data/15-messaging-and-chat.md) | Planned | [Mailbox] lists target durable sources, channel/group admission and safe membership as follow-up adapters. |
| [ ] | FC-076 | [Channels, group DMs and member read state](specs/chat-ux.md) | Planned | [Mailbox] and [agent CLI] do not implement selected channel/group commands; fake UI is narrower evidence. |
| [ ] | FC-077 | [Mentions, search, attachments and reactions](specs/chat-ux.md) | Needs verification | Verify visibility-safe live services and durable sources against selected messaging contract. |
| [ ] | FC-078 | [Drafts, history bounds and conversation performance](specs/chat-ux.md) | Needs verification | Verify retention/pagination and 200-message/large-DM behavior beyond existing layout tests. |
| [ ] | FC-079 | [Directed asks, answers and withdrawals](specs/chat-ux.md) | Needs verification | Phase A ask/reply exists [agent CLI]; reconcile production cards, resolution and selected protected sources. |
| [ ] | FC-080 | [Atomic task handoff and system notices](specs/chat-ux.md) | Needs verification | Verify durable assignment/message transaction and privacy against selected task and recovery contracts. |
| [ ] | FC-081 | [Uninterrupting delivery, pause and waiting](specs/chat-ux.md) | Needs verification | Verify actual execution wake/delivery semantics, not only live-delivery fake tests. |
| [ ] | FC-082 | [Live transport cutover with triage preservation](adr/0003-agent-messaging-office-first.md) | Planned | [Agent CLI] explicitly retains installed Phase A tooling/spool until verified replacement; no automatic switch. |
| [ ] | FC-083 | [External chat bridges](specs/chat-ux.md) | Deferred | Optional bridge engines require separate selection and privacy/identity contracts. |

### Usage, costs and budgets

Category: [AREA-07 — scope and verification](FEATURE-AREAS.md#area-07).

Full requirements: [Usage, costs and budgets](specs/costs-and-insights-ux.md).

| Check | ID | Feature / short description | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| [ ] | FC-084 | [Immutable durable usage ledger and corrections](design/organization-data/10-usage-and-cost-accounting.md) | Needs verification | Verify collector, protected archive, de-duplication and correction adapters against selected records. |
| [ ] | FC-085 | [Historical consumer, team, task and release attribution](design/organization-data/10-usage-and-cost-accounting.md) | Needs verification | Verify immutable execution-time snapshots; current assignment is not a valid historical fallback. |
| [ ] | FC-086 | [Price versions, tokens, USD and coverage states](specs/costs-and-insights-ux.md) | Needs verification | Verify actual route/model pricing and partial/unknown data instead of fabricated totals. |
| [ ] | FC-087 | [Cost views, scoped permissions and CSV export](specs/costs-and-insights-ux.md) | Needs verification | Verify live authorized reads and export; test row-cost presentation alone does not establish accounting. |
| [ ] | FC-088 | [Period queries, bucketing and duration formatting](specs/costs-and-insights-ux.md) | Needs verification | Verify account-zone boundaries and rolling calendar semantics against real ledger queries. |
| [ ] | FC-089 | [Task lifetime and agent daily/monthly budgets](design/organization-data/17-budgets-and-controls.md) | Planned | Data index explicitly states budget runtime service unimplemented; selected definitions are not admission enforcement. |
| [ ] | FC-090 | [Release warning caps](design/organization-data/17-budgets-and-controls.md) | Planned | Selected warning-only policy; budget runtime service still unimplemented. |
| [ ] | FC-091 | [Threshold notices and independent execution holds](design/organization-data/17-budgets-and-controls.md) | Planned | Implement 80/100 notices, stop_pending and verified cleanup without clearing manual/security holds. |
| [ ] | FC-092 | [Budget raises, extensions and reached-once rules](design/organization-data/17-budgets-and-controls.md) | Planned | Implement authorized period-bound controls, one-use rules and immutable history; schema alone is insufficient. |
| [ ] | FC-093 | [Own-agent budget access and CLI](specs/costs-and-insights-ux.md) | Planned | [Agent CLI] explicitly claims no implemented budget command; apply selected own-agent reader exception. |
| [ ] | FC-094 | [Detailed analytics and organization Insights](specs/costs-and-insights-ux.md) | Deferred | Separately gated analytics scope; base usage accounting remains selected. |

### Execution, providers and supervision

Category: [AREA-08 — scope and verification](FEATURE-AREAS.md#area-08).

Full requirements: [Execution, providers and supervision](specs/agent-run-modes-ux.md).

| Check | ID | Feature / short description | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| [x] | FC-095 | [Owned process spawning and cleanup primitives](../crates/org-supervise/README.md) | Done | [Supervisor] documents piped spawn, identity ordering, stop/socket cleanup and native orphan discovery; excludes office integration. |
| [ ] | FC-096 | [Explicit agent Run and runtime supervision](specs/employee-supervision-ux.md) | In progress | [Supervisor] primitives exist; placement, permits, durable ownership and real adapter integration still required. |
| [ ] | FC-097 | [Employee execution configuration and model profiles](design/organization-data/11-model-profiles-and-agent-execution.md) | Planned | [Supervisor] explicitly excludes configuration/profile services; target SQL is not an applied migration. |
| [ ] | FC-098 | [Provider subscriptions and ordered connection pools](design/organization-data/14-model-providers-and-connection-pools.md) | Planned | [Supervisor] excludes ordered pool services; actual routing/bindings must implement the selected contract. |
| [ ] | FC-099 | [Shared quota permits, cooldown and failover](design/organization-data/11-model-profiles-and-agent-execution.md) | Planned | Requires HQ admission and actual attempt identity; native process primitives do not implement routing. |
| [ ] | FC-100 | [Single open turn, attempts and placement fencing](design/organization-data/11-model-profiles-and-agent-execution.md) | Planned | Integrate durable generations, occupied unknown cleanup and proof before reassignment [supervisor]. |
| [ ] | FC-101 | [Claude, Codex and Agy native runtime adapters](design/agent-sessions.md) | Needs verification | Audit each adapter's real launch, streaming, cancellation and sandbox behavior; generic process library is insufficient. |
| [ ] | FC-102 | [Progress, health, waiting and stale-run detection](specs/employee-supervision-ux.md) | Needs verification | Verify out-of-model observation and truthful stop/cleanup states against real sessions. |
| [ ] | FC-103 | [Per-task repository workspace isolation](design/organization-data/04-agent-structure.md) | Needs verification | Verify fresh sandbox/credential/other-task denies for target topology; old path allowlists do not prove safety [supervisor]. |
| [ ] | FC-104 | [On-call, scheduled and always-on run modes](specs/agent-run-modes-ux.md) | Deferred | Automatic scheduling/wake/sleep engine remains unselected. |
| [ ] | FC-105 | [Extra model-policy slots and role overrides](specs/agent-run-modes-ux.md) | Deferred | Additional selection/Why policy requires its own contract; data fields do not activate routing policy. |
| [ ] | FC-106 | [Goals, reminders and automatic memory consolidation](specs/employee-supervision-ux.md) | Deferred | Scheduling/memory engine remains separate pending selection. |
| [ ] | FC-107 | [Private specialists and child delegation engines](specs/employee-supervision-ux.md) | Deferred | Native delegate/replica orchestration requires separate selection and execution/accounting boundaries. |

### Roles, processes and retrospectives

Category: [AREA-09 — scope and verification](FEATURE-AREAS.md#area-09).

Full requirements: [Roles, processes and retrospectives](specs/processes-ux.md).

| Check | ID | Feature / short description | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| [ ] | FC-108 | [Role records and employee role assignment](specs/roles-ux.md) | Needs verification | Selected identity data is separate from execution configuration; verify actual authoring/persistence. |
| [ ] | FC-109 | [Role inheritance, overrides and drift approval](specs/roles-ux.md) | Deferred | Earlier composition/injection engine remains deferred; existing role records do not implement it. |
| [ ] | FC-110 | [Scoped process and standard documents](design/processes.md) | Needs verification | Organization/team/project content is selected; verify persistence without claiming an inheritance engine. |
| [ ] | FC-111 | [Process catalog and editor UI](specs/processes-ux.md) | In progress | [Process tests] exercise UI; production data adapter and selected structural model remain to verify [UI]. |
| [ ] | FC-112 | [Process resolution, sealed overrides and agent injection](specs/processes-ux.md) | Deferred | Runtime inheritance/execution engine is explicitly unselected. |
| [ ] | FC-113 | [Asks-first and full-autonomy policy engine](specs/processes-ux.md) | Deferred | Proposed behavior requires a selected authorization/execution contract. |
| [ ] | FC-114 | [Retrospective routines and scheduled occurrences](specs/retrospectives-ux.md) | Deferred | Routine engine is deferred; settings do not establish scheduling. |
| [ ] | FC-115 | [Retrospective fact gathering and reviewed publication](specs/retrospectives-ux.md) | Deferred | Deferred lead/participant, no-change and owner-review workflow. |
| [ ] | FC-116 | [Retrospective action tracking and missed-run problems](specs/retrospectives-ux.md) | Deferred | Deferred budgets, no-lead/missed occurrence handling and improvement application. |

### Project health and reports

Category: [AREA-10 — scope and verification](FEATURE-AREAS.md#area-10).

Full requirements: [Project health and reports](specs/project-status-ux.md).

| Check | ID | Feature / short description | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| [ ] | FC-117 | [Project health snapshots and status overview](specs/project-status-ux.md) | Deferred | Status feature proposal awaits selected reporting/health service; ordinary project overview is tracked separately. |
| [ ] | FC-118 | [Reporter narratives and periodic status reports](specs/project-status-ux.md) | Deferred | Report generation/scheduling is pending; Git narrative cannot expose protected amounts. |
| [ ] | FC-119 | [Progress, burnup and throughput charts](specs/project-status-ux.md) | Deferred | Explicitly deferred analytics; no charts inferred from task records. |
| [ ] | FC-120 | [Cycle-time percentiles and waiting analysis](specs/project-status-ux.md) | Deferred | Explicitly deferred sample/overlap/median/P85 requirements. |
| [ ] | FC-121 | [Private away digests and daily summaries](specs/project-status-ux.md) | Deferred | Deferred account-only context, window/threshold/seen-state and retention engine. |

### Copilot drafting

Category: [AREA-11 — scope and verification](FEATURE-AREAS.md#area-11).

Full requirements: [Copilot drafting](design/smart-features.md).

| Check | ID | Feature / short description | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| [x] | FC-122 | [Copilot registry, prompt and output validation library](design/smart-features.md) | Done | Design explicitly identifies implemented registry/rendering/validation/fake-runtime contract; excludes real runtime integration. |
| [ ] | FC-123 | [Draft or improve text](design/smart-features.md) | In progress | text.improve library entry exists; native execution, prepared context and ordinary reviewed save need integration. |
| [ ] | FC-124 | [Draft a bug from a sentence](design/smart-features.md) | In progress | bug.from_sentence library entry exists; target task model and real office runtime remain integration work. |
| [ ] | FC-125 | [Suggest duplicate bugs](design/smart-features.md) | Deferred | Candidate 2; review up to three suggestions, no automatic merge/close. |
| [ ] | FC-126 | [Suggest bug triage](design/smart-features.md) | Deferred | Candidate 3; human confirms scope, severity, priority and assignment. |
| [ ] | FC-127 | [Draft a hire from description](design/smart-features.md) | Deferred | Candidate 5; ordinary reviewed hire, no direct provisioning. |
| [ ] | FC-128 | [Draft a definition of done](design/smart-features.md) | Deferred | Candidate 6; editable completion text, no new completion engine. |
| [ ] | FC-129 | [Break a goal into task proposals](design/smart-features.md) | Deferred | Candidate 7; confirm accepted proposals before creating tasks. |
| [ ] | FC-130 | [Draft a fix task from a bug](design/smart-features.md) | Deferred | Candidate 8; reviewed form, no repository access. |
| [ ] | FC-131 | [Suggest replies](design/smart-features.md) | Deferred | Candidate 9; authorized prepared turns, ordinary Send required. |
| [ ] | FC-132 | [Summarize linked activity or conversations](design/smart-features.md) | Deferred | Candidate 10; source-linked, authorized read-only output. |
| [ ] | FC-133 | [Draft a document or project brief](design/smart-features.md) | Deferred | Candidate 11; reviewed scoped Markdown publication. |
| [ ] | FC-134 | [Draft release notes](design/smart-features.md) | Deferred | Candidate 12; verified work, normal reviewed Save. |
| [ ] | FC-135 | [Ask the organization with source links](design/smart-features.md) | Deferred | Candidate 13; office-prepared authorized context, no copilot live tools/search. |
| [ ] | FC-136 | [Suggest a team grouping](design/smart-features.md) | Deferred | Candidate 14; prefill ordinary form, no automatic employee moves. |
| [ ] | FC-137 | [Propose team staffing](design/smart-features.md) | Deferred | Candidate 15; review each hire and execution choice. |
| [ ] | FC-138 | [Suggest request routing and scopers](design/smart-features.md) | Deferred | Candidate 16; reviewed suggestions with manual fallback. |

### Git, identity, synchronization and recovery

Category: [AREA-12 — scope and verification](FEATURE-AREAS.md#area-12).

Full requirements: [Git, identity, synchronization and recovery](design/organization-data/README.md).

| Check | ID | Feature / short description | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| [ ] | FC-139 | [Shared repository registration and project branches](design/projects-git-github.md) | Needs verification | Verify remote organization Git, local/remote code and per-project repository integration branch associations. |
| [ ] | FC-140 | [Git worktrees and GitHub integration](design/projects-git-github.md) | Needs verification | Verify actual supported operations and credentials; repository association design does not prove end-to-end delivery. |
| [ ] | FC-141 | [Durable canonical records and indexed projections](design/organization-data/08-record-contracts.md) | Needs verification | Audit file round-trip, concurrency revisions, tombstones and projection adapters against selected records. |
| [ ] | FC-142 | [UUIDv7, scoped counters and readable references](design/organization-data/06-identity-and-numbering.md) | Needs verification | Verify durable authority allocation and unresolved local IDs without renumbering/import loss. |
| [ ] | FC-143 | [Legacy data import mappings and staging](design/organization-data/12-task-assignment-and-history.md) | Needs verification | Preserve stable IDs/provenance and unresolved data; retired existing-team UI import is a different feature. |
| [x] | FC-144 | [Replication codecs and storage-independent sync core](../crates/org-sync/README.md) | Done | [Sync core] documents implemented protocol/hash/authority, gap/retry/snapshot and receipt primitives; no network/filesystem I/O. |
| [ ] | FC-145 | [Durable replication and office continuity integration](design/organization-data/16-recovery-and-office-continuity.md) | In progress | [Sync core] exists; native source adapters, HQ ingest and protected source/recovery composition remain follow-up work. |
| [ ] | FC-146 | [Actor-bound mutation receipts and recovery journals](design/organization-data/16-recovery-and-office-continuity.md) | Planned | Selected durable command/source contract requires target adapters; core receipt primitives are narrower [sync core]. |
| [ ] | FC-147 | [Protected chat, note, budget and usage restore](design/organization-data/16-recovery-and-office-continuity.md) | Planned | Target restore services remain work; Git clone or cache rebuild cannot restore these sources. |
| [ ] | FC-148 | [Credential rebind and process-safe cache rebuild](design/organization-data/16-recovery-and-office-continuity.md) | Planned | Implement durable authority/process reconciliation before execution; [supervisor] is only a lower-level primitive. |
| [ ] | FC-149 | [Automatic promotion and backup automation](design/organization-data/16-recovery-and-office-continuity.md) | Deferred | No automatic failover/backup engine selected by the record schema. |

### Session inspection and private memory

Category: [AREA-13 — scope and verification](FEATURE-AREAS.md#area-13).

Full requirements: [Session inspection and private memory](design/agent-sessions.md).

| Check | ID | Feature / short description | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| [ ] | FC-150 | [Inspect sessions and transcripts in browser](specs/our-org-in-the-app.md) | Needs verification | Verify authorized HQ inspection, event history, privacy and freshness against real sessions, beyond profile cards. |
| [ ] | FC-151 | [Authorized CLI takeover and PTY handback](design/agent-sessions.md) | Planned | [Supervisor] explicitly leaves PTYs for a separate delivery; verify exclusive takeover leases and safe handback before claiming full support. |
| [ ] | FC-152 | [Private notes and memory profile reader](specs/our-org-in-the-app.md) | Deferred | Deferred human-only read API/UI, confined journal/MEMORY paths, caps and no-symlink/security checks; memory schema/retention remain pending. |

## Keeping this checklist current

Update the relevant row in the same PR that changes implementation or selects/defers scope. Preserve row IDs; add a row when a separately deliverable capability is introduced. Assign it to exactly one primary category in the [area guide](FEATURE-AREAS.md), and update that guide's area totals with any status/category change. Cross-area dependencies belong in links or evidence, not duplicate feature rows. These are documentation categories; they do not select the deferred product Areas feature. Keep acceptance details in the linked owning spec. Ada maintains this index; the implementing area's owner supplies evidence and remaining work.

To mark Done, link the implementation commit/PR or current implementation documentation and the relevant verification result for the row's full scope, including real persistence, authorization and runtime integration where required. A merged PR, a draft schema or a fake-backed screen alone is insufficient. Record any remaining gap as In progress. If evidence is missing or contradictory, use Needs verification and state what must be checked. When implementation changes invalidate an earlier Done claim, reopen it.

Use this table to choose and reconcile work, then link an assigned issue/PR in its evidence cell; do not imply that every unchecked row is an active task. Needs verification rows require an implementation audit before estimating or rebuilding them. Deferred rows require the decision described in their owning contract before implementation. The feature status itself (for example planned/live/retired inside the product) is distinct from delivery status in this checklist.

Historical agent TODO lists, handovers and merged-task logs can help locate commits, but are not additional authoritative completion lists. This inventory does not manufacture acceptance evidence from them. The [documentation authority map](design/documentation-authority.md) resolves requirement precedence; new integrated requirements are implementation targets until verified.

[UI]: ../apps/agentic-org-ui/README.md
[browser tests]: ../apps/agentic-org-ui/e2e/
[production test]: ../apps/agentic-org-ui/e2e/production-unavailable.spec.ts
[People tests]: ../apps/agentic-org-ui/e2e/org-chart-people-edit.spec.ts
[Team tests]: ../apps/agentic-org-ui/e2e/org-chart-teams-edit.spec.ts
[Process tests]: ../apps/agentic-org-ui/e2e/processes.spec.ts
[retired import test]: ../apps/agentic-org-ui/e2e/retired-import.spec.ts
[executable status]: design/executable-migration.md
[Agent CLI]: ../apps/agent-os/README.md
[Mailbox]: ../crates/org-mailbox/README.md
[Supervisor]: ../crates/org-supervise/README.md
[Sync core]: ../crates/org-sync/README.md
