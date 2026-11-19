# B2 plan: organization, teams and people

Base: `v1` at `5f9941f1` (B1b integrated). Repository root: `/Users/vgmello/projects/agentic-org-claude`.
Paths are relative to it. Read-only planning; nothing was built or run. Claims marked
**(not verified)** were inferred from reading. Planned by a delegated staff-engineer session on
`claude-opus-5-5`; profile `hybrid-2026-11-14` (planner opus/medium, risk effort high).

## 0. Summary

- **Outcome.** From `/api/v1` (and, after A5, from `agentic-org`), the owner creates, renames,
  moves and archives teams; adds, updates, moves and deactivates people; sets a team lead and a
  manager. Every change is idempotent under its operation ID, revision-checked, attributed to the
  owner's person, durable, and visible in the journal and receipts. Agents are never added here.
- **Shape.** Five blocks. B2.1 and B2.3a can run in parallel (separate worktrees, no shared files).
  B2.1, B2.2, B2.3a and B2.3b land in `v1` **before A5**. Only B2.4 (command-line rows) waits for A5.
- **Biggest findings.**
  1. No browser request reaches an office today: `apps/agentic-org-web/src/host.rs:3-4` "opens no
     office channel", and the route table (`apps/agentic-org-web/src/api.rs:482-512`) has no
     organization-data route. "Through `/api/v1`" needs that path first (B2.3a). It is in A3's
     Build list (`docs/implementation-plan.md:167-168`) but unbuilt. Ownership needs the other lead (Q8).
  2. Rule refusals are not recorded today. `Refusal::Invalid` is undone and not receipted
     (`crates/org-store/src/writer.rs:344`, `features/mod.rs:60`). The wire contract stores them
     (`crates/org-proto/src/org.rs:33-41`, `docs/design/web-api-v1.md` §3 "a rule refusal is stored
     and replays"). B2 records domain refusals as `Rejected` (decision D3).
  3. The native wire, gateway codec and UI mirrors still carry the many-to-many model
     (`org.rs:485-528` `Member`/`Membership`; `gateway/org_mutations.rs:160-250`). B2 replaces them.
     The UI session must update its mirrors in step with B2.2 (section 7).
  4. `expired` / `clock_skew` on command IDs are not implemented anywhere (grep of `org-store` and
     `org-office` finds nothing). B2.2 adds them for organization commands.

## 1. Scope

**In scope**
- Organization family on the office store: team writes (create incl. top-level per Q1, rename,
  move, set lead, archive), person writes (add, update, move home team, set manager, deactivate),
  role records read-only (codec, projection, `role_id` reference rule), new record fields
  `teams.max_members` and `employees.builtin_key` (Q4).
- Deleting the many-to-many model: `crates/org-office/src/rules/**`,
  `crates/org-office/src/employees/**`, `organization/{structure,people,policy,view,apply}.rs`,
  `people::owner_bootstrap` (B1b review L1). Worth-keeping tests are ported (section 4).
- Native wire: new organization commands and snapshot in `org-proto`; `dry_run`; `command_id` as
  ULID or UUIDv7; `expired`/`clock_skew`; `operation.get` from receipts; `--durable` semantics.
- Store: one dry-run entry point (decide + apply + undo, no frame, no receipt, positions returned).
- Web API: browser session to office channel (B2.3a), snapshot route, §9 organization mutation
  routes, `GET /api/v1/operations/org/{id}` (B2.3b).
- Command line (after A5): rows `org inspect`, `team list|inspect|create|update|move|archive`,
  `person inspect|add|update|move|deactivate`, `agent inspect`; CLI-4, CLI-5, CLI-6.
- Records: FEATURES FC-025..029 (and FC-013, FC-168 notes), web-api-v1.md and
  teams-and-org-chart.md "What the code does today", org.rs module docs, implementation-plan notes.

**Not in scope**
| Item | Owner |
| --- | --- |
| Any `apps/agentic-org-ui` file (mirrors listed in section 7) | UI session |
| Agents: hire, agent update/move/pause/offboard, `may_hire` changes, HR lead behavior | C2/C3 |
| Role writes (create/update a role) | C2 / roles UX (Q12) |
| Projects, project teams, contributing teams; the old `project.*` ops stay `unsupported` | B3 |
| Second person, invitations, ownership hand-over, non-owner reads | A6 |
| Change stream / `org.changed` / `subscribe`; `who` presence | B11 / B6 |
| Receipt retention and pruning (30-day window) | B1c follow-up (record it) |
| UI processes-key `already_exists` contract | C5 (log line 214) |
| Wire-wide ULID to UUIDv7 switch beyond org commands | ID slice (FC-142) |
| `agent-os who` for a placed agent, offboarded agent refused at bind, LNX-2 `agent-os` in container | C2/C3 (Q9) |

## 2. Premises (with evidence)

| # | Premise | Evidence |
| --- | --- | --- |
| P1 | Records already model one home team: `Team{parent_team_id, lead_employee_id, status}`, `Employee{home_team_id, role_id, reports_to_employee_id, status, may_hire}` | `crates/org-office/src/organization/records.rs:518-620` |
| P2 | Records lack `max_members` (doc 01 table, default 6) and `builtin_key` (doc 04, `hr`) | `records.rs:518-527, 600-620`; `docs/design/organization-data/01-organization-structure.md:345`, `04-agent-structure.md:241` |
| P3 | Paths follow the team chain; a team or person move rewrites every descendant path | `records.rs:552` `team_path(chain)`, `:623` `employee_path(chain,..)`; projector folder check `projector.rs:301-317` |
| P4 | Projector `validate` already enforces: tree, unique numbers, allocation, lead is a direct member, unique handle, acyclic reporting, attribution, folder/field agreement | `projector.rs:142-318` (`:222`, `:264`, `:274`) |
| P5 | Only `team.create` is served; every other org command is `unsupported`; a top-level team is refused | `office/mod.rs:350-371` (`:357`), `organization/wire.rs:52` |
| P6 | `operation.get` answers `unsupported` | `apps/agentic-org-office/src/lib.rs:472` |
| P7 | Only the owner passes the office's per-request authority; owner = creation intent's creator | `apps/agentic-org-office/src/serve/host/authority.rs:70`, `serve.rs:426-435`, `office/mod.rs:244`, `snapshot.rs:30` |
| P8 | Invalid refusals are not recorded; Rejected ones are | `crates/org-store/src/writer.rs:343-354`, `features/mod.rs:60` |
| P9 | The store has mark/undo but no dry-run entry point; `apply_entry` needs a framed entry | `writer.rs:322-386`; `docs/design/agent-os-command-line.md` §2.6 |
| P10 | Entry size bound 16 MiB (bounds a subtree move) | `crates/org-store/src/journal.rs:37` |
| P11 | Engine-independent suite exists and runs on memory and SQLite | `organization/testkit.rs:1219`, `:1270`; `store/open/tests.rs:202,212` |
| P12 | `rules/**` is used outside its tests only for `OrgCommand` parse; `directory.rs` uses `policy::{is_offboarded,name_key}` | `rules/mod.rs:43-47`, `serve.rs:33,1160`, `directory.rs:18` |
| P13 | Web process opens no office channel; host has `office.open_channel` and `session.grant` for native sessions and a `ChannelKind::Web` transfer path | `apps/agentic-org-web/src/host.rs:3-4`; `crates/org-host/src/service/mod.rs:353-386`; `service/listener.rs:1005-1020`; `crates/org-host-control/src/ops.rs:188-209` |
| P14 | K1 library exists; `agentic-org` has no sign-in and no rows | `crates/org-command/src/*`; `apps/agentic-org/src/` (admin, serve, service, setup only) |
| P15 | Office allow-list of native methods is an inventory file | `apps/agentic-org-office/src/serve/host/operations.json:55,70` |
| P16 | Writer wire has `Request::Owner` with no production caller | `crates/org-office-launch/src/writer.rs:206-216`; only `org-host/src/writer.rs` and test support use it |

## 3. Decisions taken in this plan (lead may overrule)

- **D1 Wire replaced, not adapted** (non-negotiable 14). New `org-proto::org` DTOs: `Team{id, number,
  name, parent_team_id?, lead_employee_id?, status, max_members, revision}`, `Employee{id, handle, kind,
  name, home_team_id, role_id?, reports_to_employee_id?, status, contact?, title?, focus?, role_text?,
  title_in_team?, offboarded_at?, may_hire, created_at?, revision}`, `OrgSnapshot{position,
  organization{id,slug,name}, offices[], teams[], employees[], projects/areas as today for B3}`. `Member`,
  `Membership`, `AgentProfile*`, `RunMode`, `Schedule` and the membership/agent commands are deleted
  when no consumer remains (compiler decides). `who` (`org_proto::directory`) stays; `directory.rs`
  maps from the new snapshot, with `handle` read from the record (no name-derived handle).
- **D2 Commands** (native method = web route action = CLI row): `team.create{team_id, name,
  parent_team_id?}`, `team.update{team_id, expected_revision, name?, lead_employee_id?, clear_lead}`,
  `team.move{team_id, expected_revision, parent_team_id}`, `team.archive{team_id, expected_revision}`,
  `person.add{employee_id, handle, name, home_team_id, contact?, title?, focus?, role_text?,
  title_in_team?, role_id?}`, `person.update{employee_id, expected_revision, <same optionals>,
  clear_<field>, reports_to_employee_id?, clear_manager}`, `person.move{employee_id,
  expected_revision, home_team_id}`, `person.deactivate{employee_id, expected_revision}`. Every command
  carries `command_id?` and `dry_run` (D5). Exact field names are the writer's within this shape.
- **D3 Refusal classes.** `Invalid` (not recorded): undecodable params, bad ID shapes, unauthorized
  actor, unknown organization, `expired`, `clock_skew`. Every domain rule refusal (`not_found`,
  `invalid_name`, `invalid_request` for a rule, `cycle`, `owner_protected`, `not_empty`, `conflict`,
  `already_exists`) is `Rejected` and receipted, so a retry and `operation.get` replay it. Precedence
  is the existing one in `org.rs:43-60`.
- **D4 Operation IDs.** `organization::operation_id::from_wire(&str)`: UUIDv7 kept as is; canonical
  ULID mapped with `from_wire_ulid` (B1b R2-B3). `expired` (>29 days) and `clock_skew` (>5 min ahead)
  use the ID's time against the store clock's high-water. Idempotency keys: Q7.
- **D5 Dry run.** `org_store::Store::check(op, decide) -> Result<Reply, StoreError>`: on the writer
  thread, mark, decide, apply the change's files through the projectors and `validate`, then undo to the
  mark and return positions. No frame, no append, no receipt, no file. `dry_run` with a `command_id` is
  `invalid_request`. Answer: `CommandAccepted{status: checked}` (new `CommandStatus::Checked`), no position.
- **D6 `--durable`.** Every organization op is `Ack::Durable` (`features/mod.rs:30-33`); the flag is
  accepted and changes nothing. Documented in the row help.
- **D7 Authorization.** Owner only, checked twice: the grant authority (P7) and inside each `decide`
  (actor = `By::Employee(owner)`, passed in from `Office`), per B1b ruling L1/L7. `team.create`'s
  interim "lead of the parent" rule is replaced by owner-only (doc 04 "Who may change what").
- **D8 Agents.** No command adds or moves an agent. `person.*` on an agent ID is `invalid_request`
  ("agents change through HR"). A team or person move still rewrites agent profile paths under a moved
  subtree (durable only; no agent is placed before C3).
- **D9 History.** "In the history" means: one journal entry and one receipt per change (kind, actor =
  owner's employee ID, operation ID, outcome) and record attribution (`updated_by`, `updated_at`,
  `revision + 1`). A readable history view is U1/B11 (Q10).
- **D10 `Request::Owner`** stays unused for A6's ownership hand-over (Q11); recorded in FC-029.

## 4. Data-contract rules to enforce, mapped to tests

All rows are tests in the engine-independent suite (`organization_conformance_tests!`, memory and
SQLite) unless marked *wire*. Enforcement point: `decide` refuses; projector `validate` backs it.

| Rule (source) | Enforced | Test (new unless noted) |
| --- | --- | --- |
| R1 Team ID UUIDv7, unique across the organization's records (06) | decide | `team_refusals` (kept) |
| R2 HQ alone numbers teams; counter + immutable allocation in one entry; a number never moves or is reused (06) | decide + validate | `team_numbers`, `concurrent_team_numbers` (kept); `archive_and_move_keep_the_number` |
| R3 Tree: parent exists; move never under self or a descendant (`cycle`); depth <= 16 counting the moved subtree (01, design) | decide + validate | `moving_a_team_cannot_make_a_loop` (ported), `a_move_past_the_depth_limit_is_refused` |
| R4 Folders match fields; a move rewrites the team, every descendant team and every profile below in one entry; nothing left at old paths (01) | decide + validate | `a_subtree_move_rewrites_every_path_once`, `rebuild_equivalent` after a move |
| R5 Top-level teams per Q1; the creation team is never moved under another or archived | decide | `the_first_team_stays_top_level` |
| R6 Name one line, <= 200 chars; sibling names unique ignoring case among active siblings (Q6) | decide | `sibling_team_names_clash_ignoring_case_but_not_across_parents` (ported) |
| R7 Status `active|archived`; archived parent takes no new team, move or person; archive preconditions per Q5 | decide | `archived_teams_take_nothing_new`, `archive_needs_an_empty_team` |
| R8 Lead optional, a direct employee (home team = team), active, person or agent (01, 04) | decide + validate | `a_teams_lead_is_a_direct_active_employee` (ported, owner exemption removed) |
| R9 A lead who moves or is deactivated stops leading in the same entry, team revision +1 (Q3) | decide | `moving_a_lead_ends_the_appointment_in_one_entry` |
| R10 One home team, required, existing and active for add/move (01, 04) | decide + validate | `a_person_needs_an_active_home_team` |
| R11 Handle: `is_handle`, unique in the organization over every status, not reserved (`here`, address shapes) (04, 06) | decide + validate | `people_share_one_handle_namespace` (ported), `an_inactive_persons_handle_stays_taken` |
| R12 One status field; `paused` agent-only; `offboarded_at` only when inactive (04) | decode + decide | `deactivate_sets_inactive_and_offboarded_at_and_keeps_the_handle` |
| R13 Reporting: target exists, is active, not self, acyclic (04) | decide + validate | `reporting_lines_have_no_loops` (ported), `nobody_new_reports_to_an_inactive_person` |
| R14 Deactivation: owner refused `owner_protected`; references to the person handled per Q2 | decide | `the_owner_cannot_be_deactivated`, `deactivating_clears_or_refuses_references` |
| R15 New person: kind person, `may_hire` true, `builtin_key` null (04) | decide | `a_new_person_may_hire` |
| R16 `max_members` >= 1, default 6; enforcement on add/move per Q4 | decode + decide | `team_member_limit` |
| R17 `builtin_key = hr` only on one agent, never with `may_hire` (04) | decode + validate | `one_builtin_hr_lead` (creation golden updated) |
| R18 `role_id` names a role record of the organization (01, roles UX) | decide + validate | `a_role_reference_must_exist` |
| R19 Owner-only structure changes (04); agents never act; another member refused | decide | `only_the_owner_changes_structure` |
| R20 Every change: revision +1, `updated_by` = owner, receipt with kind and actor; an edit that changes nothing still counts (org.rs:84-90) | decide | `every_write_raises_the_revision_by_one_and_is_receipted` (ported) |
| R21 Stale `expected_revision` is `conflict`, after domain rules (org.rs:56-60) | decide | `a_domain_rule_is_told_before_a_stale_revision` (ported) |
| R22 Retry same ID + params returns stored answer (refusals too); other params `invalid_request` | store + wire | `a_refusal_replays_by_command_id` |
| R23 `expired` / `clock_skew` on the command ID's time (org.rs:50-52) | wire (*wire*) | `old_and_future_command_ids_are_refused_unrecorded` |
| R24 Dry run changes no position, number, file or receipt and reports the same refusal, including one raised by `validate` (CLI-4) | store | `a_dry_run_leaves_everything_unchanged` |

## 5. Blocks

Every test uses `tempfile::tempdir()`. Writers commit per block and run the quick checks; the lead
runs the slow gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`, ignored `org-host` real-process tests, `python3 .agent-kit/runtime/kit_hooks.py check`.
Each block gets a fresh Claude Opus review at medium effort, then a whole-change review at the end.

### B2.1 · One home team on the store (domain only)

Internal foundation; no new production caller except `team.create`. **Mergeable alone. Before A5: yes.**

- **Owned files.** `crates/org-office/src/organization/{records.rs, records/**, projector.rs,
  projector/tests.rs, ports.rs, features/**, testkit.rs, mod.rs, snapshot.rs (owner field only)}`;
  new `organization/names.rs` (`name_key`, sibling-name rule); new `organization/roles` records inside
  `records.rs` or `records/roles.rs`; `crates/org-office/src/store/sqlite/{organization.rs, schema.rs}`;
  `crates/org-office/src/store/open/{operations.rs, tests.rs}`.
  Deleted: `rules/{decide.rs, memory.rs, tests.rs, vectors.rs}`, `employees/**`,
  `organization/{structure,people,policy,view,apply}.rs` (about 4,700 lines). `rules/command.rs` stays
  until B2.2 (its only consumer is `serve.rs`). `directory.rs` takes `name_key`/`is_offboarded` from
  `names.rs` or inlines them.
- **Interfaces.** One file per feature under `features/`: `update_team`, `move_team`, `archive_team`,
  `add_person`, `update_person`, `move_person`, `deactivate_person`; each `Command`, `decide(uow, here,
  owner, by, command) -> Result<Change, Refusal>`, `Op` variant with kind `organization.<verb>` and
  `Ack::Durable`. `create_team` gains `parent: Option<Id>` and owner-only authority. One `*Op` per
  feature in `store/open/operations.rs`. Reply types are plain serde structs (`{id, revision,
  generation}`); rejections are one `OrgRejection{code, message}` reply so B2.2 maps them uniformly.
  `Kind::Role(id)` and `put_role/remove_role` on the ports. Team move helper: `relocate(view, team, new_chain)
  -> Vec<(old, new, bytes)>` used by `move_team` and `move_person`.
- **Tests that must fail without it.** Rows R2-R21 above (new and ported), on both engines; creation
  golden updated for `max_members` and `builtin_key`.
- **Quick checks.** `cargo test -p org-office organization`; `cargo test -p org-office store::open`;
  `cargo clippy -p org-office --all-targets -- -D warnings`.

### B2.2 · Native wire, dry run, retries and `operation.get`

**Mergeable alone after B2.1. Before A5: yes.** Merge with the UI mirror update (section 7).

- **Owned files.** `crates/org-proto/src/{org.rs, error.rs (if a code is added), operation.rs}`,
  `crates/org-proto/src/gateway/org_mutations.rs` + `org_mutations/tests.rs` + vectors
  `test-vectors/v2/gateway/org-mutation-mappings.json`, `gateway-mutation-results.json` (codec only, no
  routes); `crates/org-store/src/{store.rs, writer.rs}` + tests (`Store::check`);
  `crates/org-office/src/organization/{wire.rs, operation_id.rs, snapshot.rs, directory.rs}`,
  `crates/org-office/src/office/{mod.rs, tests.rs}`, delete `crates/org-office/src/rules/**`;
  `apps/agentic-org-office/src/{lib.rs, serve.rs, directory.rs, inbox/org_source.rs}`,
  `serve/host/operations.json`, `serve/tests/**`, `tests/launch.rs`, `tests/launch/activation/owner.rs`;
  `crates/org-work/src/{bugs.rs, testkit.rs}` only if types they use are deleted.
- **Interfaces.** D1, D2, D4, D5. `organization::wire::{OrgCommand (moved from rules), parse, run}`:
  one mapping from each command to its op and back to `CommandAccepted`/`ApiError`.
  `Office::submit_org(member, command)` keeps its signature (B1b frozen).
  `Office::operation(member, command_id) -> Result<Option<Result<CommandAccepted, ApiError>>, _>` reads
  the receipt through `store.read`, refuses another actor's receipt as `not_found`, and maps by kind.
  `operation_get` port wired to it in `lib.rs`.
- **Tests that must fail without it.** Real worker: each command over the office socket with an owner
  grant commits and appears in `org.snapshot`; an agent-bound or ungranted connection is refused before
  params are read (kept); R22, R23, R24; `operation.get` returns a committed and a rejected answer and
  `not_found` for another member's ID; restart then the same `command_id` replays (worker level);
  gateway vectors for every new op and none for membership ops; `who` still answers from the new snapshot.
- **Quick checks.** `cargo test -p org-proto`; `cargo test -p org-store`; `cargo test -p org-office`;
  `cargo test -p agentic-org-office --lib`; clippy on the four.

### B2.3a · A browser session reaches the office

Parallel with B2.1 (no shared files). **Mergeable alone. Before A5: yes.** Needs Q8 (ownership with
the other lead; A5 touches the same host session code).

- **Owned files.** `crates/org-host-control/src/ops.rs` (web variant of open-channel/grant keyed by the
  browser session), `crates/org-host/src/service/{mod.rs, authentication.rs, authentication/browser.rs,
  listener.rs}`, `crates/org-host/src/sessions.rs` if needed, `apps/agentic-org-web/src/{host.rs,
  office.rs (new: channel pool and native client), api.rs}`, `crates/org-host/src/web_process_tests/**`.
- **Interfaces.** Host, web connection only: `office.open_channel_for_browser{cookie_digest,
  proof_digest, org_id, office_id}` returns `{channel_id, grant}`; the descriptor arrives as a
  `ChannelKind::Web` transfer record (P13). Grant claims carry the account's person for that
  organization (`account_links`). Web: one pooled channel per (session, office), capped at 7 per office
  (ADR-0012), closed on `session.ended`; every request carries the principal with a fresh grant
  (renew once on `not_authorized`, web-api §9). First real use: `GET
  /api/v1/organizations/{org}/snapshot` over today's `org.snapshot`.
- **Tests that must fail without it.** Real process (`web_process_tests`): create an organization over
  HTTP, then the snapshot route returns it; a foreign organization is `not_found`; sign-out closes the
  channel and the next snapshot is 401; the eighth web channel is refused by the host while a local
  channel still opens; no host op for an unauthenticated request.
- **Quick checks.** `cargo test -p org-host service::`; `cargo test -p agentic-org-web`;
  `cargo test -p org-host web_process_tests -- --ignored` (lead).

### B2.3b · Organization routes on `/api/v1`

**After B2.2 and B2.3a. Before A5: yes.**

- **Owned files.** `apps/agentic-org-web/src/{api.rs, api/tests.rs, office.rs}`,
  `crates/org-proto/src/gateway/{mod.rs (capability rows), stream.rs (SnapshotResponse only)}`,
  `crates/org-host/src/web_process_tests/organizations.rs` (or a new `teams_people.rs`),
  `docs/design/web-api-v1.md`.
- **Interfaces.** web-api §9 table, one POST route per D2 command under
  `/api/v1/organizations/{org}/…` (route names the writer's, recorded in web-api-v1.md), envelope
  `operation_id` = native `command_id` (UUIDv7), reply classified by `classify_org_reply`;
  `GET /api/v1/operations/org/{id}` over `operation.get`; snapshot with `resume_cursor` at its position.
  Each route is unavailable (503 before dispatch) until its tests pass (web-api §9).
- **Tests that must fail without it.** Real process: the full outcome (create team, add person, set lead,
  move person, rename, archive, set manager, deactivate) over HTTP; same operation ID replays; other
  payload `operation_reused`; stale revision 409 with nothing applied; restart of the office then
  snapshot equal and lookup answers; a lost reply resolved by lookup.
- **Quick checks.** `cargo test -p agentic-org-web`; `cargo test -p org-proto gateway`.

### B2.4 · Command-line rows (needs A5)

**Waits for A5** (signed-in person, selected instance/office, grant per call; P14).

- **Owned files.** Rows module per the design §7 ("one library module shared by both executables
  until domain crates exist"): `crates/org-command/src/rows/organization.rs` (new) and its contract
  tests; `apps/agentic-org` wiring only where A5 left the hook; `apps/agentic-org/tests/*`.
- **Interfaces.** Rows from D2 plus reads: `org inspect` (snapshot: organization, offices, top team),
  `team list`, `team inspect` (default: caller's home team), `person inspect`/`agent inspect` (one card,
  says its kind). `--dry-run` -> `dry_run`; `--idempotency-key` -> `command_id` per Q7;
  `--if-match` -> `expected_revision`, `conflict` carries the current revision in `retry_with`;
  `--durable` per D6.
- **Tests that must fail without it.** CLI-1/2/3/7/8/11 for the new rows (K1 testkit); CLI-4, CLI-5,
  CLI-6 against a real office; refused without a session (A5's rule).
- **Quick checks.** `cargo test -p org-command`; `cargo test -p agentic-org`.

### Order and parallel split

`B2.1 ‖ B2.3a` → `B2.2` → `B2.3b` → (A5) → `B2.4`. Two writers at most (B2.1 in `crates/org-office/**`;
B2.3a in `crates/org-host*/**`, `apps/agentic-org-web/**`). B2.2 is one writer: it spans
`org-proto`, `org-store`, `org-office` and the worker. Docs travel with each block; FEATURES rows are
edited by the lead at each merge.

## 6. A5 dependency split

| Lands before A5 | Waits for A5 |
| --- | --- |
| B2.1, B2.2, B2.3a, B2.3b: the whole outcome through `/api/v1`, CLI-4/5/6 semantics proven at the office (dry run, retries, revision conflicts) | B2.4 rows on `agentic-org`; CLI-4/5/6 through the command line |
| `operation.get` from receipts | A5's two real-host checks (FC-146 M4/M5), already moved to A5 |
| | `agent-os` selected-office addressing (A5) and the deferred `agent-os` office cases (need a placed agent: C2/C3, Q9) |

## 7. Wire mirrors the UI session needs (tell ui-dev before B2.2 merges)

- `apps/agentic-org-ui/src/lib/proto.ts`: new `Team`, `Employee`, `OrgSnapshot`; removed `Member`,
  `Membership`, agent profile fields; `CommandStatus` gains `checked`; any new error code.
- `src/lib/org/{api.ts, commands.ts, model.ts, reducer.ts, actions.ts, team-edit.ts, people-edit.ts,
  agents.ts, sample.ts}` and their tests: membership commands removed, D2 commands added,
  `expected_version` -> `expected_revision`, `owner_team_id` -> `parent_team_id`, `lead_member_id` ->
  `lead_employee_id`, `version` -> `revision`.
- `src/lib/platform/http/{wire.ts, gateway-vectors.test.ts}` and `src/lib/office/wire-shapes.test.ts`:
  regenerated `org-mutation-mappings.json` and `gateway-mutation-results.json` (B2.2), route table and
  snapshot/lookup routes (B2.3b).
- Risk: the UI's vector tests go red at B2.2's merge unless ui-dev lands the mirror in the same window.

## 8. Risks

**Security**
- S1 Authorization is checked at two layers (grant authority, `decide`); a test drives a non-owner
  grant directly at the office socket (CLI-10 shape) for every write.
- S2 Browser channel: grant must bind account, person, organization, office and channel; sign-out and
  revocation must close pooled channels at once; the 7-channel cap is a denial-of-service surface
  (one account must not hold all seven: cap per session, Q8).
- S3 Path construction only from validated UUIDv7 IDs (`team_path`, `employee_path`); handles and
  names never reach a path. YAML encoding of free text through the existing encoder only.
- S4 `operation.get` must not reveal another actor's operation (receipt actor check) and dry runs
  must leave no receipt that lookup could expose.
- S5 Error messages carry no path or file content (projector reasons name paths: map to fixed text at
  the wire, as `team_create_answer` does).

**Data**
- D-R1 New required fields (`max_members`, `builtin_key`) refuse to open organizations created between
  B1b and B2 (no migration, non-negotiable 14). Owner confirms recreate (Q4).
- D-R2 A subtree move rewrites many files in one entry: bounded by 16 MiB (P10) and a test with the
  deepest legal tree; materialization crash between remove and put recovers from the journal
  (crash-point test with `Testing.faults`).
- D-R3 Recording refusals (D3) adds a journal entry per refusal: flood bound is the per-call rate of an
  authenticated owner only; note for B1c measurement.
- D-R4 ULID→UUIDv7 truncation (6 bits) can collide within one millisecond (org.rs:28-32); browsers send
  UUIDv7 so only native ULID clients are exposed.

**Concurrency and coordination**
- C1 Single writer serializes decisions; revision checks cover stale clients. A move and a concurrent
  rename of a descendant are ordered by the writer; test both orders.
- C2 File overlap with A5: `crates/org-host/src/service/**`, `apps/agentic-org-office/src/{serve.rs,
  lib.rs}`. Agree hand-over with the other lead before B2.2/B2.3a start (Q8).
- C3 UI mirror timing (section 7).

## 9. Open questions needing the owner or the other lead

- **Q1 Forest or one top-level team?** Doc 01 says "a team forest"; B3 and FC-025 say "the top-level
  team". Recommendation: allow more top-level teams (owner only); "the top-level team" for B3 means the
  one made at creation (TEAM-1), which is never moved or archived.
- **Q2 Deactivating a person who leads or manages others.** Doc says references are "cleared or replaced
  explicitly". Recommendation: refuse with `not_empty` naming what to clear (teams led, direct reports),
  so nothing changes implicitly.
- **Q3 Moving a team lead.** Doc: "a lead appointment the move would invalidate is resolved first".
  Recommendation: the move ends the appointment in the same entry (old wire behavior, one recoverable
  operation), team revision +1. Alternative: refuse until cleared.
- **Q4 `max_members`.** Add the field now (default 6) and recreate post-B1b organizations? Does the limit
  apply to adding or moving a person, or only to hires (doc 04 names hires only)? Recommendation: add the
  field; enforce on hires only (C2); people unlimited.
- **Q5 Archiving.** Preconditions (no active child teams, no active employees?) and whether to offer
  unarchive. Recommendation: require an empty team; no unarchive in B2.
- **Q6 Sibling names.** Keep the old rule (unique among active siblings, ignoring case)? Doc 01 is
  silent. Recommendation: keep.
- **Q7 Idempotency keys.** How a caller's key (<= 64 letters, digits, hyphens) becomes a store
  operation ID. Recommendation: UUIDv7 or ULID used as today (D4); any other key maps to
  `UUIDv8(sha256("agentic-org/key/1" ‖ org ‖ actor ‖ key))`, with the age window taken from the
  receipt's time instead of the ID's.
- **Q8 Who builds the browser-to-office path (B2.3a)?** It is listed in A3's Build but unbuilt, and
  shares host session code with A5. Recommendation: B2 builds it now, with lead A agreeing the
  `ops.rs` contract and file hand-over.
- **Q9 Deferred agent cases.** B2's Done-when names `agent-os who` for a placed agent, an offboarded
  agent refused at bind and LNX-2's `agent-os` steps; no agent can be placed before C2/C3.
  Recommendation: move them to C3's Done-when.
- **Q10 History.** Is D9 (journal entry + receipt + attribution) enough for "is in the history", or is a
  readable per-record history expected in B2? Recommendation: D9; the view is U1/B11.
- **Q11 `Request::Owner`.** Keep for A6 (recommended) or delete now as unused.
- **Q12 Role writes.** The command-line design says no command writes a role in v1; roles UX offers
  "Create one". Who writes `roles/<id>.md`, and when? B2 builds role records read-only either way.

## 10. Acceptance map (Done-when)

| Done-when clause | Evidence |
| --- | --- |
| Every team/employee/role rule of docs 01, 04, 06 enforced and in the engine-independent suite | Section 4 rows on memory and SQLite (B2.1, B2.2) |
| Each outcome action through `/api/v1` and in the history | B2.3b real-process test; receipts and attribution asserted (D9) |
| Each outcome action from the command line | B2.4 (after A5) |
| `operation.get` for organization commands from receipts | B2.2 |
| Many-to-many membership model deleted | B2.1/B2.2 deletions; grep for `Membership`, `memberships`, `team.add_member` finds nothing outside docs history |
| `agent-os` office cases and LNX-2 `agent-os` | Q9 (C3) |

## Owner decisions (2026-11-19)
- Q1: flexible: the team forest. The owner may create more top-level teams; the creation's top-level team (TEAM-1) is never moved or archived.
- Q2: deactivating a person who leads a team or has direct reports is refused with `not_empty` naming what to clear; nothing changes implicitly.
- Q3: moving a team lead is REFUSED until the appointment is cleared or another lead appointed (not the plan's recommendation): refusal names the team.
- Q4: add `max_members` now (default 6); organizations made since B1b are recreated; enforced only on hires (C2); adding/moving people is unlimited.
- Q5: archive only an empty team (no active child teams, no active employees); no unarchive in B2.
- Q6-Q12: plan recommendations stand unless the owner says otherwise; Q8 (B2.3a ownership) and Q9 (move agent-placed cases to C3) still open.
