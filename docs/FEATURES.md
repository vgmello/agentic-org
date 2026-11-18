# Feature checklist

The single product feature/status index, categorized by the [documented feature areas](FEATURE-AREAS.md). Each row is a small capability with a short description linked to its full requirement. Supporting ADRs and data contracts remain authoritative; this file does not duplicate them. **An unchecked row is not complete.**

The order of work is the [implementation plan](implementation-plan.md). Internal foundations are not usable features. Signing setup and authentic signed-release verification come last.

This inventory covers all 17 UX specs, the selected organization-data domains and supporting designs, including deferred candidates. It is a capability checklist, not one row per acceptance criterion or database field.

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

Evidence describes what the code holds, read from the source; "(not verified)" marks a claim this revision did not recheck. Withdrawn rows keep their ID with status Deferred and say why.

| Status | Rows |
| --- | ---: |
| Done | 9 |
| In progress | 62 |
| Planned | 63 |
| Blocked | 1 |
| Deferred | 42 |
| Needs verification | 0 |
| **Total** | **177** |

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
| [ ] | FC-005 | [Shared forms, feedback and visual rules](specs/app-shell-ux.md) | In progress | Shared primitives and selected focus/menu/form checks pass; skip-link, form errors/pending feedback, loading and reduced-motion gaps remain. |
| [ ] | FC-006 | [Search, New and scoped lookup](specs/app-shell-ux.md) | Planned | No global Search/New controls, /search route or authorized grouped lookup composition; implement selected entry points, references and shortcuts. |
| [ ] | FC-007 | [Device preferences and keyboard settings](specs/app-shell-ux.md) | Planned | No /preferences Keyboard switch or required device-storage key/fallback; implement persistence and shortcut/hint coupling. |
| [ ] | FC-008 | [Account timezone persistence](specs/costs-and-insights-ux.md) | Planned | Account-zone contract/persistence remains gated; browser-local time is not equivalent. |
| [ ] | FC-009 | [Native desktop shell](adr/0005-standalone-server-v1.md) | Deferred | A native desktop application is not in v1. |

### Installation, access and office operations

Category: [AREA-02 — scope and verification](FEATURE-AREAS.md#area-02).

Full requirements: [Installation, access and office operations](specs/server-installation-and-access-ux.md).

| Check | ID | Feature / short description | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| [x] | FC-010 | [Existing host and office launch entry points](design/executable-migration.md) | Done | Existing host lifecycle and worker entry points remain implemented; excludes setup and production web composition. |
| [ ] | FC-011 | [macOS/Linux installation and named instances](specs/server-installation-and-access-ux.md) | In progress | Internal [release-verifier library](../crates/org-release/README.md) implements the pure signed-manifest/payload policy under [ADR-0009](adr/0009-complete-release-verification.md); it enables no installer/setup/service path. [Executable status] still records setup refusal. Consumers, platform admission, installation and authentic signed-release acceptance remain. |
| [ ] | FC-012 | [HQ bootstrap and user-service registration](specs/server-installation-and-access-ux.md) | In progress | `agentic-org setup` refuses and changes nothing (`apps/agentic-org/src/lib.rs`). Host instance registry and protected setup code exist in `crates/org-host/src/instance` and `bootstrap.rs` and are reached only from tests. Required: setup creates the instance, registers the user service, creates no account and asks for no password, and prints one one-time registration link (expiring, stored only as a digest, re-issued by a command on the server machine); the person registers the first account, an instance administrator, in the browser. Setup creates no organization and no office. |
| [ ] | FC-013 | [Production browser service and domain APIs](design/web-api-v1.md) | In progress | F4: in debug/test instance composition the host starts `agentic-org-web`, checks its image and sandbox before the `[Transfer, Listener]` handoff over the rendezvous ([`web.rs`](../crates/org-host/src/web.rs)), marks it ready after `web.serving`, and restarts it after control EOF; the web process serves only `GET /api/v1/version` and answers every other `/api/v1` path, including the session routes, with JSON 404. Real-process test `bootstrap::tests::web_process_tests::actual_host_spawns_web_process_serves_version_and_restarts_after_eof`. No sign-in until stored sessions (A3, FC-159); production `serve` start is A2/A3. The host serves the work reads the web needs through the office channel; it answers only `hello` and `instance.status` today. |
| [ ] | FC-014 | [First access through a registration link, sign-in and sign-out](specs/server-installation-and-access-ux.md) | In progress | `crates/org-host/src/service` has account verification and session handlers (sessions held in memory), answering only for a registered instance, which only tests register. Remaining: registration of the first account through setup's one-time link in the browser, browser sign-in and sign-out, and command-line sign-in approved in the browser through a one-time link; [UI] claims no real sign-in or setup behavior. |
| [ ] | FC-015 | [Session expiry and organization switching](specs/server-installation-and-access-ux.md) | Planned | Target authenticated session flow; session-foundation tests do not establish a real identity backend [UI]. |
| [ ] | FC-016 | [Recovery code and owner access recovery](specs/server-installation-and-access-ux.md) | In progress | Recovery-code library in `crates/org-pki` (code/envelope parsing, sealing and same-key recovery under ADR-0007) with tests. HQ creation persistence in `crates/org-office/src/office/hq_creation.rs` and host provisioning, writer custody and recovery-credential storage in `crates/org-host` (`provisioning.rs`, `writer`, `hostdb`) exist with tests and are reached only from tests. Remaining: delivery of the recovery code once on the HQ machine through a running instance, browser flow, REC-1 acceptance. |
| [ ] | FC-017 | [TLS, CA custody and privilege separation](design/office-host-boundary.md) | In progress | Socket confinement and loaded-image primitives in `crates/org-socket` (`confinement.rs`, `image.rs`) implement the ADR-0008/0009 OS adapter scope, with a subprocess example for kernel enforcement. F4 uses them for the debug/test web handoff: the host asks the confinement adapter whether the platform is supported and checks the ready report there (`web_confinement_supported`, `verify_ready_report`; other platforms refuse with a reported reason), then verifies the child's sandbox before the `[Transfer, Listener]` handoff ([`web.rs`](../crates/org-host/src/web.rs)). Remaining: production handoff and authorization, CA escrow, signed macOS/Linux credential acceptance. One launch contract, the rendezvous form, with per-person grants verified by the worker; `app_token` and the second form are removed ([host boundary](design/office-host-boundary.md#what-the-code-does-today)). |
| [ ] | FC-018 | [Create and open organization](specs/offices-and-teams-ux.md) | In progress | A signed-in instance administrator creates the organization and becomes its owner. The request carries name, slug, organization repository and state branch and asks for no team ([selected payload](design/web-api-v1.md#selected-create-organization-payload)). One accepted transaction makes the organization, its HQ office, a top-level team named after it, the creator's person record as that team's lead and the built-in HR lead ([data contract](design/organization-data/01-organization-structure.md#creating-an-organization)). Code today: `organization.create` takes a name only; `organization.provision.v1` (`crates/org-proto/src/selected_bootstrap.rs`, `crates/org-host/src/provisioning.rs`) carries a home-team name and writes the organization, office, that team and an HR profile, but no creator person, and is reached only from tests. [browser tests] exercise the UI flow against fakes; [UI] states the web adapters are unavailable. |
| [ ] | FC-019 | [Office list, readiness and freshness](specs/offices-and-teams-ux.md) | In progress | Office/status refresh tests exist in [browser tests]; real browser data path remains gated [UI]. |
| [ ] | FC-020 | [Satellite invite, join approval and repair](specs/offices-and-teams-ux.md) | In progress | Status and repair-instruction UI exists, but Add office is disabled and the worker refuses non-HQ creation; invitation, approval, drain and repair integration remain. |
| [ ] | FC-021 | [Restart, relocate and remove office](specs/offices-and-teams-ux.md) | In progress | Native restart exists; authorized relocate/remove, safe cleanup and workspace preservation remain. |
| [ ] | FC-022 | [Updates, uninstall and instance lifecycle](specs/server-installation-and-access-ux.md) | In progress | Host instance registry/foreground lifecycle exists; update activation, uninstall and service setup remain. |
| [ ] | FC-023 | [Logs, doctor and support diagnostics](specs/server-installation-and-access-ux.md) | In progress | Owner-only capped/rotating log files exist; usable logs/doctor/support CLI and end-to-end redaction remain. |
| [ ] | FC-024 | [Fresh-HQ and satellite release acceptance](specs/dogfood-run.md) | Blocked | Depends on real installation, authenticated browser APIs and both-platform service and recovery validation. Signing setup and authentic signed-release verification come in the final phase; release code stays fail-closed. This does not block ordinary functional development. |
| [ ] | FC-153 | [Account administration through registration links](specs/server-installation-and-access-ux.md) | Planned | Accounts are administered in the browser, from People, by the server's one administrator or the organization's owner: each new account is created by a one-time registration link issued for an existing person, and the person chooses a username and password in the browser; a forgotten password is reset through a one-time link issued the same way. The only account command is `agentic-org admin reset`, for the administrator account. The administrator role and an organization's ownership are each handed over in the browser. Every account is tied to a person: in each organization it can open, to exactly one person record; never to an agent ([what the host owns](design/organization-data/STATUS.md#outside-this-set-what-the-host-owns)). Code today: account storage in `crates/org-host/src/hostdb` (`accounts.rs`, `first_account.rs`); `account.create` is defined in `crates/org-host-control` with no handler, and nothing issues registration links. [UI] claims no real sign-in or setup behavior. |
| [ ] | FC-154 | [Verified downloads and signed offline bundles](specs/server-installation-and-access-ux.md) | Planned | [Executable status] lists downloads, signed bundles and update activation as pending. Signing setup and authentic signed-release verification come in the final phase; release code stays fail-closed. |
| [ ] | FC-155 | [Browser CSP and production asset security](design/rust-browser-bff-and-static-sveltekit.md) | In progress | [UI] documents production-static CSP harness and browser security checks; authenticated production service remains blocked. |
| [ ] | FC-159 | [Stored browser sessions that survive restarts](adr/0013-stored-browser-sessions.md) | Planned | The host holds sessions in memory today and ends them with the control connection; the web process holds none (F4 removed its connection-bound session layer and answers the session routes with JSON 404 until this row lands); criteria SES-1 to SES-8 are unmet, and the absolute lifetime and per-account cap the decision removes are still in the code. Native CLI sessions stay connection-bound. |
| [ ] | FC-162 | [No application-level data encryption; office private keys wrapped](adr/0016-no-application-level-data-encryption.md) | In progress | The host registry is plain SQLite with no `registry` secret (`crates/org-host/src/hostdb.rs`, test `registry_is_plain_sqlite_that_opens_without_any_key`). The HQ CA private key is stored only as an `org_pki::wrap` record under the per-office key (column `wrapped_ca_key`; tests `org_pki::wrap::tests`, `enc5_ca_key_is_stored_only_wrapped_with_the_office_secrets_key`). The office databases still use SQLCipher. The per-office key becomes the office secrets key that wraps an office's private keys. Criteria ENC-1 to ENC-6 are unmet. |
| [ ] | FC-164 | [Linux and container hosting behind platform contracts](adr/0017-linux-and-container-hosting.md) | Planned | macOS and Linux are both v1, including an office inside a container. The host compiles on Linux and starts workers only on macOS: the check that a started worker is the intended program has a macOS version and a refusing one for every other system (`crates/org-host/src/supervisor/channels.rs`). The Linux admission, credential and confinement adapters are not written; criteria LNX-1 to LNX-6 are unmet. Design and increments X1a to X6 selected 2026-11-17 ([Linux host design](design/linux-host.md), implementation plan Track X). |

### Organization, people and teams

Category: [AREA-03 — scope and verification](FEATURE-AREAS.md#area-03).

Full requirements: [Organization, people and teams](specs/offices-and-teams-ux.md).

| Check | ID | Feature / short description | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| [ ] | FC-025 | [Team directory and organization chart](specs/offices-and-teams-ux.md) | In progress | Chart and view tests exist in [browser tests]; the single-home-team hierarchy, with one top-level team made at organization creation and project teams under it, and the live adapter remain [UI]. |
| [ ] | FC-026 | [Add, edit and remove people](specs/offices-and-teams-ux.md) | In progress | [People tests] cover UI behavior. A person is added directly (not through HR) in the top-level team or another team and moved like anyone else. Production adapter and single-home-team enforcement remain. |
| [ ] | FC-027 | [Create, move and archive teams](specs/offices-and-teams-ux.md) | In progress | [Team tests] cover UI behavior. Teams are created by HQ commands and, by default, with each project; the top-level team is made at organization creation. Live mutation, cycle checks and lead constraints remain. |
| [ ] | FC-028 | [Single home team and subtree lead authority](design/organization-data/01-organization-structure.md) | In progress | Team lead rules exist in `crates/org-office/src/rules`; the code keeps many-to-many membership (`memberships` table in `crates/org-office/src/store/org.rs`). A single home team is the rule; that membership code is removed, not migrated. |
| [ ] | FC-029 | [Reporting lines and accounts tied to persons](design/organization-data/04-agent-structure.md) | In progress | Every account is tied to a person: in each organization it can open, to exactly one person record; a person needs no account; an account is never tied to an agent. The owner is the person whose account created the organization, and their actions carry that person's ID ([data contract](design/organization-data/04-agent-structure.md#accounts-people-and-the-organization-owner)). Code today: reporting lines and cycle checks exist in `crates/org-office`; organization provisioning attributes records to `owner` with no person record (`crates/org-office/src/organization/bootstrap.rs`), and the host has no account-to-person link. |
| [ ] | FC-030 | [Project accountability and contributing teams](design/organization-data/02-project-structure.md) | In progress | Project-team links exist (not verified). Creating a project creates its owning team under the top-level team by default, or names an existing team ([data contract](design/organization-data/02-project-structure.md#creating-a-project)). Remaining: that default, explicit accountable owner versus contributing teams, and safe ownership transitions. |
| [ ] | FC-031 | [Built-in HR lead](specs/our-org-in-the-app.md) | Planned | The HR lead is built into the product: created with the organization as an employee of kind agent in the top-level team, it appears and is addressed as an agent, is not kept running and runs only when called ([data contract](design/organization-data/04-agent-structure.md#the-hr-lead-and-how-agents-are-onboarded)). Code today: organization provisioning writes a paused HR agent profile (`crates/org-office/src/organization/bootstrap.rs`), reached only from tests; nothing runs it. |
| [ ] | FC-032 | [Employee roster, cards and profiles](specs/our-org-in-the-app.md) | In progress | Roster/chart tests exist in [browser tests]; execution, activity and cost adapters remain separate. |
| [ ] | FC-033 | [Hire proposal, approval and provisioning](specs/our-org-in-the-app.md) | In progress | Agents are onboarded only through HR: HR drafts a proposal, a person approves it, then product code creates the employee, its home and its execution configuration; nothing exists before approval. Code today: a direct profile hire/upsert exists (`crates/org-office/src/employees/hire.rs`) and is replaced by this flow. Remaining: proposal, approval and provisioning. |
| [ ] | FC-034 | [Edit, move, pause, resume and offboard employees](specs/our-org-in-the-app.md) | In progress | Profile/status/offboarding mutations exist; actual execution stop/holds/fencing and safe workspace moves remain. |
| [ ] | FC-035 | [Home and unified action inbox](specs/our-org-in-the-app.md) | In progress | HQ problem-source inbox backend exists; owner Home/unified browser Inbox and complete source/action semantics remain. |
| [ ] | FC-036 | [Live activity and truthful employee presence](specs/our-org-in-the-app.md) | In progress | Organization activity/status and UI reducers exist; integrated cross-domain live timeline and production freshness remain. |
| [x] | FC-037 | [Retired existing-team import is gated](specs/our-org-in-the-app.md) | Done | Import removed from v1 per [UI]; [retired import test] records regression coverage. |

### Projects, tasks, bugs and requests

Category: [AREA-04 — scope and verification](FEATURE-AREAS.md#area-04).

Full requirements: [Projects, tasks, bugs and requests](specs/work-items-ux.md).

| Check | ID | Feature / short description | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| [ ] | FC-038 | [Project overview, work and team views](specs/work-items-ux.md) | In progress | Project/profile/work tests exist in [browser tests]; [UI] has no production project/work adapter claim. |
| [ ] | FC-039 | [Task board, list, detail and filters](specs/work-items-ux.md) | In progress | Tasks/work tests exist in [browser tests]; real selected-model mutations remain to verify. |
| [ ] | FC-040 | [Unified task, bug and request identity](design/organization-data/03-features-and-work.md) | In progress | Separate bug and request entities exist in the code; they are replaced by typed tasks outright, with no import mapping. Every task has a priority (urgent, high, normal, low) and may have an effort in points. |
| [ ] | FC-041 | [Accountable team and independent assignment](design/organization-data/12-task-assignment-and-history.md) | In progress | Individual assignment exists (`assignee_id` in `crates/org-work/src/model.rs`, `task.assign`); the independent team and person pair and accountable ownership remain. |
| [ ] | FC-042 | [Immutable reporter and task activity history](design/organization-data/12-task-assignment-and-history.md) | In progress | Immutable SQL task events and the reporter exist (`work_events` in `crates/org-office/src/store/work.rs`). Internal foundations exist: the acceptance journal (`crates/org-office/src/store/acceptance.rs`), codec-2 event integrity (`crates/org-work/src/selected_history.rs`) and lossless source editing (`crates/org-docs/src/source.rs`); no command reaches them. Remaining: the authorized history service on the selected sources. |
| [ ] | FC-043 | [Priority, status and task transition rules](specs/work-items-ux.md) | In progress | Task lifecycle validation exists in the code; selected unified transitions and authorized target adapters remain. |
| [ ] | FC-044 | [Report bugs with severity and reproduction](specs/work-items-ux.md) | In progress | Bug tests exist in [browser tests]; production adapter and selected task mapping remain open. |
| [ ] | FC-045 | [Duplicate linking and bug triage](specs/work-items-ux.md) | In progress | Bug triage/duplicate relationships exist; selected typed-task mapping and end-to-end browser integration remain. |
| [ ] | FC-046 | [Fix task, verification, close and reopen](specs/work-items-ux.md) | In progress | Fix, verify, close and reopen behavior exists in the code; selected history/assignment and live browser integration remain. |
| [ ] | FC-047 | [Capture and list feature requests](specs/feature-requests-ux.md) | In progress | Request tests exist in [browser tests]; production handler and selected task identity require integration. |
| [ ] | FC-048 | [Request routing and scoper selection](specs/feature-requests-ux.md) | In progress | Request routing/manual fallback UI and request behavior in the code exist; authorized live target routing remains. |
| [ ] | FC-049 | [Request approval, decline and merge](specs/feature-requests-ux.md) | In progress | Request outcome transitions exist in the code; selected typed identity, durable history and live adapters remain. |
| [ ] | FC-050 | [Plain task checklists](specs/work-items-ux.md) | In progress | Persisted plain checklist operations exist; selected source/identity and production browser integration remain. |
| [ ] | FC-051 | [Task output and completion evidence](specs/work-items-ux.md) | Planned | No dedicated Output view, deliverable resolver or shared completion explanation; generic links do not satisfy this feature. |
| [ ] | FC-052 | [Task work and waiting time](specs/work-items-ux.md) | In progress | Time and waiting computations exist in the code; revised reopen/baseline/overlap semantics and target live reads remain. |
| [ ] | FC-053 | [Author-private task notes](design/organization-data/13-private-task-notes.md) | Planned | Not implemented: the data contract defines the structure; runtime, access controls and UI do not exist. Shared comments do not satisfy privacy. |
| [ ] | FC-054 | [Shared comments, watchers and owner questions](design/organization-data/05-operational-data.md) | In progress | Shared comments/watch/question state exists; selected source/receipt durability and authorized browser integration remain. |
| [ ] | FC-055 | [Needs-me and owner-wait views](specs/work-items-ux.md) | In progress | Needs-you sample view and owner-wait primitives exist; authenticated live aggregation and selected scoping remain. |
| [ ] | FC-056 | [Areas and advanced checklist execution](specs/work-items-ux.md) | Deferred | Withdrawn: areas are not part of v1, and the `area.*` calls and tables in the code are deleted with the typed-task work; a task's accountable team supplies that context. Advanced checklist execution is not part of v1. |
| [ ] | FC-057 | [Undo and organization-wide event history](design/organization-data/README.md) | Deferred | Not part of v1; task history is a separate capability. |
| [ ] | FC-156 | [Following and requested-by tracking semantics](design/tasks-projects-docs.md) | Deferred | Section 7.2 gates AC-TK1–3 pending decisions and verified attribution; watcher data alone does not select these views. |
| [ ] | FC-157 | [Backlog ranking and planning lifecycle](design/tasks-projects-docs.md) | Deferred | Section 7.2 retains planning requirements pending lifecycle/storage decisions. |
| [ ] | FC-158 | [Subtask and cascade execution engine](design/tasks-projects-docs.md) | Deferred | Section 7.2 requires hierarchy/cascade decisions; simple checklist data does not implement this engine. |

### Feature definitions, knowledge and delivery

Category: [AREA-05 — scope and verification](FEATURE-AREAS.md#area-05).

Full requirements: [Feature definitions, knowledge and delivery](specs/features-ux.md).

| Check | ID | Feature / short description | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| [ ] | FC-058 | [Canonical feature definition and lifecycle](specs/features-ux.md) | Planned | No selected feature definition/lifecycle persistence found; draft schemas are not product storage. |
| [ ] | FC-059 | [Feature list, page and editing](specs/features-ux.md) | Planned | Project feature tab is gated; no feature list/page/editor service or live UI found. |
| [ ] | FC-060 | [Feature task membership and affected bugs](specs/features-ux.md) | Planned | No persisted feature membership/affected-bug service or selected progress calculation found. |
| [ ] | FC-061 | [Knowledge library, reader and Markdown editor](specs/decisions-and-tech-debt-ux.md) | In progress | Bounded Markdown/YAML parser exists; knowledge library, real reader/editor and scoped storage/publication remain. |
| [ ] | FC-062 | [Knowledge kinds, templates and scoped search](specs/decisions-and-tech-debt-ux.md) | Planned | No kind/template catalog or authorized title/ID/body search service found; parser has no kind awareness. |
| [ ] | FC-063 | [Project briefs and capability requirements](specs/decisions-and-tech-debt-ux.md) | Planned | No canonical project-brief/capability-requirement authoring/publication service found. |
| [ ] | FC-064 | [ADRs, designs, evaluations and Why links](specs/decisions-and-tech-debt-ux.md) | Planned | No application ADR/design/evaluation publication/history or Why-link behavior found. |
| [ ] | FC-065 | [Shortcut provenance and feature limitations](specs/decisions-and-tech-debt-ux.md) | Planned | v1 scope: task `details.shortcut` keeps tagged IDs without a debt subsystem, and each feature's known limitations are shown from its `feature.md` (LMT-1, LMT-2). Debt register and exception records are not in v1. No implementation found yet. |
| [ ] | FC-066 | [Agent document CLI and publication](specs/decisions-and-tech-debt-ux.md) | Planned | Agent CLI has no document/publication commands; do not treat UX command examples as implemented. |
| [ ] | FC-067 | [Releases, milestones and explicit content manifests](design/organization-data/09-release-content-and-flags.md) | Planned | No release/milestone/content-manifest service found; draft record schema only. |
| [ ] | FC-068 | [Release product changelogs](design/organization-data/09-release-content-and-flags.md) | Planned | No authored product release-changelog storage/service/UI found. |
| [ ] | FC-069 | [Deployment records, environments and rollback links](design/organization-data/07-release-deployments.md) | Planned | No deployment-record/rollback-link/actual-snapshot persistence service found; deployment runner is separate. |
| [ ] | FC-070 | [Flag definitions and intended/applied snapshots](design/organization-data/09-release-content-and-flags.md) | Planned | No flag definition/intent/applied-snapshot service found; evaluation/deployment engines remain separate. |
| [ ] | FC-071 | [Automatic feature transitions and release automation](specs/features-ux.md) | Deferred | Not part of v1; no lifecycle or deployment engine is selected. |
| [ ] | FC-173 | [Project documentation tree and index](design/organization-data/18-process-packages-and-project-documentation.md#project-documentation-tree) | Planned | Owner's [REQ-07](specs/req-07-composable-process-packages-and-team-blueprints.md). Project-wide documents, diagrams and images live in `projects/<id>/docs/` with sections and one `index.yaml` that the office writes with every accepted change; `agent-os doc index`, `ls`, `show`, `new`, `edit`, `move`, `put` and the Docs tab by section; an agent reaches only its own projects; one writing office per tree ([design](design/process-packages.md#6-the-project-documentation-tree), PKG-11, PKG-12, PKG-14, PPD-4, PPD-5, PPD-9). No tree, index or media code exists; `crates/org-docs` parses front matter only. |
| [ ] | FC-174 | [Issued document versions and differences](design/organization-data/18-process-packages-and-project-documentation.md#issued-versions) | Planned | Owner's [REQ-06](specs/req-06-desired-state-spec-authoring-and-reconciliation.md). A versioned document's working text is issued as version N, written once under `docs-history/`; readers get the latest issued version by default, with unissued changes, version list, sources and differences ([design](design/desired-state-specifications.md#6-the-contract), DSS-3, DSS-4, PPD-6, PPD-7). No code exists. Needs FC-173. |
| [ ] | FC-175 | [Code links and derived convergence of a version](design/organization-data/18-process-packages-and-project-documentation.md#code-links) | Planned | REQ-06. `code_links` on a document (repository, path, optional symbol) with a staleness check and the reverse lookup by file; a task's `realizes` names issued versions, and a version is converged when the tasks that realize it are finished (DSS-7, DSS-8, PPD-8, PPD-10). No code exists. Needs FC-174 and repositories (C7). |

### Chat and agent messaging

Category: [AREA-06 — scope and verification](FEATURE-AREAS.md#area-06).

Full requirements: [Chat and agent messaging](specs/chat-ux.md).

| Check | ID | Feature / short description | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| [x] | FC-072 | [Agent messaging command line over the file queue](../apps/agent-os/README.md) | Done | [Agent CLI] documents send/ask/reply, inbox/read/wait, defer/done/notify/due and relay over the file-queue transport, which the chat work deletes. |
| [x] | FC-073 | [Local mailbox delivery library slice](../crates/org-mailbox/README.md) | Done | [Mailbox] documents send/inbox/read/wait against in-memory testkit; no server/SQL/cross-office integration claimed. |
| [ ] | FC-074 | [Chat layout, threads and readable messages](specs/chat-ux.md) | In progress | Chat/followup/layout/accessibility tests exist in [browser tests]; production conversation adapter remains unavailable [UI]. |
| [ ] | FC-075 | [Protected conversation sources and memberships](design/organization-data/15-messaging-and-chat.md) | Planned | [Mailbox] lists target durable sources, channel/group admission and safe membership as follow-up adapters. |
| [ ] | FC-076 | [Channels, group DMs and member read state](specs/chat-ux.md) | Planned | [Mailbox] and [agent CLI] do not implement selected channel/group commands; fake UI is narrower evidence. |
| [ ] | FC-077 | [Mentions, search, attachments and reactions](specs/chat-ux.md) | In progress | Sample chat interactions exist; protected visibility-safe search/attachments/reactions and live adapters remain. |
| [ ] | FC-078 | [Drafts, history bounds and conversation performance](specs/chat-ux.md) | In progress | Sample drafts/history and bounded UI behavior exist; durable retention/pagination and real conversation performance remain. |
| [ ] | FC-079 | [Directed asks, answers and withdrawals](specs/chat-ux.md) | In progress | CLI asks and replies and sample cards exist; protected-source resolution and withdrawal remain. [REQ-04](specs/req-04-channel-messaging-and-question-components.md): the `ask_user` schema with choice, yes/no and free-text components, rendered with shadcn-svelte and Bits UI in the product's own chat. A default value is preselected, is handed to the agent with the timeout of an ask that is not required, and never answers a required ask. |
| [ ] | FC-166 | [Required asks: wait with a timeout, suspend the agent, resume on the answer](adr/0020-required-asks-suspend-and-resume.md) | Planned | An ask has no `required` field, no office timer for a suspended asker and no resume on an answer today; criteria ASK-1 to ASK-9 are unmet. Needs the agent launch path (FC-101). |
| [ ] | FC-080 | [Atomic task handoff and system notices](specs/chat-ux.md) | In progress | Sample recipient-only handoffs/notices exist; no atomic task/history/protected-message/receipt transaction. |
| [ ] | FC-081 | [Uninterrupting delivery, pause and waiting](specs/chat-ux.md) | In progress | SQL-backed office mailbox send/inbox/read/held-wait is wired; execution-aware wake/holds/noninterrupting delivery remain. |
| [ ] | FC-082 | [Live transport cutover with triage preservation](adr/0003-agent-messaging-office-first.md) | Deferred | Withdrawn: the development team's message queue is dropped and nothing is carried over. Not to be built. |
| [ ] | FC-083 | [External chat bridges](specs/chat-ux.md) | Deferred | Withdrawn: there are no external channels; the product's own chat is the only one. |
| [ ] | FC-160 | [Acknowledgement classes and the agent-os session mode](adr/0014-acknowledgement-classes-and-agent-os-latency.md) | Planned | Messaging and non-critical task activity, including creating a task, use the committed class, declared in the domain crates and honoured by the store adapter. Every write waits for a full flush and agent-os starts a process per call today; criteria ACK-1 to ACK-9 are unmet. Includes decision 9: a send relays after it answers and an ask always has a deadline. |

### Usage, costs and budgets

Category: [AREA-07 — scope and verification](FEATURE-AREAS.md#area-07).

Full requirements: [Usage, costs and budgets](specs/costs-and-insights-ux.md).

| Check | ID | Feature / short description | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| [ ] | FC-084 | [Immutable durable usage ledger and corrections](design/organization-data/10-usage-and-cost-accounting.md) | Planned | Only usage event types/fakes found; no real durable ledger, collector, protected archive or correction service. |
| [ ] | FC-085 | [Historical consumer, team, task and release attribution](design/organization-data/10-usage-and-cost-accounting.md) | Planned | No execution-time consumer/scope/historical-attribution capture service found. |
| [ ] | FC-086 | [Price versions, tokens, USD and coverage states](specs/costs-and-insights-ux.md) | Planned | Only token/USD event types/fakes found; no real capture, versioned pricing or coverage service. |
| [ ] | FC-087 | [Cost views, scoped permissions and CSV export](specs/costs-and-insights-ux.md) | Planned | No live ledger cost views, authorized amount reads or CSV export; request estimates are not recorded spend. |
| [ ] | FC-088 | [Period queries, bucketing and duration formatting](specs/costs-and-insights-ux.md) | Planned | No ledger period/bucket query or persisted account-zone service; generic formatting is insufficient. |
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
| [ ] | FC-101 | [Claude, Codex and Agy native runtime adapters](design/agent-sessions.md) | Planned | No native Claude/Codex/Agy adapters or office session runner; generic contracts/process primitives are separate. |
| [ ] | FC-102 | [Progress, health, waiting and stale-run detection](specs/employee-supervision-ux.md) | In progress | Process health/recovery primitives exist; real-session observation, stale-run detection and supervision view remain. |
| [ ] | FC-103 | [Per-task repository workspace isolation](design/organization-data/04-agent-structure.md) | Planned | No selected task/repository workspace admission or native sandbox enforcement; generic path/process primitives are separate. |
| [ ] | FC-104 | [On-call, scheduled and always-on run modes](specs/agent-run-modes-ux.md) | Deferred | Automatic scheduling/wake/sleep engine remains unselected. |
| [ ] | FC-105 | [Extra model-policy slots and role overrides](specs/agent-run-modes-ux.md) | Deferred | Additional selection/Why policy requires its own contract; data fields do not activate routing policy. |
| [ ] | FC-106 | [Goals, reminders and automatic memory consolidation](specs/employee-supervision-ux.md) | Deferred | Scheduling/memory engine remains separate pending selection. |
| [ ] | FC-107 | [Private specialists and child delegation engines](specs/employee-supervision-ux.md) | Deferred | Native delegate/replica orchestration requires separate selection and execution/accounting boundaries. |
| [ ] | FC-161 | [Selectable agent isolation with a dedicated OS user](adr/0015-agent-isolation-modes.md) | Planned | The launch contract has no isolation setting and agents share the service user; the macOS spike of decision 7 and criteria ISO-1 to ISO-9 are unmet. Includes the private agent home and the sign-in from the subscription pool (decisions 10 to 12). |

### Roles, processes and retrospectives

Category: [AREA-09 — scope and verification](FEATURE-AREAS.md#area-09).

Full requirements: [Roles, processes and retrospectives](specs/processes-ux.md).

| Check | ID | Feature / short description | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| [ ] | FC-108 | [Role records and employee role assignment](specs/roles-ux.md) | In progress | Employee free-text role field exists; normalized role catalog/assignment and selected persistence remain. |
| [ ] | FC-109 | [Role inheritance, overrides and drift approval](specs/roles-ux.md) | Deferred | Not part of v1; role records do not implement composition or injection. |
| [ ] | FC-110 | [Scoped process and standard documents](design/processes.md) | In progress | A process parser and helpers in `crates/org-process` and a sample catalog exist; scoped process and standard persistence remains. |
| [ ] | FC-111 | [Process catalog and editor UI](specs/processes-ux.md) | In progress | [Process tests] exercise UI; production data adapter and selected structural model remain to verify [UI]. |
| [ ] | FC-112 | [Process resolution, sealed overrides and agent injection](specs/processes-ux.md) | Planned | Resolution is organization, then the accountable team's chain, then the project's process package if it follows one (FC-170), then the project, with non-overridable documents always given to the agent and marked in its system instructions ([processes design](design/processes.md), decision 4). `crates/org-process` implements a multi-chain rule and nothing calls it. Mechanical enforcement and approvals are not part of v1. |
| [ ] | FC-113 | [Asks-first and full-autonomy policy engine](specs/processes-ux.md) | Deferred | Proposed behavior requires a selected authorization/execution contract. |
| [ ] | FC-168 | [Process package records and catalogue](design/process-packages.md#2-the-package) | Planned | Owner's [REQ-07](specs/req-07-composable-process-packages-and-team-blueprints.md). A package is a manifest, role definitions, role-bound skills, document kinds with templates and folders, as content with no engine ([document 18](design/organization-data/18-process-packages-and-project-documentation.md)); built-in packages ship with the product and are customized by copying; catalogue and editor in the browser (PKG-1 to PKG-3, PPD-1, PPD-2). No code exists. |
| [ ] | FC-169 | [A project's process and its seed roster](design/process-packages.md#3-a-projects-process) | Planned | REQ-07. The Process select on a project; choosing a package creates the missing roles and the declared documentation sections in one accepted change and starts nothing; the seed roster is derived and each seed role is filled by a hire or by an employee who has the role (PKG-4 to PKG-6, PKG-13, PPD-3). No code exists. Needs FC-168, projects (B3) and FC-173. |
| [ ] | FC-170 | [Role-bound skills in resolution and delivery](design/process-packages.md#4-how-skills-reach-an-agent) | Planned | REQ-07. A package's skills are a layer of process resolution between the team chain and the project, filtered by the agent's role, delivered as overridable guidance and never displacing a non-overridable document ([processes design](design/processes.md), PKG-7 to PKG-9, PROC-35). No code exists. Needs FC-112 and FC-168. |
| [ ] | FC-171 | [Role inspection and role-named hire requests by agents](design/process-packages.md#5-hiring-when-the-package-says-so) | Planned | REQ-07. `agent-os role list` and `role show` print a role and what a hire proposal needs; `agent-os hire request --role` asks HR for a hire into the project's team; approval by a person is unchanged (PKG-10). No `role` command exists. Needs FC-033 and FC-108. |
| [ ] | FC-172 | [Moving a process package between organizations](design/process-packages.md#2-the-package) | Planned | REQ-07 (portable package). Export and import of a package folder as an archive, last in the delivery order; the timing is a point for the owner. No code exists. |
| [ ] | FC-176 | [Built-in desired state specification package](design/desired-state-specifications.md) | Planned | Owner's [REQ-06](specs/req-06-desired-state-spec-authoring-and-reconciliation.md). The `dss` package: two seed roles, Product DSS and Technical DSS kinds, the product and technical sections, and the skills for specifying, deriving, planning and staffing, answering questions and reconciling (DSS-1 to DSS-11); DSS-5, DSS-6, DSS-9 and DSS-10 are checked with real agents. No content exists. Needs FC-168 to FC-171, FC-173 to FC-175, FC-177, asks (FC-079) and agent sessions. Running unattended also needs the owner's answer on whether an ask or an assignment starts an agent (FC-104). |
| [ ] | FC-177 | [Process and standard writes by agents for their own project](design/processes.md#5-changing-processes) | Planned | REQ-06 (standards as a byproduct). `agent-os process new` and `process edit` let an agent of a project's own team add process and standard documents at that project's scope and change the project's own, never replacing an inherited key, never at team or organization scope and never as not overridable (PROC-36). No write command exists. Needs FC-110. |
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
| [ ] | FC-139 | [Shared repository registration and project branches](design/projects-git-github.md) | Planned | No organization repository registration/per-project integration-branch association service found. |
| [ ] | FC-140 | [Local Git: office caches, task worktrees and push over plain remotes](design/projects-git-github.md) | Planned | No worktree, credential, push or PR adapters or commands found; path restrictions do not implement Git operations. v1 is plain Git with the office's own credentials against the selected layout; the design lists open questions to answer first. |
| [ ] | FC-163 | [Hosting-provider integrations: provider app, short-lived tokens, repository creation and pull requests](design/projects-git-github.md) | Deferred | v2. Split from FC-140. |
| [ ] | FC-141 | [Durable canonical records and indexed projections](design/organization-data/08-record-contracts.md) | In progress | SQLite work/event projection exists; selected canonical files, round-trip, revision/tombstone and restore adapters remain. |
| [ ] | FC-142 | [UUIDv7 IDs, HQ counters and office-minted task references](design/organization-data/06-identity-and-numbering.md) | In progress | Every ID is a UUIDv7; task references are minted by the creating office, one sequence per office, with a letter code (`T-B37`) ([ADR-0019](adr/0019-office-coded-task-numbers.md)); features and the other numbered records keep HQ allocation. Code today: organization provisioning mints UUIDv7 (`crates/org-host/src/provisioning.rs`); the rest validates ULIDs and keeps per-project T- counters. Remaining: UUIDv7 everywhere, office codes and counters, the reference parser. |
| [ ] | FC-143 | [Legacy data import mappings and staging](design/organization-data/README.md#no-compatibility-obligations-before-v1-ships) | Deferred | Withdrawn: nothing has shipped, so there is nothing to import. Not to be built. |
| [x] | FC-144 | [Replication codecs and storage-independent sync core](../crates/org-sync/README.md) | Done | [Sync core] documents implemented protocol/hash/authority, gap/retry/snapshot and receipt primitives; no network/filesystem I/O. |
| [ ] | FC-145 | [Durable replication and office continuity integration](design/organization-data/16-recovery-and-office-continuity.md) | In progress | [Sync core] exists; native source adapters, HQ ingest and protected source/recovery composition remain follow-up work. |
| [ ] | FC-146 | [Actor-bound mutation receipts and recovery journals](design/organization-data/16-recovery-and-office-continuity.md) | In progress | The acceptance journal and capture in `crates/org-office/src/store/acceptance.rs` implement frozen acceptance on the existing writer, receipts and publication/replay, with crash/reopen tests; no command reaches it. Remaining: protected host composition and the receipt schema of the data contract. |
| [ ] | FC-147 | [Protected chat, note, budget and usage restore](design/organization-data/16-recovery-and-office-continuity.md) | Planned | Target restore services remain work; Git clone or cache rebuild cannot restore these sources. |
| [ ] | FC-148 | [Credential rebind and process-safe cache rebuild](design/organization-data/16-recovery-and-office-continuity.md) | Planned | Implement durable authority/process reconciliation before execution; [supervisor] is only a lower-level primitive. |
| [ ] | FC-149 | [Automatic promotion and backup automation](design/organization-data/16-recovery-and-office-continuity.md) | Deferred | No automatic failover/backup engine selected by the record schema. |
| [ ] | FC-165 | [Replaceable storage engine](adr/0018-storage-engine-independence.md) | Planned | PostgreSQL must be possible later. SQL is used outside the store folder in two office files, one concrete store type is wired, and replication and messaging have engine-independent suites while work, organization and host storage do not (not verified); criteria ENG-1 to ENG-5 are unmet. |
| [ ] | FC-167 | [Open an existing organization from its files](design/organization-data/16-recovery-and-office-continuity.md#opening-an-existing-organization) | Planned | Owner decision 2026-11-17. On an instance with no record of an organization (the host's database lost, or a new server), the administrator opens it from its repository and state branch: the files are validated against the data contract, the host registers the organization under its existing ID, the office rebuilds its projection, the administrator becomes the owner as their person record, and people are invited again. Records kept only in protected local files come back only if `durable/` survived or from a backup. Nothing opens an organization from files today. |

### Session inspection and private memory

Category: [AREA-13 — scope and verification](FEATURE-AREAS.md#area-13).

Full requirements: [Session inspection and private memory](design/agent-sessions.md).

| Check | ID | Feature / short description | Status | Evidence / remaining work |
| --- | --- | --- | --- | --- |
| [ ] | FC-150 | [Inspect sessions and transcripts in browser](specs/our-org-in-the-app.md) | Planned | Sessions route is a placeholder; no authorized browser session or transcript inspector found. [REQ-05](specs/req-05-virtual-console-conversation-log-viewer.md): a read-only console built by parsing Claude and Codex conversation files, with tool-call and file-visualization toggles; a proof of concept against sample files comes first. |
| [ ] | FC-151 | [Authorized local CLI takeover and handback](design/agent-sessions.md) | Planned | [Supervisor] leaves PTYs for a separate delivery; verify exclusive takeover leases and safe handback before claiming full support. There is no terminal in the browser ([ADR-0004](adr/0004-headless-agent-sessions.md)); the session view is read-only and people use chat. |
| [ ] | FC-152 | [Private notes and memory profile reader](specs/our-org-in-the-app.md) | Deferred | Deferred human-only read API/UI, confined journal/MEMORY paths, caps and no-symlink/security checks; memory schema/retention remain pending. |

## Keeping this checklist current

Update the relevant row in the same change that changes implementation or selects/defers scope. Preserve row IDs; add a row when a separately deliverable capability is introduced. Assign it to exactly one primary category in the [area guide](FEATURE-AREAS.md), and update that guide's area totals with any status/category change. Cross-area dependencies belong in links or evidence, not duplicate feature rows. These are documentation categories, unrelated to the removed product Areas feature. Keep acceptance details in the linked owning spec. The lead maintains this index; the implementing area's owner supplies evidence and remaining work.

To mark Done, link the implementation commit/PR or current implementation documentation and the relevant verification result for the row's full scope, including real persistence, authorization and runtime integration where required. A merged PR, a draft schema or a fake-backed screen alone is insufficient. Record any remaining gap as In progress. If evidence is missing or contradictory, use Needs verification and state what must be checked. When implementation changes invalidate an earlier Done claim, reopen it.

Use this table to choose and reconcile work, then link an assigned issue/PR in its evidence cell; do not imply that every unchecked row is an active task. Needs verification rows require an implementation audit before estimating or rebuilding them. Deferred rows require the decision described in their owning contract before implementation. The feature status itself (for example planned/live/retired inside the product) is distinct from delivery status in this checklist.

Agent task logs and handovers are not completion evidence. The [documentation authority map](design/documentation-authority.md) resolves requirement precedence.

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
