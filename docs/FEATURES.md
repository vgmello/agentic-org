# Feature checklist

The single product feature/status index, categorized by the [documented feature areas](FEATURE-AREAS.md). Each row is a small capability with a short description linked to its full requirement. Supporting ADRs and data contracts remain authoritative; this file does not duplicate them. **An unchecked row is not complete.**

Current delivery direction (2026-11-16): [separate acceptance and merge units](../.agent-kit/tasks/v1-incremental-delivery.md). Internal foundations are not usable features. The accepted F0 guidance baseline imported the original 158-row inventory at the preservation checkpoint and left the concurrent original-checkout refresh intact. Subsequent units update only their delivered scope; this branch does not contain the full preserved implementation. Full-row completion, security and local review gates remain. Signing setup/authentic verification stays final.

Reviewed: **2026-11-16**, by Ada. Evidence baseline: `origin/v1` at `b1256c9771b15c0b47198828164e5efe78db2e96`; reconciled requirements at `b4f8e8b3456d1dc9dd5cf43bd3db3dd13d403a6e`. This inventory covers all 17 UX specs, the selected organization-data domains and supporting designs, including deferred candidates. It is a capability checklist, not one row per acceptance criterion or database field.

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

[First-five verification audit](verification/2026-11-16-first-five-features.md): three partial implementations and two planned capabilities confirmed on 2026-11-16; none complete. The [remaining-58 audit](verification/2026-11-16-remaining-feature-audit.md) then established 33 partial implementations and 25 planned capabilities. Remaining Needs verification rows: **0**. All 63 original unknown states have been classified; none became Done. That historical reservoir run reported an office source-guard failure, documented in its source-scoped record; it is not a current test result for each extracted unit.

| Status | Rows |
| --- | ---: |
| Done | 9 |
| In progress | 58 |
| Planned | 48 |
| Blocked | 3 |
| Deferred | 40 |
| Needs verification | 0 |
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
| [ ] | FC-011 | [macOS/Linux installation and named instances](specs/server-installation-and-access-ux.md) | In progress | Internal [release-verifier library](../crates/org-release/README.md) implements the pure signed-manifest/payload policy under [ADR-0009](adr/0009-complete-release-verification.md); it enables no installer/setup/service path. [Executable status] still records setup refusal. Consumers, platform admission, installation and authentic signed-release acceptance remain. |
| [ ] | FC-012 | [HQ bootstrap and user-service registration](specs/server-installation-and-access-ux.md) | Planned | setup refuses without creating state; protected bootstrap is unfinished in [executable status]. |
| [ ] | FC-013 | [Production browser service and domain APIs](design/gateway-api-v1.md) | Blocked | Blocked on authenticated host handoff/backend composition; web executable refuses startup [executable status]. |
| [ ] | FC-014 | [Owner first access, sign-in and sign-out](specs/server-installation-and-access-ux.md) | Blocked | Requires protected account/session backend; [UI] claims no real sign-in/setup behavior. |
| [ ] | FC-015 | [Session expiry and organization switching](specs/server-installation-and-access-ux.md) | Planned | Target authenticated session flow; session-foundation tests do not establish a real identity backend [UI]. |
| [ ] | FC-016 | [Recovery code and owner access recovery](specs/server-installation-and-access-ux.md) | In progress | Internal [REC-ESC-1 library](../crates/org-pki/src/lib.rs) and [five crypto tests](../crates/org-pki/src/tests.rs) implement fixed code/envelope parsing, sealing and same-key recovery under ADR-0007. Targeted production-library compilation/tests pass; full extracted-unit checks/reviews remain pending. HQ atomic persistence, protected collect/redelivery/export/ACK, actual physical authentication and complete recovery flow remain; no normal setup or REC-1 completion claimed. |
| [ ] | FC-017 | [TLS, CA custody and privilege separation](design/office-host-boundary.md) | In progress | Internal [socket confinement](../crates/org-socket/src/confinement.rs) and [loaded-image primitives](../crates/org-socket/src/image.rs) implement ADR-0008/0009 OS adapter scope; [subprocess vector](../crates/org-socket/examples/gateway_confinement.rs) covers disposable kernel enforcement. This enables no gateway/handoff. [Historical audit](verification/2026-11-16-first-five-features.md#fc-017) remains source-scoped; production handoff/authorization, CA escrow and signed macOS/Linux credential acceptance remain incomplete. |
| [ ] | FC-018 | [Create and open organization](specs/offices-and-teams-ux.md) | In progress | [browser tests] exercise explicit fakes; [UI] states creation/open gateway adapters unavailable. |
| [ ] | FC-019 | [Office list, readiness and freshness](specs/offices-and-teams-ux.md) | In progress | Office/status refresh tests exist in [browser tests]; real browser data path remains gated [UI]. |
| [ ] | FC-020 | [Satellite invite, join approval and repair](specs/offices-and-teams-ux.md) | In progress | [Audit](verification/2026-11-16-first-five-features.md#fc-020): Status/repair-instruction UI exists, but Add office is disabled and worker rejects non-HQ creation; invitation/approval/drain/repair integration remains. |
| [ ] | FC-021 | [Restart, relocate and remove office](specs/offices-and-teams-ux.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-021): Native restart exists; authorized relocate/remove, safe cleanup and workspace preservation remain. |
| [ ] | FC-022 | [Updates, uninstall and instance lifecycle](specs/server-installation-and-access-ux.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-022): Host instance registry/foreground lifecycle exists; update activation, uninstall and service setup remain. |
| [ ] | FC-023 | [Logs, doctor and support diagnostics](specs/server-installation-and-access-ux.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-023): Owner-only capped/rotating log files exist; usable logs/doctor/support CLI and end-to-end redaction remain. |
| [ ] | FC-024 | [Fresh-HQ and satellite release acceptance](specs/dogfood-run.md) | Blocked | Depends on real installation, authenticated browser APIs and both-platform service/recovery validation.  Owner defers signing setup and authentic signed-release verification to the final phase after other implementation; preserve fail-closed release code. This does not block ordinary functional development. See [phase amendment](../.agent-kit/tasks/v1-release-signing-deferral.md). |
| [ ] | FC-153 | [Account administration and organization ownership grants](specs/server-installation-and-access-ux.md) | Planned | Target account create/link/unlink/revoke and owner grants require protected backend; [UI] claims no real sign-in/setup behavior. |
| [ ] | FC-154 | [Verified downloads and signed offline bundles](specs/server-installation-and-access-ux.md) | Planned | [Executable status] explicitly lists downloads, signed bundles and update activation as pending integration.  Owner defers signing setup and authentic signed-release verification to the final phase after other implementation; preserve fail-closed release code. This does not block ordinary functional development. See [phase amendment](../.agent-kit/tasks/v1-release-signing-deferral.md). |
| [ ] | FC-155 | [Browser CSP and production asset security](design/rust-browser-bff-and-static-sveltekit.md) | In progress | [UI] documents production-static CSP harness and browser security checks; authenticated production service remains blocked. |

### Organization, people and teams

Category: [AREA-03 — scope and verification](FEATURE-AREAS.md#area-03).

Full requirements: [Organization, people and teams](specs/offices-and-teams-ux.md).

| Check | ID | Feature / short description | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| [ ] | FC-025 | [Team directory and organization chart](specs/offices-and-teams-ux.md) | In progress | Chart/view tests exist in [browser tests]; selected hierarchy and live adapter require reconciliation [UI]. |
| [ ] | FC-026 | [Add, edit and remove people](specs/offices-and-teams-ux.md) | In progress | [People tests] cover UI behavior; production adapter and selected membership enforcement remain to verify. |
| [ ] | FC-027 | [Create, move and archive teams](specs/offices-and-teams-ux.md) | In progress | [Team tests] cover UI behavior; live mutation, cycle checks and lead constraints remain to verify. |
| [ ] | FC-028 | [Single home team and subtree lead authority](design/organization-data/01-organization-structure.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-028): Team/lead rules exist, but current many-to-many membership conflicts with required single home team. |
| [ ] | FC-029 | [Reporting lines and owner-linked employee](design/organization-data/04-agent-structure.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-029): Reporting lines/cycle checks exist; owner bootstrap remains teamless and selected account/employee mapping is missing. |
| [ ] | FC-030 | [Project accountability and contributing teams](design/organization-data/02-project-structure.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-030): Project-team links exist; explicit accountable owner versus contributors and safe ownership transitions remain. |
| [ ] | FC-031 | [Built-in HR identity and home team](specs/our-org-in-the-app.md) | Planned | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-031): No built-in HR bootstrap or required valid home-team provisioning found. |
| [ ] | FC-032 | [Employee roster, cards and profiles](specs/our-org-in-the-app.md) | In progress | Roster/chart tests exist in [browser tests]; execution, activity and cost adapters remain separate. |
| [ ] | FC-033 | [Hire proposal, approval and provisioning](specs/our-org-in-the-app.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-033): Profile hiring/upsert exists; reviewed proposal, selected identity and real provisioning remain. |
| [ ] | FC-034 | [Edit, move, pause, resume and offboard employees](specs/our-org-in-the-app.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-034): Profile/status/offboarding mutations exist; actual execution stop/holds/fencing and safe workspace moves remain. |
| [ ] | FC-035 | [Home and unified action inbox](specs/our-org-in-the-app.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-035): HQ problem-source inbox backend exists; owner Home/unified browser Inbox and complete source/action semantics remain. |
| [ ] | FC-036 | [Live activity and truthful employee presence](specs/our-org-in-the-app.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-036): Organization activity/status and UI reducers exist; integrated cross-domain live timeline and production freshness remain. |
| [x] | FC-037 | [Retired existing-team import is gated](specs/our-org-in-the-app.md) | Done | Import removed from v1 per [UI]; [retired import test] records regression coverage. |

### Projects, tasks, bugs and requests

Category: [AREA-04 — scope and verification](FEATURE-AREAS.md#area-04).

Full requirements: [Projects, tasks, bugs and requests](specs/work-items-ux.md).

| Check | ID | Feature / short description | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| [ ] | FC-038 | [Project overview, work and team views](specs/work-items-ux.md) | In progress | Project/profile/work tests exist in [browser tests]; [UI] has no production project/work adapter claim. |
| [ ] | FC-039 | [Task board, list, detail and filters](specs/work-items-ux.md) | In progress | Tasks/work tests exist in [browser tests]; real selected-model mutations remain to verify. |
| [ ] | FC-040 | [Unified task, bug and request identity](design/organization-data/03-features-and-work.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-040): Legacy task/bug/request entities exist; unified selected typed T- identity and explicit import mappings remain. |
| [ ] | FC-041 | [Accountable team and independent assignment](design/organization-data/12-task-assignment-and-history.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-041): Individual assignment exists; independent team/person pair and target accountable ownership remain.  Current unaccepted assignment branch adds the persisted complete pair, strict native/HTTP/UI route and real-service Chromium evidence; current channel repairs and complete local acceptance remain open. See [branch checkpoint](verification/2026-11-16-assignment-branch-checkpoint.md). |
| [ ] | FC-042 | [Immutable reporter and task activity history](design/organization-data/12-task-assignment-and-history.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-042): Immutable SQL task events/reporter evidence exist; selected durable source/history/receipt migration remains.  Unaccepted branch implements frozen journal/source/event/receipt publication and public-provenance admission; ten new Markdown privacy regressions and all60selected tests pass. Full coherent runtime/check/review acceptance remains open; see [branch checkpoint](verification/2026-11-16-assignment-branch-checkpoint.md). Pure [lossless source editing](../crates/org-docs/src/source.rs) and [codec-2 event integrity](../crates/org-work/src/selected_history.rs) were separately accepted at `2fe7c3d3` and integrated at `1b911da2` after retained-author validation, current full checks and both fresh reviews. They establish no durable writer, public classification or usable history service. Separately accepted internal [F2b source](../crates/org-office/src/store/acceptance.rs) and scoped journal/crash/migration tests add storage evidence; F2b complete-unit checks/author/fresh reviews passed at `d55e3c25` (integration `eacb505d`); authorized history service and full-row acceptance remain. |
| [ ] | FC-043 | [Priority, status and task transition rules](specs/work-items-ux.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-043): Legacy lifecycle validation exists; selected unified transitions and authorized target adapters remain. |
| [ ] | FC-044 | [Report bugs with severity and reproduction](specs/work-items-ux.md) | In progress | Bug tests exist in [browser tests]; production adapter and selected task mapping remain open. |
| [ ] | FC-045 | [Duplicate linking and bug triage](specs/work-items-ux.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-045): Bug triage/duplicate relationships exist; selected typed-task mapping and end-to-end browser integration remain. |
| [ ] | FC-046 | [Fix task, verification, close and reopen](specs/work-items-ux.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-046): Legacy fix/verify/close/reopen behavior exists; selected history/assignment and live browser integration remain. |
| [ ] | FC-047 | [Capture and list feature requests](specs/feature-requests-ux.md) | In progress | Request tests exist in [browser tests]; production handler and selected task identity require integration. |
| [ ] | FC-048 | [Request routing and scoper selection](specs/feature-requests-ux.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-048): Request routing/manual fallback UI and legacy request behavior exist; authorized live target routing remains. |
| [ ] | FC-049 | [Request approval, decline and merge](specs/feature-requests-ux.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-049): Legacy request outcome transitions exist; selected typed identity, durable history and live adapters remain. |
| [ ] | FC-050 | [Plain task checklists](specs/work-items-ux.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-050): Persisted plain checklist operations exist; selected source/identity and production browser integration remain. |
| [ ] | FC-051 | [Task output and completion evidence](specs/work-items-ux.md) | Planned | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-051): No dedicated Output view, deliverable resolver or shared completion explanation; generic links do not satisfy this feature. |
| [ ] | FC-052 | [Task work and waiting time](specs/work-items-ux.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-052): Legacy time/waiting computations exist; revised reopen/baseline/overlap semantics and target live reads remain. |
| [ ] | FC-053 | [Author-private task notes](design/organization-data/13-private-task-notes.md) | Planned | Data index explicitly states runtime/access/UI unimplemented; shared comments do not satisfy privacy. |
| [ ] | FC-054 | [Shared comments, watchers and owner questions](design/organization-data/05-operational-data.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-054): Shared comments/watch/question state exists; selected source/receipt durability and authorized browser integration remain. |
| [ ] | FC-055 | [Needs-me and owner-wait views](specs/work-items-ux.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-055): Needs-you sample view and owner-wait primitives exist; authenticated live aggregation and selected scoping remain. |
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
| [ ] | FC-058 | [Canonical feature definition and lifecycle](specs/features-ux.md) | Planned | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-058): No selected feature definition/lifecycle persistence found; draft schemas are not product storage. |
| [ ] | FC-059 | [Feature list, page and editing](specs/features-ux.md) | Planned | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-059): Project feature tab is gated; no feature list/page/editor service or live UI found. |
| [ ] | FC-060 | [Feature task membership and affected bugs](specs/features-ux.md) | Planned | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-060): No persisted feature membership/affected-bug service or selected progress calculation found. |
| [ ] | FC-061 | [Knowledge library, reader and Markdown editor](specs/decisions-and-tech-debt-ux.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-061): Bounded Markdown/YAML parser exists; knowledge library, real reader/editor and scoped storage/publication remain. |
| [ ] | FC-062 | [Knowledge kinds, templates and scoped search](specs/decisions-and-tech-debt-ux.md) | Planned | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-062): No kind/template catalog or authorized title/ID/body search service found; parser has no kind awareness. |
| [ ] | FC-063 | [Project briefs and capability requirements](specs/decisions-and-tech-debt-ux.md) | Planned | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-063): No canonical project-brief/capability-requirement authoring/publication service found. |
| [ ] | FC-064 | [ADRs, designs, evaluations and Why links](specs/decisions-and-tech-debt-ux.md) | Planned | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-064): No application ADR/design/evaluation publication/history or Why-link behavior found. |
| [ ] | FC-065 | [Debt, limitation and exception records](specs/decisions-and-tech-debt-ux.md) | Planned | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-065): No scoped debt/limitation/exception record service found; required structures remain draft contracts. |
| [ ] | FC-066 | [Agent document CLI and publication](specs/decisions-and-tech-debt-ux.md) | Planned | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-066): Agent CLI has no document/publication commands; do not treat UX command examples as implemented. |
| [ ] | FC-067 | [Releases, milestones and explicit content manifests](design/organization-data/09-release-content-and-flags.md) | Planned | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-067): No release/milestone/content-manifest service found; draft record schema only. |
| [ ] | FC-068 | [Release product changelogs](design/organization-data/09-release-content-and-flags.md) | Planned | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-068): No authored product release-changelog storage/service/UI found. |
| [ ] | FC-069 | [Deployment records, environments and rollback links](design/organization-data/07-release-deployments.md) | Planned | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-069): No deployment-record/rollback-link/actual-snapshot persistence service found; deployment runner is separate. |
| [ ] | FC-070 | [Flag definitions and intended/applied snapshots](design/organization-data/09-release-content-and-flags.md) | Planned | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-070): No flag definition/intent/applied-snapshot service found; evaluation/deployment engines remain separate. |
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
| [ ] | FC-077 | [Mentions, search, attachments and reactions](specs/chat-ux.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-077): Sample chat interactions exist; protected visibility-safe search/attachments/reactions and live adapters remain. |
| [ ] | FC-078 | [Drafts, history bounds and conversation performance](specs/chat-ux.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-078): Sample drafts/history and bounded UI behavior exist; durable retention/pagination and real conversation performance remain. |
| [ ] | FC-079 | [Directed asks, answers and withdrawals](specs/chat-ux.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-079): Phase A CLI asks/replies and sample cards exist; selected protected-source resolution/withdrawal integration remains. |
| [ ] | FC-080 | [Atomic task handoff and system notices](specs/chat-ux.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-080): Sample recipient-only handoffs/notices exist; no atomic task/history/protected-message/receipt transaction. |
| [ ] | FC-081 | [Uninterrupting delivery, pause and waiting](specs/chat-ux.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-081): SQL-backed office mailbox send/inbox/read/held-wait is wired; execution-aware wake/holds/noninterrupting delivery remain. |
| [ ] | FC-082 | [Live transport cutover with triage preservation](adr/0003-agent-messaging-office-first.md) | Planned | [Agent CLI] explicitly retains installed Phase A tooling/spool until verified replacement; no automatic switch. |
| [ ] | FC-083 | [External chat bridges](specs/chat-ux.md) | Deferred | Optional bridge engines require separate selection and privacy/identity contracts. |

### Usage, costs and budgets

Category: [AREA-07 — scope and verification](FEATURE-AREAS.md#area-07).

Full requirements: [Usage, costs and budgets](specs/costs-and-insights-ux.md).

| Check | ID | Feature / short description | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| [ ] | FC-084 | [Immutable durable usage ledger and corrections](design/organization-data/10-usage-and-cost-accounting.md) | Planned | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-084): Only usage event types/fakes found; no real durable ledger, collector, protected archive or correction service. |
| [ ] | FC-085 | [Historical consumer, team, task and release attribution](design/organization-data/10-usage-and-cost-accounting.md) | Planned | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-085): No execution-time consumer/scope/historical-attribution capture service found. |
| [ ] | FC-086 | [Price versions, tokens, USD and coverage states](specs/costs-and-insights-ux.md) | Planned | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-086): Only token/USD event types/fakes found; no real capture, versioned pricing or coverage service. |
| [ ] | FC-087 | [Cost views, scoped permissions and CSV export](specs/costs-and-insights-ux.md) | Planned | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-087): No live ledger cost views, authorized amount reads or CSV export; request estimates are not recorded spend. |
| [ ] | FC-088 | [Period queries, bucketing and duration formatting](specs/costs-and-insights-ux.md) | Planned | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-088): No ledger period/bucket query or persisted account-zone service; generic formatting is insufficient. |
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
| [ ] | FC-101 | [Claude, Codex and Agy native runtime adapters](design/agent-sessions.md) | Planned | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-101): No native Claude/Codex/Agy adapters or office session runner; generic contracts/process primitives are separate. |
| [ ] | FC-102 | [Progress, health, waiting and stale-run detection](specs/employee-supervision-ux.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-102): Process health/recovery primitives exist; real-session observation, stale-run detection and supervision view remain. |
| [ ] | FC-103 | [Per-task repository workspace isolation](design/organization-data/04-agent-structure.md) | Planned | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-103): No selected task/repository workspace admission or native sandbox enforcement; generic path/process primitives are separate. |
| [ ] | FC-104 | [On-call, scheduled and always-on run modes](specs/agent-run-modes-ux.md) | Deferred | Automatic scheduling/wake/sleep engine remains unselected. |
| [ ] | FC-105 | [Extra model-policy slots and role overrides](specs/agent-run-modes-ux.md) | Deferred | Additional selection/Why policy requires its own contract; data fields do not activate routing policy. |
| [ ] | FC-106 | [Goals, reminders and automatic memory consolidation](specs/employee-supervision-ux.md) | Deferred | Scheduling/memory engine remains separate pending selection. |
| [ ] | FC-107 | [Private specialists and child delegation engines](specs/employee-supervision-ux.md) | Deferred | Native delegate/replica orchestration requires separate selection and execution/accounting boundaries. |

### Roles, processes and retrospectives

Category: [AREA-09 — scope and verification](FEATURE-AREAS.md#area-09).

Full requirements: [Roles, processes and retrospectives](specs/processes-ux.md).

| Check | ID | Feature / short description | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| [ ] | FC-108 | [Role records and employee role assignment](specs/roles-ux.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-108): Employee free-text role field exists; normalized role catalog/assignment and selected persistence remain. |
| [ ] | FC-109 | [Role inheritance, overrides and drift approval](specs/roles-ux.md) | Deferred | Earlier composition/injection engine remains deferred; existing role records do not implement it. |
| [ ] | FC-110 | [Scoped process and standard documents](design/processes.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-110): Earlier process parser/helpers and sample catalog exist; selected scoped process/standard persistence remains. |
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
| [ ] | FC-139 | [Shared repository registration and project branches](design/projects-git-github.md) | Planned | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-139): No organization repository registration/per-project integration-branch association service found. |
| [ ] | FC-140 | [Git worktrees and GitHub integration](design/projects-git-github.md) | Planned | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-140): No worktree/GitHub credential/push/PR adapters or commands found; path restrictions do not implement Git operations. |
| [ ] | FC-141 | [Durable canonical records and indexed projections](design/organization-data/08-record-contracts.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-141): SQLite work/event projection exists; selected canonical files, round-trip, revision/tombstone and restore adapters remain. |
| [ ] | FC-142 | [UUIDv7, scoped counters and readable references](design/organization-data/06-identity-and-numbering.md) | In progress | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-142): ULID validation and transactional project T- counters exist; new UUIDv7/scoped authority allocation and mappings remain. |
| [ ] | FC-143 | [Legacy data import mappings and staging](design/organization-data/12-task-assignment-and-history.md) | Planned | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-143): No explicit legacy ID/reference mapping, collision staging or unresolved-record migration workflow found. |
| [x] | FC-144 | [Replication codecs and storage-independent sync core](../crates/org-sync/README.md) | Done | [Sync core] documents implemented protocol/hash/authority, gap/retry/snapshot and receipt primitives; no network/filesystem I/O. |
| [ ] | FC-145 | [Durable replication and office continuity integration](design/organization-data/16-recovery-and-office-continuity.md) | In progress | [Sync core] exists; native source adapters, HQ ingest and protected source/recovery composition remain follow-up work. |
| [ ] | FC-146 | [Actor-bound mutation receipts and recovery journals](design/organization-data/16-recovery-and-office-continuity.md) | In progress | Internal [F2b journal/capture](../crates/org-office/src/store/acceptance.rs) implements frozen acceptance on the existing writer, original receipts, publication/replay and legacy SQL fences. Scoped storage tests pass, including 34 SIGKILL/reopen cases across 14 hooks, genuine schema migration and exact typed-text preservation. Recovery reflushes retained source and directory ancestry before confirmation; ancestor-flush failures fence startup. Native saved-result retries preserve journal/clock/publication state, including exhaustion, with fresh authority checks. F2b was independently accepted at `d55e3c25` and integrated at `eacb505d` after full checks and author/fresh reviews; protected host custody, actor-authorized service consumers, normal setup and complete continuity acceptance remain. |
| [ ] | FC-147 | [Protected chat, note, budget and usage restore](design/organization-data/16-recovery-and-office-continuity.md) | Planned | Target restore services remain work; Git clone or cache rebuild cannot restore these sources. |
| [ ] | FC-148 | [Credential rebind and process-safe cache rebuild](design/organization-data/16-recovery-and-office-continuity.md) | Planned | Implement durable authority/process reconciliation before execution; [supervisor] is only a lower-level primitive. |
| [ ] | FC-149 | [Automatic promotion and backup automation](design/organization-data/16-recovery-and-office-continuity.md) | Deferred | No automatic failover/backup engine selected by the record schema. |

### Session inspection and private memory

Category: [AREA-13 — scope and verification](FEATURE-AREAS.md#area-13).

Full requirements: [Session inspection and private memory](design/agent-sessions.md).

| Check | ID | Feature / short description | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| [ ] | FC-150 | [Inspect sessions and transcripts in browser](specs/our-org-in-the-app.md) | Planned | [Audit](verification/2026-11-16-remaining-feature-audit.md#fc-150): Sessions route is a placeholder; no authorized browser session/transcript inspector found. |
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
