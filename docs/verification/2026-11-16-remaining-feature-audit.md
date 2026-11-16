# Remaining 58 feature verification audit

Audited **2026-11-16**, against source and requirements at `555037f30259cf7dc27a8c2680822064b7fb4d96` on `ada/migrate-eden-emery-docs`. This continues the [first-five audit](2026-11-16-first-five-features.md). It covers **every one of the remaining 58 Needs verification rows** in the [feature checklist](../FEATURES.md).

## Outcome and method

| Assessed rows | In progress | Planned | Done | Still unclassified |
| ---: | ---: | ---: | ---: | ---: |
| 58 | 33 | 25 | 0 | 0 |

“In progress” means a meaningful implemented slice exists; it does not mean an agent is actively assigned. “Planned” means the selected capability is absent from the inspected application composition. Generic types, draft SQL, interfaces or loosely related helpers alone do not establish a partial product feature. These classifications establish delivery state and concrete gaps, not end-to-end acceptance. The total checklist now has **9 Done, 55 In progress, 51 Planned, 3 Blocked, 40 Deferred and 0 Needs verification** rows; completed scopes are unchanged.

The first pass used three **gpt-6-luna** subagents, divided into 17 organization/execution, 18 work/messaging and 23 knowledge/cost/Git items. Each traced its owning requirements into current source and tests. The lead checked escalations and disputed classifications directly, coordinated test execution, reconciled source/README conflicts and integrated this record. No product implementation was changed.

### RepoWise and Hindsight provenance

RepoWise **0.61.0** was queried through the installed bounded lookup in `/Users/vgmello/projects/agentic-org`. Its main graph reported HEAD `73a39c58d683b00ad8620d05f7d5ae10fdc17bc9` on `codex/v1-assignment-integration`, with refreshed working-tree source fingerprints. This is a **different checkout/branch** from the audited one. Queries located organization membership/policy, mailbox conformance and chat-model sources; those paths were checked here. Policy, mailbox-conformance and organization-model file bytes matched across the two checkouts when compared. Broad/empty graph results were not treated as proof that a feature or caller is absent. Some queries required refresh/retry; only completed results informed navigation.

🧠 From Hindsight memory (**Core concepts**, page `kp-5c497f4a65d94d289fea76face9fbfa2`, updated 2026-11-16; **Coordinated parallel delivery of the assignment feature**, page `kp-945a011478a44459a7e0cca938fcf384`, updated 2026-11-16): selected contracts are distinct from shipped behavior, and newer assignment integration was explicitly **IN_PROGRESS/UNACCEPTED** on another branch. Bank `coding-agent::agentic-org` reported synced with six pages and no active operations. These pages supplied navigation/history; current branch code and executed checks determined statuses. The migration worktree itself is outside the memory opt-in path; querying the configured main checkout required no global memory/configuration change.

### Lead adjudication of uncertain findings

| Question | Direct check and decision |
| --- | --- |
| Does legacy membership satisfy the selected home-team model? | `org-proto/src/org.rs` explicitly allows any number of teams; `org-office/src/rules/tests.rs` tests multi-team membership. Team/lead behavior is real but incompatible with required single home team: FC-028 stays partial. |
| Do generic task links implement Output? | The work link command records item/doc relations; no dedicated Output view, permitted-deliverable resolver or shared completion explanation was found. TSK-18 is a separate behavior: FC-051 is Planned. |
| Is the mailbox still an unwired in-memory library? | Its README is stale on this point. `org-office/src/messaging.rs` composes SQL MessageStore/MemberDirectory adapters, and worker startup/socket dispatch uses them, including held waits. FC-081 is partial for the remaining execution-aware delivery contract, not for missing basic SQL wiring. |
| Do runtime/spawn contracts implement native adapters or task isolation? | Workspace members contain generic runtime/supervise only; fake EchoRuntime is not a native driver, and selected task-worktree admission is not wired. FC-101/103 are Planned. Real generic process health observations support a narrower partial FC-102. |
| Do Usage fields/fake events implement ledger/pricing? | Rust semantic references lead to exports, event serialization/redaction and fake-session fixtures; no production collector, office aggregation or pricing service was found. FC-084/086 are Planned. |
| Are parser and SQL projection records meaningful partials? | org-docs actually parses bounded front matter/body; office code actually persists immutable work events/revisions. FC-061/141 remain partial, with missing selected storage/UI/canonical-source integration explicit. |
| Do logs, generic project-team links or activity establish the full target? | Rotation/file permissions, ProjectTeam relation and organization status/activity are concrete slices. They do not establish secret-safe support bundles, accountable-owner/contributor semantics or a cross-domain live feed: FC-023/030/036 remain partial. |

## Executed checks and limits

All commands used this migration checkout on macOS arm64, locked dependencies, temporary/test data and explicit fakes where required. No installed CLI, live organization, service, real credential store or GitHub resources were changed. Evidence/test files listed per feature were inspected; only commands in this table are claimed freshly executed for this audit.

| Command | Result | Scope / limitation |
| --- | --- | --- |
| `cargo test -p org-work --locked --lib` | 186 passed | Existing legacy/domain task, bug, request, checklist and history semantics; not selected-model migration. |
| `cargo test -p org-mailbox --locked --lib` | 96 passed | Mailbox library/conformance; no selected full chat/wake engine acceptance. |
| `cargo test -p org-docs --locked --lib` | 16 passed | Pure bounded Markdown/YAML parsing, no storage/editor. |
| `cargo test -p org-process --locked --lib` | 48 passed | Existing helper/parser behavior; does not select the deferred process engine. |
| `cargo test -p org-office --locked --lib` | **582 passed, 1 failed, 2 ignored** | Domain/storage tests ran; package is **not green**. See failure below. |
| `cargo test -p org-sync --locked --lib` | 141 passed | Current core protocol/state, not target canonical-file or restore adapters. |
| `cargo test -p org-runtime --locked` | 76 passed, 1 doctest ignored | Runtime contracts/unit tests and fake sessions, not actual vendor adapters. |
| `cargo test -p org-host --lib --locked logs::` | 2 passed | Rotating owner-only logs; no global content-redaction claim. |
| `cargo test -p agentic-org --test instances --locked` | 1 passed | Host instance primitives, not installation/update/uninstall acceptance. |
| `cargo test -p agentic-org --test contract --locked native_restart_follows_the_runtime_contract` | 1 passed | Native debug host restart using scripted worker fixtures, not authenticated browser restart/removal. |
| UI: `bun run test` | **80 files / 1,929 tests passed** | Current UI/unit model contracts, including samples; does not enable the production capability gate. |

The office failure reproduced with:

```sh
cargo test -p org-office --lib --locked store::sqlcipher_init_tests::sqlcipher_openers_and_clippy_overrides_keep_the_fence -- --exact
```

The guard failed at [opener_inventory.rs](../../crates/org-office/src/store/tests/opener_inventory.rs), line 51: `tracked input resolves to a directory (symlink or gitlink): .../.claude/skills/shadcn-svelte`. Git records that path as mode **120000**. The inventory rejects the existing directory symlink before source/config filtering. The isolated rerun had **0 passed / 1 failed**, matching the original failure. This audit did not change, skip or weaken the guard, delete the symlink or claim SQLCipher/security acceptance. The code/path are unchanged by the documentation commits. The source-guard failure remains unresolved and distinct from the feature implementation gaps below.

No new full browser, Linux, signed release/real Keychain, real runtime-provider or HQ/satellite end-to-end run is claimed. Earlier [first-five checks](2026-11-16-first-five-features.md#executed-verification) remain separately scoped. New statuses do not imply all tests passed or a production-ready release.

## Per-feature findings

Each feature appears once, under its [documented primary category](../FEATURE-AREAS.md). Source links are pinned to the inspected commit so later edits cannot silently change the audit evidence. Requirement links point to owning documents; their selection/deferment gates remain authoritative.

## Installation, access and office operations

### FC-021

[Restart, relocate and remove office](../specs/offices-and-teams-ux.md) — **In progress**.

**Verified slice:** Host can restart an offline/unavailable office after startup reconciliation; native restart outcomes are covered.

**Remaining work:** No relocation or authorized removal flow found. Restart does not prove process cleanup, placement fencing, data cleanup, or workspace preservation required by the selected removal flow.

**Requirement checks:** OFC-43–46 offices UX §6.4 organization-data 04 and 16.

**Source evidence:** [crates/org-host/src/offices.rs:243](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-host/src/offices.rs#L243), [apps/agentic-org/tests/contract.rs:207](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org/tests/contract.rs#L207), [docs/specs/offices-and-teams-ux.md:838](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/offices-and-teams-ux.md#L838).

**Test evidence inspected:** native_restart_follows_the_runtime_contract (apps/agentic-org/tests/contract.rs); the_give_up_rule_stops_restarts_until_office_restart (apps/agentic-org/tests/contract.rs)

### FC-022

[Updates, uninstall and instance lifecycle](../specs/server-installation-and-access-ux.md) — **In progress**.

**Verified slice:** Private host instance location/reservation/registry and foreground serve entry point exist.

**Remaining work:** CLI setup refuses because protected bootstrap and service registration are not wired. No update activation or uninstall/retained-data workflow in exposed CLI.

**Requirement checks:** SRV-33–35 installation/access UX setup and service criteria.

**Source evidence:** [apps/agentic-org/src/lib.rs:4](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org/src/lib.rs#L4), [crates/org-host/src/instance/mod.rs:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-host/src/instance/mod.rs#L1), [apps/agentic-org/tests/cli.rs:32](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org/tests/cli.rs#L32).

**Test evidence inspected:** unsupported_bootstrap_never_creates_partial_offices_or_service_state (apps/agentic-org/tests/cli.rs); apps/agentic-org/tests/instances.rs instance reservation tests

### FC-023

[Logs, doctor and support diagnostics](../specs/server-installation-and-access-ux.md) — **In progress**.

**Verified slice:** Host captures office stderr into owner-only, no-follow, capped rotating log files.

**Remaining work:** No exposed human logs/doctor/support command; CLI help only exposes serve/setup/help/version. No cross-component secret-redaction proof for support diagnostics; file permissions do not establish safe log contents.

**Requirement checks:** OFC-38–41 installation/access diagnostics and secret-safe support requirements.

**Source evidence:** [crates/org-host/src/logs.rs:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-host/src/logs.rs#L1), [apps/agentic-org/src/lib.rs:4](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org/src/lib.rs#L4), [docs/specs/offices-and-teams-ux.md:1070](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/offices-and-teams-ux.md#L1070).

**Test evidence inspected:** rotates_at_the_cap_and_keeps_one_old_file (crates/org-host/src/logs.rs); files_are_owner_only_and_reopen_at_their_size (crates/org-host/src/logs.rs)

## Organization, people and teams

### FC-028

[Single home team and subtree lead authority](../design/organization-data/01-organization-structure.md) — **In progress**.

**Verified slice:** Team tree, lead appointment, cycle prevention, ancestor/subtree authority checks, and multiple membership operations exist.

**Remaining work:** Current org identity is many-to-many Membership; selected contract requires exactly one home_team_id. No selected-model storage/admission/migration. UI sample contains employees with multiple teams.

**Requirement checks:** TEAM-2, TEAM-3, TEAM-5, TEAM-6, TEAM-15 organization-data 01 §Relationships organization-data 04 §Confirmed relationships.

**Source evidence:** [crates/org-office/src/organization/structure.rs:86](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/organization/structure.rs#L86), [crates/org-office/src/organization/policy.rs:164](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/organization/policy.rs#L164), [crates/org-proto/src/org.rs:476](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-proto/src/org.rs#L476), [apps/agentic-org-ui/src/lib/org/sample.ts:58](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/org/sample.ts#L58).

**Test evidence inspected:** apps/agentic-org-ui/src/lib/org/team-edit.test.ts; apps/agentic-org-ui/src/lib/org/reporting.test.ts; apps/agentic-org-ui/e2e/org-chart-teams-edit.spec.ts; a_teams_lead_must_be_a_member_and_never_the_owner; a_member_can_be_in_many_teams_and_duplicates_are_refused; moving_a_team_cannot_make_a_loop (crates/org-office/src/rules/tests.rs)

### FC-029

[Reporting lines and owner-linked employee](../design/organization-data/04-agent-structure.md) — **In progress**.

**Verified slice:** Current members have explicit reports_to_member_id with cycle checks and manager presentation; reporting-tree UI logic exists.

**Remaining work:** owner_bootstrap creates a teamless owner person; target linked owner employee, when present, has required home_team_id and remains distinct from authenticated account. Sample reporting tree does not establish a production browser data path.

**Requirement checks:** REPORT-1–5 OWNER-1–3 organization-data 04 §Confirmed relationships.

**Source evidence:** [crates/org-office/src/organization/people.rs:13](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/organization/people.rs#L13), [crates/org-office/src/organization/policy.rs:180](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/organization/policy.rs#L180), [crates/org-office/src/organization/directory.rs:298](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/organization/directory.rs#L298), [docs/specs/offices-and-teams-ux.md:1108](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/offices-and-teams-ux.md#L1108).

**Test evidence inspected:** apps/agentic-org-ui/src/lib/org/reporting.test.ts; reporting_lines_have_no_loops_and_the_owner_reports_to_no_one; a_new_reporting_line_raises_the_version_by_one (crates/org-office/src/rules/tests.rs)

### FC-030

[Project accountability and contributing teams](../design/organization-data/02-project-structure.md) — **In progress**.

**Verified slice:** Current org protocol/service manages project-to-team links; UI model presents participating teams.

**Remaining work:** Current generic project-team link does not implement selected owner_team_id distinct from optional contributing teams. No evidence here of atomic project-owner change updating task accountability/history.

**Requirement checks:** TEAM-8, TEAM-13–15 organization-data 02 §Relationships.

**Source evidence:** [crates/org-office/src/organization/structure.rs:246](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/organization/structure.rs#L246), [crates/org-proto/src/org.rs:509](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-proto/src/org.rs#L509), [apps/agentic-org-ui/src/lib/org/sample.ts:77](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/org/sample.ts#L77), [docs/design/organization-data/02-project-structure.md:124](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/02-project-structure.md#L124).

**Test evidence inspected:** several_teams_can_share_a_project_and_links_are_their_own_entities (crates/org-office/src/rules/tests.rs); apps/agentic-org-ui/src/lib/org/projects.test.ts; apps/agentic-org-ui/e2e/projects.spec.ts

### FC-031

[Built-in HR identity and home team](../specs/our-org-in-the-app.md) — **Planned**.

**Verified slice:** General owner/person bootstrap exists, but it is teamless and no built-in HR profile was found.

**Remaining work:** No built-in HR identity with stable handle and selected execution settings. No bootstrap flow requiring an explicitly chosen valid HR home team.

**Requirement checks:** HRL-1–8 SRV-36 TEAM-3.

**Source evidence:** [crates/org-office/src/organization/people.rs:13](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/organization/people.rs#L13), [apps/agentic-org-ui/src/lib/org/sample.ts:31](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/org/sample.ts#L31), [docs/specs/our-org-in-the-app.md:373](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/our-org-in-the-app.md#L373).

**Test evidence inspected:** No HR bootstrap test found; apps/agentic-org-ui/e2e/agents-roster.spec.ts is fixture roster coverage only.

### FC-033

[Hire proposal, approval and provisioning](../specs/our-org-in-the-app.md) — **In progress**.

**Verified slice:** Office domain validates agent hire fields and persists MemberUpserted; UI hire/profile flow has fake-backed tests.

**Remaining work:** Direct hire path has no approved proposal check, including direct callers. No selected home_team_id or role_id validation; record creation is not real runtime provisioning.

**Requirement checks:** STAFF-1–8 HRL-3–4 SRV-30 organization-data 04.

**Source evidence:** [crates/org-office/src/employees/hire.rs:13](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/employees/hire.rs#L13), [apps/agentic-org-ui/src/lib/org/api.ts:85](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/org/api.ts#L85), [docs/specs/our-org-in-the-app.md:342](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/our-org-in-the-app.md#L342).

**Test evidence inspected:** employee command tests under crates/org-office/src/employees; apps/agentic-org-ui/src/lib/org/agents.test.ts; apps/agentic-org-ui/e2e/org-chart-agents.spec.ts (fake-backed)

### FC-034

[Edit, move, pause, resume and offboard employees](../specs/our-org-in-the-app.md) — **In progress**.

**Verified slice:** Current model has revision-checked profile changes, active/paused status and offboarding record transitions; UI includes edit/pause/offboard flows.

**Remaining work:** Current home field is optional filesystem path, not selected home_team_id move. Record mutations do not prove actual runtime stop/cleanup, independent hold reconciliation, or safe workspace transfer/preservation.

**Requirement checks:** PROF-1 STAFF-8 TEAM-6 PEOPLE-5 EMP-13 execution/recovery contracts 11 and 16.

**Source evidence:** [crates/org-office/src/employees/update.rs:9](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/employees/update.rs#L9), [crates/org-office/src/employees/lifecycle.rs:10](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/employees/lifecycle.rs#L10), [apps/agentic-org-ui/src/lib/org/agents.ts:46](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/org/agents.ts#L46), [docs/specs/our-org-in-the-app.md:383](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/our-org-in-the-app.md#L383).

**Test evidence inspected:** an_agent_is_edited_field_by_field_and_each_edit_raises_the_version; an_agent_is_paused_and_resumed_and_only_to_those_two; offboarding_ends_an_agents_relations_and_keeps_its_record_and_its_handle (crates/org-office/src/rules/tests.rs); apps/agentic-org-ui/src/lib/org/reducer.test.ts; apps/agentic-org-ui/e2e/org-chart-agents.spec.ts

### FC-035

[Home and unified action inbox](../specs/our-org-in-the-app.md) — **In progress**.

**Verified slice:** HQ inbox list merges sources and supports list/count/filter/Handled queries; org source contributes an unplaced-agent Problem. No owner Home/browser Inbox.

**Remaining work:** Home is not in navigation; `/home`/`/inbox` capability is absent and generic sections show Coming soon. No unified durable source aggregation or clear/Handled semantics across approvals, questions, problems and actions.

**Requirement checks:** LIVE-4 our-org-in-the-app §6.1–6.2 APPR-1–10 for approval source semantics.

**Source evidence:** [apps/agentic-org-ui/src/lib/nav.ts:11](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/nav.ts#L11), [apps/agentic-org-ui/src/routes/(shell)/[section=section]/+page.svelte:9](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/routes/(shell)/[section=section]/+page.svelte#L9), [apps/agentic-org-office/src/inbox/list.rs:9](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-office/src/inbox/list.rs#L9), [apps/agentic-org-office/src/inbox/org_source.rs:24](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-office/src/inbox/org_source.rs#L24), [docs/specs/our-org-in-the-app.md:195](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/our-org-in-the-app.md#L195).

**Test evidence inspected:** apps/agentic-org-office/src/inbox/org_source/tests.rs; apps/agentic-org-office/src/inbox/tests.rs; apps/agentic-org-office/src/inbox/hint/tests.rs; no Home/integrated owner Inbox UI test found.

### FC-036

[Live activity and truthful employee presence](../specs/our-org-in-the-app.md) — **In progress**.

**Verified slice:** Office records org-change activity; current status/presence helpers and UI live-feed reducers exist.

**Remaining work:** No integrated Activity page or merged work/messages/session/git/org timeline. No production browser org adapter; sample/fake live changes do not prove real data freshness or cross-domain presence.

**Requirement checks:** PROF-3 LIVE-1–3 LIVE-6–8 EMP-13.

**Source evidence:** [crates/org-office/src/organization/activity.rs:10](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/organization/activity.rs#L10), [crates/org-office/src/organization/status.rs:81](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/organization/status.rs#L81), [apps/agentic-org-ui/src/lib/org/live.ts:22](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/org/live.ts#L22), [apps/agentic-org-ui/src/lib/org/api.ts:201](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/org/api.ts#L201), [apps/agentic-org-ui/src/routes/(shell)/[section=section]/+page.svelte:9](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/routes/(shell)/[section=section]/+page.svelte#L9).

**Test evidence inspected:** apps/agentic-org-ui/src/lib/org/live.test.ts; apps/agentic-org-ui/src/lib/org/reporting.test.ts; crates/org-office/src/organization/status.rs tests; crates/org-office/src/store/org activity tests

## Projects, tasks, bugs and requests

### FC-040

[Unified task, bug and request identity](../design/organization-data/03-features-and-work.md) — **In progress**.

**Verified slice:** The current work domain and office SQLite store support task, bug and request variants, stable item IDs, and transactionally allocated numbers. Tasks use T-n, but bugs use a separate B-n sequence; the old request flow is only a UI sample. Existing server data is not migrated to the selected single typed-task/T- model.

**Remaining work:** Implement selected shared T- identity and project/team scopes for all types; retain IDs and import old B-/REQ- references through explicit collision-aware mappings; wire the browser to the owning service.

**Requirement checks:** TSK-12 TSK-16 WORK-4 WORK-8 RQS-20.

**Source evidence:** [crates/org-work/src/lib.rs:19](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/lib.rs#L19), [crates/org-work/src/lib.rs:32](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/lib.rs#L32), [crates/org-office/src/store/work/metadata.rs:106](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/store/work/metadata.rs#L106), [docs/design/organization-data/03-features-and-work.md:8](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/03-features-and-work.md#L8), [apps/agentic-org-ui/src/lib/work/index.ts:7](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/work/index.ts#L7).

**Test evidence inspected:** crates/org-work/src/tests/numbering.rs; crates/org-office/src/store/work/tests/metadata.rs; apps/agentic-org-ui/src/lib/work/engine.test.ts; apps/agentic-org-ui/src/lib/work/requests.test.ts (test model only)

### FC-041

[Accountable team and independent assignment](../design/organization-data/12-task-assignment-and-history.md) — **In progress**.

**Verified slice:** The legacy work service stores an accountable team for task records and supports nullable individual assignment with expected-version conflict checks and assignment events. The selected nullable assigned-team plus individual pair is absent.

**Remaining work:** Add assigned_team_id alongside assignee_id; validate both against active same-organization records; write both old/new values atomically; migrate/retain existing assignments; connect target service/UI.

**Requirement checks:** TSK-12 TSK-13 TRI-2 TRI-3 TRI-6.

**Source evidence:** [crates/org-work/src/model.rs:26](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/model.rs#L26), [crates/org-work/src/model.rs:76](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/model.rs#L76), [crates/org-work/src/tasks.rs:292](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/tasks.rs#L292), [docs/design/organization-data/12-task-assignment-and-history.md:8](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/12-task-assignment-and-history.md#L8).

**Test evidence inspected:** crates/org-work/src/tests/conflicts.rs; crates/org-work/src/tests/activity.rs; apps/agentic-org-ui/src/lib/work/engine.test.ts

### FC-042

[Immutable reporter and task activity history](../design/organization-data/12-task-assignment-and-history.md) — **In progress**.

**Verified slice:** The current work writer appends versioned events with immutable SQL triggers; reporter identity comes from the admitted creator and is distinct from current assignee. Tombstones and event reads exist in the legacy model.

**Remaining work:** Selected organization-Git event files, crash-recoverable task/event/receipt commits, import baselines/unknown history handling, and preservation of selected history after archive need the target migration. Current office SQLite projection is not the selected durable source contract.

**Requirement checks:** TSK-14 TSK-15 WORK-7 RQS-10.

**Source evidence:** [crates/org-work/src/model.rs:38](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/model.rs#L38), [crates/org-office/src/store/work.rs:35](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/store/work.rs#L35), [crates/org-office/src/store/work.rs:44](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/store/work.rs#L44), [docs/design/organization-data/05-operational-data.md:240](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/05-operational-data.md#L240), [docs/design/organization-data/12-task-assignment-and-history.md:31](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/12-task-assignment-and-history.md#L31).

**Test evidence inspected:** crates/org-office/src/store/work/tests/activity.rs; crates/org-work/src/tests/activity.rs; crates/org-work/src/tests/idempotency.rs

### FC-043

[Priority, status and task transition rules](../specs/work-items-ux.md) — **In progress**.

**Verified slice:** The domain implements task and bug state machines, expected-version conflicts, status events, and required-step guards. These are current legacy product rules with permissive handlers, not a completed selected authorization/UI path.

**Remaining work:** Reconcile selected lifecycle and permission rules and T- model; complete production browser/service integration, live updates and target actor authorization. Keep old lifecycle requirements explicitly separated where deferred.

**Requirement checks:** TSK-1 TSK-2 TSK-3 TSK-4 TSK-5 TSK-7 WORK-7.

**Source evidence:** [crates/org-work/src/lib.rs:19](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/lib.rs#L19), [crates/org-work/src/lib.rs:32](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/lib.rs#L32), [crates/org-work/src/tasks.rs:342](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/tasks.rs#L342), [docs/specs/work-items-ux.md:280](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/work-items-ux.md#L280), [apps/agentic-org-ui/src/lib/work/index.ts:7](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/work/index.ts#L7).

**Test evidence inspected:** crates/org-work/src/tests/state_machine.rs; crates/org-work/src/tests/conflicts.rs; apps/agentic-org-ui/src/lib/work/engine.test.ts

### FC-045

[Duplicate linking and bug triage](../specs/work-items-ux.md) — **In progress**.

**Verified slice:** Legacy bug triage, duplicate close resolution, and reopen behavior are present in the Rust domain. A TypeScript sample work engine also models duplicate references and UI suggestions.

**Remaining work:** Reconcile duplicate provenance against selected task links/details and explicit human choices; integrate triage with selected assignments and target permissions. The selected browser adapter is unavailable and legacy B- identity remains.

**Requirement checks:** BUG-3 TRI-1 TRI-2 TRI-6 FIX-7.

**Source evidence:** [crates/org-work/src/bugs.rs:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/bugs.rs#L1), [crates/org-work/src/bugs.rs:48](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/bugs.rs#L48), [crates/org-work/src/lib.rs:32](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/lib.rs#L32), [apps/agentic-org-ui/src/lib/work/bugs.test.ts:55](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/work/bugs.test.ts#L55), [apps/agentic-org-ui/src/lib/work/index.ts:7](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/work/index.ts#L7).

**Test evidence inspected:** crates/org-work/src/tests/bugs.rs; crates/org-office/src/store/work/tests/assembly.rs; apps/agentic-org-ui/src/lib/work/bugs.test.ts

### FC-046

[Fix task, verification, close and reopen](../specs/work-items-ux.md) — **In progress**.

**Verified slice:** Rust supports fix links, bug mark-fixed, verify, close and reopen, with automated fix-task event cascades; UI fake work tests include a lifecycle story.

**Remaining work:** Target transition semantics and selected immutable reporter/history mapping remain to reconcile; current lifecycle is not the fully selected UX/service contract, and production browser adapters are unavailable. Do not treat deferred auto-propagation/Undo requirements as selected acceptance.

**Requirement checks:** FIX-1 FIX-3 FIX-4 FIX-5 FIX-6 FIX-9 TSK-14.

**Source evidence:** [crates/org-work/src/lib.rs:32](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/lib.rs#L32), [crates/org-work/src/lib.rs:37](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/lib.rs#L37), [crates/org-work/src/fixes.rs:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/fixes.rs#L1), [crates/org-work/src/bugs.rs:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/bugs.rs#L1), [docs/specs/work-items-ux.md:339](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/work-items-ux.md#L339).

**Test evidence inspected:** crates/org-work/src/tests/fix_tasks.rs; crates/org-work/src/tests/bugs.rs; apps/agentic-org-ui/src/lib/work/engine.test.ts

### FC-048

[Request routing and scoper selection](../specs/feature-requests-ux.md) — **In progress**.

**Verified slice:** The browser sample has leading-mention routing, suggestion ranking, ambiguity handling and labeled routing copy, with tests for mention parsing and alternatives.

**Remaining work:** Requests are not a current server-backed work type: the production work API is unavailable and has requests=false. Selected request task/assignment semantics, rules-backed authorized suggestions and explicit manual fallback need live service integration.

**Requirement checks:** RQS-3 RQS-4 RQS-5 RQS-8 RQS-19.

**Source evidence:** [apps/agentic-org-ui/src/lib/work/requests-view.ts:256](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/work/requests-view.ts#L256), [apps/agentic-org-ui/src/lib/work/requests-view.test.ts:78](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/work/requests-view.test.ts#L78), [apps/agentic-org-ui/src/lib/work/requests-view.test.ts:233](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/work/requests-view.test.ts#L233), [apps/agentic-org-ui/src/lib/work/index.ts:7](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/work/index.ts#L7), [apps/agentic-org-ui/README.md:7](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/README.md#L7).

**Test evidence inspected:** apps/agentic-org-ui/src/lib/work/requests-view.test.ts; apps/agentic-org-ui/src/lib/work/requests.test.ts (test model only)

### FC-049

[Request approval, decline and merge](../specs/feature-requests-ux.md) — **In progress**.

**Verified slice:** The UI-only request engine models routing, scoping, approval, decline and merge, and tests lifecycle/event and idempotency cases.

**Remaining work:** No Rust request mutation service or production browser adapter is wired; `workApi.requests` is false. Move request to selected typed task identity and implement durable task/event/receipt outcomes and authorized human choices.

**Requirement checks:** RQS-9 RQS-10 RQS-12 RQS-16 RQS-20.

**Source evidence:** [apps/agentic-org-ui/src/lib/work/requests.test.ts:186](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/work/requests.test.ts#L186), [apps/agentic-org-ui/src/lib/work/requests.test.ts:429](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/work/requests.test.ts#L429), [apps/agentic-org-ui/src/lib/work/requests.test.ts:493](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/work/requests.test.ts#L493), [apps/agentic-org-ui/src/lib/work/requests.test.ts:503](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/work/requests.test.ts#L503), [apps/agentic-org-ui/src/lib/work/index.ts:7](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/work/index.ts#L7), [docs/specs/feature-requests-ux.md:123](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/feature-requests-ux.md#L123).

**Test evidence inspected:** apps/agentic-org-ui/src/lib/work/requests.test.ts (sample state machine only)

### FC-050

[Plain task checklists](../specs/work-items-ux.md) — **In progress**.

**Verified slice:** The Rust work domain and SQLite projection persist checklist steps and check/order changes on tasks and bugs; API/UI fake has basic checklist behavior.

**Remaining work:** Target browser adapter is unavailable and selected TSK typed-task migration is pending. Verify the delivered plain-step UI against CHK-1..4 and keep CHK-5/6/8/9 advanced execution explicitly deferred.

**Requirement checks:** CHK-1 CHK-2 CHK-3 CHK-4 TSK-15.

**Source evidence:** [crates/org-work/src/checklist.rs:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/checklist.rs#L1), [crates/org-work/src/model.rs:52](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/model.rs#L52), [crates/org-work/src/lib.rs:22](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/lib.rs#L22), [apps/agentic-org-ui/src/lib/work/index.ts:7](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/work/index.ts#L7), [docs/specs/work-items-ux.md:294](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/work-items-ux.md#L294).

**Test evidence inspected:** crates/org-work/src/tests/checklist.rs; crates/org-office/src/store/work/tests/call.rs; apps/agentic-org-ui/src/lib/work/engine.test.ts

### FC-051

[Task output and completion evidence](../specs/work-items-ux.md) — **Planned**.

**Verified slice:** Generic typed item/document links and task branch fields are implemented as reusable data; direct source review found no dedicated Output view, output/deliverable capture, repository evidence resolver, or shared completion-explanation feature.

**Remaining work:** Implement selected Output read view and only its permitted resolvable recorded references plus shared completion explanation/unavailable state; do not infer outputs from Done, activity, arbitrary links or private data. Generic links are a foundation only, not a partial delivery of this feature row.

**Requirement checks:** TSK-8 TSK-18.

**Source evidence:** [docs/specs/work-items-ux.md:205](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/work-items-ux.md#L205), [docs/specs/work-items-ux.md:207](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/work-items-ux.md#L207), [docs/specs/work-items-ux.md:209](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/work-items-ux.md#L209), [docs/specs/work-items-ux.md:398](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/work-items-ux.md#L398), [crates/org-proto/src/work.rs:440](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-proto/src/work.rs#L440), [crates/org-proto/src/work.rs:603](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-proto/src/work.rs#L603), [apps/agentic-org-ui/src/lib/work/index.ts:7](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/work/index.ts#L7).

**Test evidence inspected:** No dedicated Output component/service/model test found by source search. Generic links are covered by `crates/org-work/src/tests/refs.rs`; the UI work API remains unavailable.

### FC-052

[Task work and waiting time](../specs/work-items-ux.md) — **In progress**.

**Verified slice:** Work events and owner-question waits feed computed lead/status/waiting time figures; bug triage waiting is also represented.

**Remaining work:** No actual working-time logger is implemented (time not spent in other statuses remains rest/untracked); target baseline/unavailable history treatment and real UI read integration remain. Calendar age must not be described as measured work.

**Requirement checks:** TSK-11 WORK-5.

**Source evidence:** [crates/org-work/src/lib.rs:30](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/lib.rs#L30), [crates/org-work/src/waiting.rs:62](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/waiting.rs#L62), [crates/org-work/src/service.rs:61](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/service.rs#L61), [docs/specs/work-items-ux.md:197](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/work-items-ux.md#L197), [apps/agentic-org-ui/src/lib/work/time.ts:22](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/work/time.ts#L22).

**Test evidence inspected:** crates/org-work/src/tests/time.rs; crates/org-work/src/tests/triage_wait.rs; apps/agentic-org-ui/src/lib/work/time.test.ts

### FC-054

[Shared comments, watchers and owner questions](../design/organization-data/05-operational-data.md) — **In progress**.

**Verified slice:** The current work service has shared comments, owner questions/answers, watchers and durable SQL-projected item/event writes; question comments open/close waiting spans.

**Remaining work:** The selected shared comment/question/watcher source and recovery contract still requires target migration; no production browser API is wired. Keep author-private notes separate; they are explicitly unimplemented.

**Requirement checks:** RQS-10 TSK-15 organization-data/05 shared comments, watchers, owner-questions and waiting-spans contract (no separate AC ID).

**Source evidence:** [crates/org-work/src/lib.rs:25](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/lib.rs#L25), [crates/org-work/src/items.rs:22](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/items.rs#L22), [crates/org-work/src/waiting.rs:62](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/waiting.rs#L62), [docs/design/organization-data/05-operational-data.md:19](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/05-operational-data.md#L19), [apps/agentic-org-ui/src/lib/work/index.ts:7](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/work/index.ts#L7).

**Test evidence inspected:** crates/org-work/src/tests/activity.rs; crates/org-work/src/tests/triage_wait.rs; apps/agentic-org-ui/src/lib/work/engine.test.ts

### FC-055

[Needs-me and owner-wait views](../specs/work-items-ux.md) — **In progress**.

**Verified slice:** The UI request sample computes a needs-you queue, and the Rust work service exposes current owner-question waits and activity read pieces.

**Remaining work:** There is no live authenticated reader-scoped Needs-me/owner-wait aggregation; the selected target forbids inferring employee assignment from organization ownership, and production work adapter remains unavailable.

**Requirement checks:** WORK-2 WORK-3 WORK-10 AC-IA18-1–3 RQS-7 RQS-15.

**Source evidence:** [apps/agentic-org-ui/src/lib/work/requests-view.ts:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/work/requests-view.ts#L1), [apps/agentic-org-ui/src/lib/work/requests-view.test.ts:34](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/work/requests-view.test.ts#L34), [crates/org-work/src/model.rs:63](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/model.rs#L63), [crates/org-work/src/waiting.rs:62](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/waiting.rs#L62), [apps/agentic-org-ui/src/lib/work/index.ts:7](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/work/index.ts#L7).

**Test evidence inspected:** apps/agentic-org-ui/src/lib/work/requests-view.test.ts; crates/org-work/src/tests/triage_wait.rs

## Feature definitions, knowledge and delivery

### FC-058

[Canonical feature definition and lifecycle](../specs/features-ux.md) — **Planned**.

**Verified slice:** No canonical feature entity/file service or lifecycle authoring found. Copilot capability descriptors are unrelated; task feature membership is rejected as a later phase.

**Remaining work:** Implement canonical feature.md read/write/index, stable IDs/F- allocation, owner/status validation, revision/history, selected lifecycle facts and projection rebuild; automatic lifecycle remains deferred.

**Requirement checks:** FTR-1 FTR-5 FTR-8 FTR-9 FTR-13 FTR-14.

**Source evidence:** [docs/specs/features-ux.md:29](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/features-ux.md#L29), [docs/specs/features-ux.md:112](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/features-ux.md#L112), [crates/org-work/src/tasks.rs:54](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/tasks.rs#L54), [crates/org-copilot/src/feature.rs:53](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-copilot/src/feature.rs#L53).

**Test evidence inspected:** No product feature-record tests found.

### FC-059

[Feature list, page and editing](../specs/features-ux.md) — **Planned**.

**Verified slice:** Project page serves Overview/Tasks/Bugs; other tabs are disabled and features/docs are identified as later release. No feature list/page/editor route or component exists.

**Remaining work:** Build feature list, filters/search, detail, Markdown editor, save/conflict/stale states, typed relationship views, authorized cost/time and revision history.

**Requirement checks:** FTR-6 FTR-10 FTR-11 FTR-13 FTR-14.

**Source evidence:** [apps/agentic-org-ui/src/routes/(shell)/projects/[id]/+page.svelte:49](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/routes/(shell)/projects/[id]/+page.svelte#L49), [apps/agentic-org-ui/src/routes/(shell)/projects/[id]/+page.svelte:53](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/routes/(shell)/projects/[id]/+page.svelte#L53), [apps/agentic-org-ui/src/routes/(shell)/projects/[id]/+page.svelte:62](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/routes/(shell)/projects/[id]/+page.svelte#L62), [docs/specs/features-ux.md:94](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/features-ux.md#L94), [docs/specs/features-ux.md:95](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/features-ux.md#L95).

**Test evidence inspected:** No feature editor/page UI tests found.; Svelte MCP autofixer on the project route (Svelte 5.57.3): no issues or suggestions.

### FC-060

[Feature task membership and affected bugs](../specs/features-ux.md) — **Planned**.

**Verified slice:** Model has task.feature_id and bug.affects_feature_id-shaped fields, but create/update handlers reject these as later phase; no membership controls/confirmation or derived feature progress.

**Remaining work:** Implement feature validation, same-project typed links, confirm-before-move, history, affected-bug distinction and derived task progress.

**Requirement checks:** FTR-4 FTR-6 FTR-12.

**Source evidence:** [crates/org-work/src/model.rs:63](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/model.rs#L63), [crates/org-work/src/model.rs:85](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/model.rs#L85), [crates/org-work/src/tasks.rs:54](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/tasks.rs#L54), [crates/org-work/src/tasks.rs:209](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/tasks.rs#L209), [crates/org-proto/src/work.rs:1268](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-proto/src/work.rs#L1268), [crates/org-proto/src/work.rs:1392](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-proto/src/work.rs#L1392).

**Test evidence inspected:** org-work item/bug tests cover existing work engine, not feature links.

### FC-061

[Knowledge library, reader and Markdown editor](../specs/decisions-and-tech-debt-ux.md) — **In progress**.

**Verified slice:** org-docs provides a bounded Markdown/YAML front-matter parser; it explicitly performs no I/O and knows no document kind. Knowledge UI/storage and project Docs tab are unavailable.

**Remaining work:** Add canonical scoped organization-Git storage, reader/editor, project-required scope validation, revisions/publication, authorization and real indexing.

**Requirement checks:** DEC-1 DEC-2 DEC-3 AUTH-1 AUTH-3.

**Source evidence:** [crates/org-docs/src/lib.rs:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-docs/src/lib.rs#L1), [crates/org-docs/src/lib.rs:8](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-docs/src/lib.rs#L8), [crates/org-docs/src/lib.rs:125](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-docs/src/lib.rs#L125), [apps/agentic-org-ui/src/routes/(shell)/projects/[id]/+page.svelte:53](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/routes/(shell)/projects/[id]/+page.svelte#L53), [docs/specs/decisions-and-tech-debt-ux.md:76](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/decisions-and-tech-debt-ux.md#L76).

**Test evidence inspected:** org-docs parser tests exist; they do not test storage/UI.

### FC-062

[Knowledge kinds, templates and scoped search](../specs/decisions-and-tech-debt-ux.md) — **Planned**.

**Verified slice:** Generic parser accepts Markdown/YAML front matter but has no kind decoding, templates, scoped search/index or UI route.

**Remaining work:** Implement selected kinds/templates and permitted title/ID/key/body search with scope/status/freshness/coverage; specialized evaluation/limitation/exception kinds remain deferred.

**Requirement checks:** DEC-1 DEC-2 DEC-3 WHY-4.

**Source evidence:** [crates/org-docs/src/lib.rs:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-docs/src/lib.rs#L1), [crates/org-docs/src/lib.rs:3](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-docs/src/lib.rs#L3), [apps/agentic-org-ui/src/routes/(shell)/projects/[id]/+page.svelte:64](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/routes/(shell)/projects/[id]/+page.svelte#L64), [docs/specs/decisions-and-tech-debt-ux.md:93](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/decisions-and-tech-debt-ux.md#L93), [docs/specs/decisions-and-tech-debt-ux.md:377](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/decisions-and-tech-debt-ux.md#L377).

**Test evidence inspected:** org-docs tests cover parsing only.; No scoped search/template tests found.

### FC-063

[Project briefs and capability requirements](../specs/decisions-and-tech-debt-ux.md) — **Planned**.

**Verified slice:** Repository specs are requirements, not product data. No project-brief/capability authoring route; project page states documents are not served.

**Remaining work:** Author and publish canonical project briefs and feature.md capability requirements with scope, owner, provenance and live indexing.

**Requirement checks:** DEC-1 FTR-1 AUTH-1 AUTH-3.

**Source evidence:** [docs/specs/decisions-and-tech-debt-ux.md:76](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/decisions-and-tech-debt-ux.md#L76), [docs/specs/decisions-and-tech-debt-ux.md:364](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/decisions-and-tech-debt-ux.md#L364), [docs/specs/features-ux.md:33](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/features-ux.md#L33), [apps/agentic-org-ui/src/routes/(shell)/projects/[id]/+page.svelte:64](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/routes/(shell)/projects/[id]/+page.svelte#L64), [docs/design/organization-data/README.md:3](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/README.md#L3).

**Test evidence inspected:** No app tests for canonical project brief/feature requirement records.

### FC-064

[ADRs, designs, evaluations and Why links](../specs/decisions-and-tech-debt-ux.md) — **Planned**.

**Verified slice:** Generic parser exists but no application document publication/history or Why backlinks are exposed. Task links/activity are separate.

**Remaining work:** Implement canonical decision/design/report authoring, typed links, actor-bound revisions/activity and Why/Led to/Mentioned in views.

**Requirement checks:** DEC-1 DEC-2 WHY-1 WHY-2 AUTH-1 AUTH-4.

**Source evidence:** [crates/org-docs/src/lib.rs:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-docs/src/lib.rs#L1), [apps/agentic-org-ui/src/routes/(shell)/projects/[id]/+page.svelte:64](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/routes/(shell)/projects/[id]/+page.svelte#L64), [docs/specs/decisions-and-tech-debt-ux.md:364](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/decisions-and-tech-debt-ux.md#L364), [docs/specs/decisions-and-tech-debt-ux.md:374](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/decisions-and-tech-debt-ux.md#L374), [docs/specs/decisions-and-tech-debt-ux.md:397](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/decisions-and-tech-debt-ux.md#L397).

**Test evidence inspected:** No publication or Why-link UI tests found.

### FC-065

[Debt, limitation and exception records](../specs/decisions-and-tech-debt-ux.md) — **Planned**.

**Verified slice:** Task shortcut provenance exists, but no canonical debt/limitation/exception authoring. UX defers specialized debt schema/workflow and limitation/exception kinds; feature limitations use feature.md and findings can use report/decision records.

**Remaining work:** Implement selected general knowledge authoring/scoping. Do not implement specialized debt/limitation/exception counters or approval engines without a new decision.

**Requirement checks:** DBT-1 DBT-3 DBT-4 LMT-1 EXN-1 AUTH-6.

**Source evidence:** [crates/org-work/src/model.rs:85](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/model.rs#L85), [docs/specs/decisions-and-tech-debt-ux.md:312](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/decisions-and-tech-debt-ux.md#L312), [docs/specs/decisions-and-tech-debt-ux.md:327](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/decisions-and-tech-debt-ux.md#L327), [docs/specs/decisions-and-tech-debt-ux.md:379](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/decisions-and-tech-debt-ux.md#L379), [docs/specs/decisions-and-tech-debt-ux.md:405](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/decisions-and-tech-debt-ux.md#L405).

**Test evidence inspected:** Work tests cover shortcut data only; no canonical records authoring tests.

### FC-066

[Agent document CLI and publication](../specs/decisions-and-tech-debt-ux.md) — **Planned**.

**Verified slice:** agent-os supports messaging/status/relay, not document commands. org-docs parses in-memory text only. Legacy doc/debt commands are deferred.

**Remaining work:** Add current-schema document list/show/create/edit and publication CLI with canonical scopes, authorization, IDs/revisions and pending-publication semantics.

**Requirement checks:** AUTH-1 AUTH-5 AUTH-6.

**Source evidence:** [apps/agent-os/src/main.rs:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agent-os/src/main.rs#L1), [crates/org-docs/src/lib.rs:8](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-docs/src/lib.rs#L8), [docs/specs/decisions-and-tech-debt-ux.md:401](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/decisions-and-tech-debt-ux.md#L401), [docs/specs/decisions-and-tech-debt-ux.md:402](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/decisions-and-tech-debt-ux.md#L402).

**Test evidence inspected:** agent-os CLI tests cover messaging/CLI contracts, not docs.

### FC-067

[Releases, milestones and explicit content manifests](../design/organization-data/09-release-content-and-flags.md) — **Planned**.

**Verified slice:** No release/milestone editor or explicit manifest route; project page says documents are later-release. No inference of release content from current tasks.

**Remaining work:** Persist R-/M- records and explicit same-project feature/task membership, tombstone links/backlinks and release revisions/changelog associations.

**Requirement checks:** Release content contract §90-94 FTR-6 FTR-11.

**Source evidence:** [apps/agentic-org-ui/src/routes/(shell)/projects/[id]/+page.svelte:49](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/routes/(shell)/projects/[id]/+page.svelte#L49), [docs/design/organization-data/09-release-content-and-flags.md:90](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/09-release-content-and-flags.md#L90), [docs/specs/features-ux.md:104](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/features-ux.md#L104), [docs/specs/features-ux.md:106](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/features-ux.md#L106), [docs/design/organization-data/README.md:3](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/README.md#L3).

**Test evidence inspected:** Draft schema test is structural, not release command behavior.; No product release tests found.

### FC-068

[Release product changelogs](../design/organization-data/09-release-content-and-flags.md) — **Planned**.

**Verified slice:** No release changelog authoring or publication route; project documents are unavailable.

**Remaining work:** Persist authored product changelog per release with scope/owner/lifecycle/revisions, separate from technical deployment changelog.

**Requirement checks:** Release content contract §5, §90-94 DEC-1 AUTH-1.

**Source evidence:** [apps/agentic-org-ui/src/routes/(shell)/projects/[id]/+page.svelte:64](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/routes/(shell)/projects/[id]/+page.svelte#L64), [docs/design/organization-data/09-release-content-and-flags.md:7](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/09-release-content-and-flags.md#L7), [docs/design/organization-data/09-release-content-and-flags.md:33](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/09-release-content-and-flags.md#L33), [docs/specs/features-ux.md:104](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/features-ux.md#L104).

**Test evidence inspected:** No product release changelog tests found.

### FC-069

[Deployment records, environments and rollback links](../design/organization-data/07-release-deployments.md) — **Planned**.

**Verified slice:** No deployment record command/view/route found. Selected design records attempts but excludes deployment executor; schema is draft only.

**Remaining work:** Implement persisted attempts/outcomes, environment/target, actual commit snapshots, baseline/rollback links, technical changelog and applied snapshots; no runner needed for this row.

**Requirement checks:** Deployment contract §14-51 Deployment contract §95-104.

**Source evidence:** [docs/design/organization-data/07-release-deployments.md:3](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/07-release-deployments.md#L3), [docs/design/organization-data/07-release-deployments.md:47](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/07-release-deployments.md#L47), [docs/design/organization-data/07-release-deployments.md:95](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/07-release-deployments.md#L95), [apps/agentic-org-ui/src/routes/(shell)/projects/[id]/+page.svelte:64](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/routes/(shell)/projects/[id]/+page.svelte#L64).

**Test evidence inspected:** Draft schema checks do not exercise service behavior.; No deployment record tests found.

### FC-070

[Flag definitions and intended/applied snapshots](../design/organization-data/09-release-content-and-flags.md) — **Planned**.

**Verified slice:** No persistent feature flag definition/intent/applied-snapshot service found. Design separates records from unselected evaluation/deployment.

**Remaining work:** Persist definitions, per-release intent and per-deployment applied snapshots with identity/history; evaluator and deployment executor stay out of scope.

**Requirement checks:** Flag contract §96-100 Flag contract §115-143.

**Source evidence:** [docs/design/organization-data/09-release-content-and-flags.md:3](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/09-release-content-and-flags.md#L3), [docs/design/organization-data/09-release-content-and-flags.md:96](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/09-release-content-and-flags.md#L96), [docs/design/organization-data/09-release-content-and-flags.md:115](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/09-release-content-and-flags.md#L115), [docs/design/organization-data/09-release-content-and-flags.md:132](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/09-release-content-and-flags.md#L132), [docs/design/organization-data/README.md:3](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/README.md#L3).

**Test evidence inspected:** Schema tests check draft flag invariants, not product persistence.; No product flag tests found.

## Chat and agent messaging

### FC-077

[Mentions, search, attachments and reactions](../specs/chat-ux.md) — **In progress**.

**Verified slice:** The Svelte chat sample includes mention completion, access-filtered in-memory search, attachment display and reactions; focused UI/model tests exercise these flows.

**Remaining work:** There is no production chat API or selected durable/visibility-safe search and attachment service. The selected protected messaging source, per-member authorization and reaction persistence are not supplied by the Phase A mailbox slice.

**Requirement checks:** CHAT-13 CHAT-14 CHAT-36 CHAT-37.

**Source evidence:** [apps/agentic-org-ui/src/lib/modules/messaging/model.ts:425](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/modules/messaging/model.ts#L425), [apps/agentic-org-ui/src/lib/modules/messaging/components/MessageRow.svelte:121](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/modules/messaging/components/MessageRow.svelte#L121), [apps/agentic-org-ui/src/lib/modules/messaging/api.ts:7](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/modules/messaging/api.ts#L7), [crates/org-mailbox/README.md:31](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-mailbox/README.md#L31), [docs/design/organization-data/15-messaging-and-chat.md:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/15-messaging-and-chat.md#L1).

**Test evidence inspected:** apps/agentic-org-ui/src/lib/modules/messaging/model.test.ts; apps/agentic-org-ui/src/lib/modules/messaging/engine.test.ts; apps/agentic-org-ui/src/lib/modules/messaging/markdown.test.ts

### FC-078

[Drafts, history bounds and conversation performance](../specs/chat-ux.md) — **In progress**.

**Verified slice:** Thread/ask drafts are retained in a page-scoped in-memory store with separate keys and bounded entry count; tests cover scope changes and draft restoration. The chat sample renders a full in-memory message collection.

**Remaining work:** No 200-message initial/older paging, 1,000-message performance evidence or production persistence/retention path is present. The bounded draft capacity is not a message history bound; implement CHAT-43 paging and separately verify AC-CP performance.

**Requirement checks:** CHAT-27 CHAT-28 CHAT-32 CHAT-33 CHAT-43 AC-CP-1–5.

**Source evidence:** [apps/agentic-org-ui/src/lib/modules/messaging/drafts/store.ts:22](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/modules/messaging/drafts/store.ts#L22), [apps/agentic-org-ui/src/lib/modules/messaging/drafts/store.test.ts:5](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/modules/messaging/drafts/store.test.ts#L5), [apps/agentic-org-ui/src/lib/modules/messaging/api.ts:7](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/modules/messaging/api.ts#L7), [docs/specs/chat-ux.md:215](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/chat-ux.md#L215), [docs/specs/chat-ux.md:292](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/chat-ux.md#L292).

**Test evidence inspected:** apps/agentic-org-ui/src/lib/modules/messaging/drafts/store.test.ts; apps/agentic-org-ui/src/lib/modules/messaging/engine.test.ts; separate browser/performance matrix still required

### FC-079

[Directed asks, answers and withdrawals](../specs/chat-ux.md) — **In progress**.

**Verified slice:** Installed Phase A CLI supports legacy-spool ask/reply, while the chat sample renders/responds to Ask cards and includes answers, recipient authorization rules and resolution behavior. Mailbox README explicitly marks ask/reply as follow-up work.

**Remaining work:** Selected protected conversation source, production cards, withdrawn/raced outcome persistence, operation receipts and execution waiting reconciliation are incomplete. Preserve installed spool state through the conditional cutover.

**Requirement checks:** CHAT-5 CHAT-6 CHAT-7 CHAT-42 MA-1 MA-3–5.

**Source evidence:** [apps/agent-os/src/main.rs:514](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agent-os/src/main.rs#L514), [apps/agent-os/src/main.rs:1000](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agent-os/src/main.rs#L1000), [crates/org-mailbox/README.md:83](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-mailbox/README.md#L83), [apps/agentic-org-ui/src/lib/modules/messaging/engine.test.ts:113](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/modules/messaging/engine.test.ts#L113), [docs/specs/chat-ux.md:126](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/chat-ux.md#L126), [docs/adr/0003-agent-messaging-office-first.md:986](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/adr/0003-agent-messaging-office-first.md#L986).

**Test evidence inspected:** apps/agent-os/src/office.rs (CLI transport contract tests); apps/agentic-org-ui/src/lib/modules/messaging/engine.test.ts; apps/agentic-org-ui/src/lib/modules/messaging/model.test.ts

### FC-080

[Atomic task handoff and system notices](../specs/chat-ux.md) — **In progress**.

**Verified slice:** The chat sample models handoff creation, addressed-recipient acceptance and status notices; tests assert the recipient-only accept rule. The separate work service has transactional task mutations.

**Remaining work:** There is no cross-domain atomic transaction binding selected task assignment/history to protected handoff message/receipt. Phase A mailbox has no handoff operation, production chat is a mock, and recovery/privacy contracts are not implemented end to end.

**Requirement checks:** CHAT-18 CHAT-36 CHAT-39 CHAT-40 MA-2 MA-6.

**Source evidence:** [apps/agentic-org-ui/src/lib/modules/messaging/engine.test.ts:151](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/modules/messaging/engine.test.ts#L151), [apps/agentic-org-ui/src/lib/modules/messaging/api.ts:7](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/modules/messaging/api.ts#L7), [crates/org-mailbox/README.md:83](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-mailbox/README.md#L83), [crates/org-work/src/lib.rs:10](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/lib.rs#L10), [docs/specs/chat-ux.md:140](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/chat-ux.md#L140).

**Test evidence inspected:** apps/agentic-org-ui/src/lib/modules/messaging/engine.test.ts; crates/org-office/src/store/work/tests/call.rs; no cross-domain atomic handoff test

### FC-081

[Uninterrupting delivery, pause and waiting](../specs/chat-ux.md) — **In progress**.

**Verified slice:** Production office wiring is present: `Office::mailbox` composes `org_mailbox::Mailbox` with a SQL-backed `OfficeMessageStore` over office.db and `OfficeDirectory` over org.db; agentic-org-office constructs this mailbox and serves msg.send/inbox/read plus held msg.wait. This is the legacy local direct-message path, not selected chat.

**Remaining work:** Execution-aware wake admission, permits/holds, run-mode and pause coordination, and selected protected chat semantics are not established by this legacy delivery path. `msg.ask` and `msg.reply` remain unserved; no selected chat service or channel/group per-member policy is wired.

**Requirement checks:** CHAT-7 CHAT-22 CHAT-42 MA-7–9.

**Source evidence:** [crates/org-office/src/messaging.rs:27](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/messaging.rs#L27), [crates/org-office/src/messaging.rs:41](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/messaging.rs#L41), [crates/org-office/src/messaging.rs:56](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/messaging.rs#L56), [crates/org-office/src/messaging.rs:71](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/messaging.rs#L71), [crates/org-office/src/store/messages.rs:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/store/messages.rs#L1), [apps/agentic-org-office/src/lib.rs:324](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-office/src/lib.rs#L324), [apps/agentic-org-office/src/lib.rs:347](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-office/src/lib.rs#L347), [apps/agentic-org-office/src/msg_calls.rs:15](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-office/src/msg_calls.rs#L15), [apps/agentic-org-office/src/serve.rs:1046](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-office/src/serve.rs#L1046), [apps/agentic-org-office/src/serve.rs:760](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-office/src/serve.rs#L760), [apps/agentic-org-office/src/serve/host/operations.json:39](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-office/src/serve/host/operations.json#L39), [apps/agentic-org-office/src/serve/host/operations.json:49](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-office/src/serve/host/operations.json#L49).

**Test evidence inspected:** Server socket messaging tests in `apps/agentic-org-office/src/serve.rs` and `apps/agentic-org-office/tests/launch.rs`

## Usage, costs and budgets

### FC-084

[Immutable durable usage ledger and corrections](../design/organization-data/10-usage-and-cost-accounting.md) — **Planned**.

**Verified slice:** The runtime Usage type and SessionEvent::Usage wire shape define token/USD fields and delta/cumulative scopes, but semantic references resolve only to the runtime contract, redaction/serialization tests and a synthetic fake-session event. No native runtime adapter capture, office event consumer, persisted collector, archive or correction implementation exists in this workspace.

**Remaining work:** Implement native adapter capture, route events into an office collector, and provide durable append-only usage records/checkpoints/coverage, protected archive, dedup/hash conflict, correction chain, receipts and recoverable acknowledgment.

**Requirement checks:** Accounting contract §9-23 COST-8 COST-9.

**Source evidence:** [crates/org-runtime/src/usage.rs:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-runtime/src/usage.rs#L1), [crates/org-runtime/src/usage.rs:33](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-runtime/src/usage.rs#L33), [docs/design/organization-data/10-usage-and-cost-accounting.md:5](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/10-usage-and-cost-accounting.md#L5), [docs/design/organization-data/10-usage-and-cost-accounting.md:7](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/10-usage-and-cost-accounting.md#L7), [docs/design/organization-data/10-usage-and-cost-accounting.md:11](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/10-usage-and-cost-accounting.md#L11), [docs/design/organization-data/schema.sql:869](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/schema.sql#L869), [crates/org-runtime/src/runtime.rs:10](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-runtime/src/runtime.rs#L10), [crates/org-runtime/src/runtime.rs:14](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-runtime/src/runtime.rs#L14), [crates/org-runtime/src/runtime.rs:40](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-runtime/src/runtime.rs#L40), [crates/org-runtime/tests/fake_session.rs:232](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-runtime/tests/fake_session.rs#L232), [crates/org-runtime/tests/fake_session.rs:631](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-runtime/tests/fake_session.rs#L631).

**Test evidence inspected:** org-runtime tests validate Usage serialization and SessionEvent wire round-trip; the only session emission found is a synthetic fake-session test event, not a native adapter.; No collector/archive/correction service tests found.

### FC-085

[Historical consumer, team, task and release attribution](../design/organization-data/10-usage-and-cost-accounting.md) — **Planned**.

**Verified slice:** Runtime Usage captures model/counts, not consumer, team, task, release/project or work classification. Selected historical attribution fields are only in target contract/schema.

**Remaining work:** Capture actual execution-time consumer/scope/model/connection/work-type; explicit context precedence; preserve historical facts on correction; never infer from current assignment.

**Requirement checks:** Accounting contract §11-23 COST-3 COST-8.

**Source evidence:** [crates/org-runtime/src/usage.rs:35](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-runtime/src/usage.rs#L35), [docs/design/organization-data/10-usage-and-cost-accounting.md:11](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/10-usage-and-cost-accounting.md#L11), [docs/design/organization-data/10-usage-and-cost-accounting.md:13](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/10-usage-and-cost-accounting.md#L13), [docs/design/organization-data/10-usage-and-cost-accounting.md:21](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/10-usage-and-cost-accounting.md#L21), [docs/design/organization-data/08-record-contracts.md:44](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/08-record-contracts.md#L44).

**Test evidence inspected:** Runtime usage tests do not cover historical attribution.; No ledger attribution service tests found.

### FC-086

[Price versions, tokens, USD and coverage states](../specs/costs-and-insights-ux.md) — **Planned**.

**Verified slice:** Usage has typed fields for token categories, optional USD, estimate basis and separate scopes, but no concrete native runtime adapter creates these values, no price-version service is consumed, and no collector computes or records coverage. Semantic references show the runtime type/event contract, serializer tests and synthetic fake-session only.

**Remaining work:** Implement native runtime capture and route/model attribution; versioned provider/model rates with immutable exact price IDs; normalization/cache semantics; API-equivalent semantics; independent complete/partial/unavailable coverage states and honest partial/unknown totals.

**Requirement checks:** COST-1 COST-5 COST-6 Accounting contract §17-25.

**Source evidence:** [crates/org-runtime/src/usage.rs:35](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-runtime/src/usage.rs#L35), [crates/org-runtime/src/usage.rs:81](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-runtime/src/usage.rs#L81), [docs/design/organization-data/10-usage-and-cost-accounting.md:17](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/10-usage-and-cost-accounting.md#L17), [docs/design/organization-data/10-usage-and-cost-accounting.md:23](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/10-usage-and-cost-accounting.md#L23), [crates/org-runtime/src/runtime.rs:10](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-runtime/src/runtime.rs#L10), [crates/org-runtime/src/runtime.rs:14](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-runtime/src/runtime.rs#L14), [crates/org-runtime/src/runtime.rs:40](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-runtime/src/runtime.rs#L40), [crates/org-runtime/tests/fake_session.rs:232](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-runtime/tests/fake_session.rs#L232), [crates/org-runtime/tests/fake_session.rs:631](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-runtime/tests/fake_session.rs#L631).

**Test evidence inspected:** org-runtime tests validate Usage serialization/event fixtures only; no native adapter capture/pricing/coverage test found.; Draft schema tests validate proposed model_prices/usage rows only.

### FC-087

[Cost views, scoped permissions and CSV export](../specs/costs-and-insights-ux.md) — **Planned**.

**Verified slice:** No live accounting ledger read/API, cost page or CSV route found. Request cost ranges are user-entered estimates, not spend; no accounting amount authorization composed.

**Remaining work:** Implement authorized usage reads/rollups, owner/PM amount filtering, coverage provenance in totals and scoped CSV consistent with underlying rows.

**Requirement checks:** COST-2 COST-4 COST-7 COST-9 COST-10.

**Source evidence:** [apps/agentic-org-ui/src/routes/(shell)/projects/[id]/+page.svelte:64](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/routes/(shell)/projects/[id]/+page.svelte#L64), [apps/agentic-org-ui/src/lib/work/requests-view.ts:102](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/work/requests-view.ts#L102), [docs/specs/costs-and-insights-ux.md:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/costs-and-insights-ux.md#L1), [docs/design/organization-data/10-usage-and-cost-accounting.md:13](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/10-usage-and-cost-accounting.md#L13).

**Test evidence inspected:** requests-view tests cover proposal estimates only.; No accounting authorization/CSV tests found.

### FC-088

[Period queries, bucketing and duration formatting](../specs/costs-and-insights-ux.md) — **Planned**.

**Verified slice:** No ledger time-window query, account-zone persistence or cost bucketing API. Generic duration formatting/agent schedule time zones are separate behaviors.

**Remaining work:** Add shared scope/bounds/checkpoint queries, account-zone calendar periods, rolling-window math, partial edge buckets and DST-aware period keys/defaults.

**Requirement checks:** COST-10.

**Source evidence:** [docs/specs/costs-and-insights-ux.md:183](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/costs-and-insights-ux.md#L183), [docs/design/organization-data/10-usage-and-cost-accounting.md:15](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/10-usage-and-cost-accounting.md#L15).

**Test evidence inspected:** Generic formatting tests are not ledger queries.; No cost period/bucket tests found.

## Execution, providers and supervision

### FC-101

[Claude, Codex and Agy native runtime adapters](../design/agent-sessions.md) — **Planned**.

**Verified slice:** Generic org-runtime interface, generic org-supervise process primitives, and a fake adapter fixture exist as foundations only; no named native runtime adapter is checked in.

**Remaining work:** Cargo workspace and source survey found no Claude/Codex/Agy native adapter crates or office session runner. No native driver evidence for launch, streaming, cancellation, or sandbox behavior across all three runtimes.

**Requirement checks:** agent-sessions design S0–S4 EMP-14 organization-data 11.

**Source evidence:** [Cargo.toml:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/Cargo.toml#L1), [crates/org-runtime/src/lib.rs:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-runtime/src/lib.rs#L1), [crates/org-runtime/tests/fake_session.rs:631](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-runtime/tests/fake_session.rs#L631), [crates/org-supervise/README.md:3](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-supervise/README.md#L3), [docs/design/agent-sessions.md:779](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/agent-sessions.md#L779).

**Test evidence inspected:** crates/org-runtime/tests/fake_session.rs uses EchoRuntime (foundation only); crates/org-supervise/tests/spawn.rs; crates/org-supervise/tests/lifecycle.rs; crates/org-supervise/tests/recovery_native.rs (generic primitives only)

### FC-102

[Progress, health, waiting and stale-run detection](../specs/employee-supervision-ux.md) — **In progress**.

**Verified slice:** Generic runtime event/outcome contract and process failure/recovery observations exist.

**Remaining work:** No production session observer/freshness aggregation or stale-run detection is wired to real vendor sessions. No employee-facing supervision view reports observed progress/wait/stop-pending for real sessions.

**Requirement checks:** EMP-1–2 EMP-13–14 organization-data 11.

**Source evidence:** [crates/org-runtime/src/lib.rs:10](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-runtime/src/lib.rs#L10), [crates/org-supervise/README.md:17](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-supervise/README.md#L17), [docs/specs/employee-supervision-ux.md:197](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/employee-supervision-ux.md#L197).

**Test evidence inspected:** crates/org-supervise/tests/lifecycle.rs; crates/org-supervise/tests/probe_reasons.rs; crates/org-runtime/tests/fake_session.rs

### FC-103

[Per-task repository workspace isolation](../design/organization-data/04-agent-structure.md) — **Planned**.

**Verified slice:** Generic sandbox specification/path validation and process/filesystem primitives exist as foundations only; no selected task-worktree admission/enforcement path is wired.

**Remaining work:** Selected employee/task/repository path topology is not wired through native adapters or office session service. No selected-layout end-to-end proof for credentials, other tasks, Git metadata, symlinks and managed-state denies.

**Requirement checks:** EMP-14 organization-data 04 §Runtime location agent-sessions security SRV-41.

**Source evidence:** [crates/org-supervise/README.md:27](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-supervise/README.md#L27), [crates/org-runtime/src/spec.rs:46](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-runtime/src/spec.rs#L46), [docs/design/documentation-authority.md:69](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/documentation-authority.md#L69), [docs/specs/server-installation-and-access-ux.md:1389](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/server-installation-and-access-ux.md#L1389).

**Test evidence inspected:** crates/org-supervise/tests/planned_files.rs (generic temp-file/process support); crates/org-supervise/tests/lifecycle.rs; crates/org-supervise/tests/recovery_native.rs; crates/org-runtime/tests/fake_session.rs (fake adapter)

## Roles, processes and retrospectives

### FC-108

[Role records and employee role assignment](../specs/roles-ux.md) — **In progress**.

**Verified slice:** Current agent profile stores and updates a required free-text role value.

**Remaining work:** No normalized selected role records or optional stable role_id assignment/persistence. No role authoring/history UI or selected employee revision/reference validation; runtime composition is separately deferred.

**Requirement checks:** ROLE-1 ROLE-5 ROLE-9–10 PROF-1 organization-data 04.

**Source evidence:** [crates/org-proto/src/org.rs:358](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-proto/src/org.rs#L358), [crates/org-office/src/employees/hire.rs:21](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/employees/hire.rs#L21), [crates/org-office/src/employees/update.rs:37](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/employees/update.rs#L37), [docs/specs/roles-ux.md:104](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/roles-ux.md#L104).

**Test evidence inspected:** crates/org-office employee profile command tests; apps/agentic-org-ui/src/lib/org/agents.test.ts

### FC-110

[Scoped process and standard documents](../design/processes.md) — **In progress**.

**Verified slice:** Pure earlier-contract process parser/resolver/render/checklist/branch helper library and sample-backed read-only Processes UI exist.

**Remaining work:** No selected org/team/project source repository persistence/publication adapter or selected envelope service. Selected inheritance/approval/runtime delivery engine is deferred; old helper field/status shapes are not interchangeable with selected records.

**Requirement checks:** PROC-1 PROC-8 PROC-16 processes design decisions 1–5 organization-data 01 process/standard content.

**Source evidence:** [crates/org-process/src/lib.rs:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-process/src/lib.rs#L1), [apps/agentic-org-ui/src/lib/modules/processes/world.ts:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/modules/processes/world.ts#L1), [docs/design/processes.md:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/processes.md#L1).

**Test evidence inspected:** crates/org-process file/resolve/render unit tests; apps/agentic-org-ui/src/lib/modules/processes/resolve.test.ts; delivery.test.ts; rules.test.ts

## Git, identity, synchronization and recovery

### FC-139

[Shared repository registration and project branches](../design/projects-git-github.md) — **Planned**.

**Verified slice:** Project UI supports project identity/teams/tasks; code repository registration and per-project integration branches are unavailable. No repository association service found.

**Remaining work:** Register organization metadata and code repositories; persist explicit per-project repository associations and integration branches with clone/readiness state.

**Requirement checks:** Git design §8-16 Git design implementation plan G1/G2d.

**Source evidence:** [docs/design/projects-git-github.md:5](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/projects-git-github.md#L5), [docs/design/projects-git-github.md:8](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/projects-git-github.md#L8), [docs/design/projects-git-github.md:145](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/projects-git-github.md#L145), [apps/agentic-org-ui/src/routes/(shell)/projects/[id]/+page.svelte:64](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/routes/(shell)/projects/[id]/+page.svelte#L64).

**Test evidence inspected:** No repository-registration/project-branch tests found.

### FC-140

[Git worktrees and GitHub integration](../design/projects-git-github.md) — **Planned**.

**Verified slice:** Workspace has no org-git or org-github crates, GitHub App/token adapter, agent push/gh-token command or task worktree operation. Legacy sandbox path restrictions are not end-to-end Git integration.

**Remaining work:** Implement per-task/repo worktrees from explicit integration branches, hardened Git operations, scoped HQ credentials/tokens, push authorization and GitHub PR integration.

**Requirement checks:** Git design §94-128 Git design implementation plan G2b-G4.

**Source evidence:** [docs/design/projects-git-github.md:19](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/projects-git-github.md#L19), [docs/design/projects-git-github.md:94](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/projects-git-github.md#L94), [docs/design/projects-git-github.md:116](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/projects-git-github.md#L116), [docs/design/projects-git-github.md:145](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/projects-git-github.md#L145), [docs/design/projects-git-github.md:160](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/projects-git-github.md#L160), [apps/agent-os/src/main.rs:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agent-os/src/main.rs#L1).

**Test evidence inspected:** No worktree/GitHub adapter/agent push tests found.

### FC-141

[Durable canonical records and indexed projections](../design/organization-data/08-record-contracts.md) — **In progress**.

**Verified slice:** Current system persists SQLite work items with append-only per-task events, revisions/conflicts/idempotency and sync projection. It does not implement selected YAML/Markdown organization-Git canonical sources, their round-trip, canonical tombstones/links or source revisions. Selected schema remains draft.

**Remaining work:** Implement canonical filesystem reader/writer/importer, projections, revision/concurrency/tombstones, typed relationships, direct-Git reindex and lossless restore.

**Requirement checks:** Record contract §1-38 Organization data README §57-80.

**Source evidence:** [crates/org-work/src/model.rs:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/model.rs#L1), [crates/org-office/src/store/work.rs:48](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/store/work.rs#L48), [crates/org-office/src/store/org/snapshot.rs:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/store/org/snapshot.rs#L1), [docs/design/organization-data/README.md:3](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/README.md#L3), [docs/design/organization-data/README.md:78](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/README.md#L78), [docs/design/organization-data/08-record-contracts.md:3](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/08-record-contracts.md#L3).

**Test evidence inspected:** org-office work tests cover current SQLite events, not selected canonical-file round-trip.; org-sync snapshot tests cover projection, not Markdown/YAML entity sources.; Draft schema test is not app migration.

### FC-142

[UUIDv7, scoped counters and readable references](../design/organization-data/06-identity-and-numbering.md) — **In progress**.

**Verified slice:** Current work service enforces ULID IDs, transactional per-project T-n allocation and readable task references. Selected target uses UUIDv7, authority-backed team/project T-, F/R/M counters and final allocation/recovery; this is materially different.

**Remaining work:** Preserve existing stable IDs while moving new records to UUIDv7; implement durable scoped organization reference allocations/journal, unify typed-task T- mappings, projectless TEAM- refs and pending/unresolved imports.

**Requirement checks:** Identity contract §1-11 Identity contract §74-77.

**Source evidence:** [crates/org-work/src/ids.rs:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/ids.rs#L1), [crates/org-work/src/tasks.rs:45](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/tasks.rs#L45), [crates/org-office/src/store/work/metadata.rs:106](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/store/work/metadata.rs#L106), [crates/org-work/src/tests/numbering.rs:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-work/src/tests/numbering.rs#L1), [docs/design/organization-data/06-identity-and-numbering.md:38](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/06-identity-and-numbering.md#L38), [docs/design/organization-data/06-identity-and-numbering.md:76](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/06-identity-and-numbering.md#L76).

**Test evidence inspected:** org-work numbering tests validate current per-project T counter/nonreuse.; org-work ID tests validate ULIDs.

### FC-143

[Legacy data import mappings and staging](../design/organization-data/12-task-assignment-and-history.md) — **Planned**.

**Verified slice:** Generic snapshot decoder supports legacy/default-field wire compatibility; no B-/REQ-/FEAT-ID mappings, import preview/staging or unresolved-record workflow found. Data model says the eventual mapping is not implemented.

**Remaining work:** Implement explicit ID/reference mappings, collision detection, provenance preservation, unresolved controlled staging and validated publish without renumbering. Existing-team UI import is unrelated.

**Requirement checks:** Import/mapping contract §227-231 Identity contract §38.

**Source evidence:** [crates/org-office/src/store/org/snapshot.rs:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/store/org/snapshot.rs#L1), [crates/org-office/src/store/org/snapshot.rs:1072](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-office/src/store/org/snapshot.rs#L1072), [docs/design/organization-data/03-features-and-work.md:227](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/03-features-and-work.md#L227), [docs/design/organization-data/03-features-and-work.md:231](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/03-features-and-work.md#L231), [docs/design/organization-data/06-identity-and-numbering.md:38](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/design/organization-data/06-identity-and-numbering.md#L38).

**Test evidence inspected:** Snapshot compatibility test covers field decoding, not legacy data migration.; No ID/reference staging tests found.

## Session inspection and private memory

### FC-150

[Inspect sessions and transcripts in browser](../specs/our-org-in-the-app.md) — **Planned**.

**Verified slice:** Session/runtime library contracts and fake-session tests exist; `/sessions` appears as a navigation entry.

**Remaining work:** Sessions route resolves to generic Coming soon placeholder. No authenticated HQ browser status/event/transcript reader, freshness or privacy proof against real sessions.

**Requirement checks:** PROF-2 employee-supervision UX §1 agent-sessions design S3f/S5 and transcript privacy boundary.

**Source evidence:** [apps/agentic-org-ui/src/lib/nav.ts:11](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/lib/nav.ts#L11), [apps/agentic-org-ui/src/routes/(shell)/[section=section]/+page.svelte:9](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/apps/agentic-org-ui/src/routes/(shell)/[section=section]/+page.svelte#L9), [crates/org-runtime/src/session.rs:1](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/crates/org-runtime/src/session.rs#L1), [docs/specs/our-org-in-the-app.md:326](https://github.com/vgmello/agentic-org/blob/555037f30259cf7dc27a8c2680822064b7fb4d96/docs/specs/our-org-in-the-app.md#L326).

**Test evidence inspected:** crates/org-runtime/tests/fake_session.rs; apps/agentic-org-ui/e2e/session-foundation.spec.ts (foundation only); no browser inspector test found

