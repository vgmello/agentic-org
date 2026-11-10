# Standalone v1: deletion and replacement inventory

Status: **source audit and proposed implementation checklist**, 2026-11-09.
Owner: ada. Base: `v1` at `7a8827875e41cac45b75cbf170d6703417652379`.
Implements [ADR-0005](../adr/0005-standalone-server-v1.md), the
[server design](standalone-server.md) and
[SRV requirements](../specs/server-installation-and-access-ux.md).

This change evaluates and documents removals. It does not claim that source files
have already been deleted or the new server implemented. The application has not
been deployed: delete superseded code directly, with no old-app compatibility or
data-migration workstream. Keep commits buildable by updating affected callers in
the same change. Git history is sufficient to recover the old shell for reference.

## 0. Rename retained applications and reorganize by domain

The owner-requested [project structure](project-structure.md) and
[architecture rules](../architecture-rules.md) define the target layout.
Paths in the source-audit tables below include the completed worker rename.
The UI is now at `apps/org-ui` (package `org-ui`), including the retained
`src-tauri` shell. The `apps/desktop` paths below identify the historical audit
baseline: resolve their suffixes under `apps/org-ui` for current source. Shell
removal and the remaining replacement work are still pending.

| Inspected name/path | Target and required follow-through |
| --- | --- |
| Retained browser application in `apps/desktop/` (historical) | Rename complete: `apps/org-ui/`, frontend package `org-ui`; static SvelteKit build, assets, licensed UI, tests and `src-tauri` retained. Delete Tauri-only pieces under §1 in the follow-up. |
| `apps/org-server/`, Cargo package/binary `org-server` | Rename complete, including Rust application-library references to `org_server`. Preserves one worker per office and host key custody. |
| New gateway/setup helpers | Add planned `apps/org-gateway` for browser HTTP outside key custody and `apps/org-setup` with distinct Cargo package ID. Remove the Tauri workspace member before building/staging the new `agentic-org` binary to avoid package/artifact collisions. |
| Workspace and consumer references | Update Cargo members/paths/lockfile, frontend package/lockfile, test helper binary paths and environment references, build/release/signing checks, host helper resolution and documentation atomically with the source move. |
| Ownership and licensing paths | Update AGENTS.md and links to the renamed applications. Keep the same owners and Tailwind Plus containment within the product UI; no standalone UI kit extraction. |
| Managed home inputs | Retire `member.update_agent.home` / `clear_home` and other caller home overrides under API_VERSION 2; keep the response path read-only and derived from the launcher's managed instance/office/member path. Update callers and forged-path tests. |
| Protocol transition | Advance local org-proto from 1 to 2 with host/worker/client/CLI consumers and fixtures; retained versions remain additive-only and unsupported peers fail before dispatch. |
| Domain/feature organization | Group use cases inside their owning organization, work, messaging, employee and process modules; preserve public APIs, transaction boundaries and private SQL. See the project structure for the sequence. |

The downloaded setup/release command remains provisionally `agentic-org`;
`org-office-host`, `org-office-launch`, and office identities are not renamed
by this decision. Worker and UI renames are implemented; setup changes remain pending.

## 1. Delete desktop-only implementation

| Existing path or surface | Disposition and affected callers |
| --- | --- |
| `apps/desktop/src-tauri/src/tray.rs`, `menu_model.rs`, `login_item.rs` | Delete native tray/menu/Dock behavior and SMAppService adapter. Standalone setup owns service registration. Native capture is v2. |
| `apps/desktop/src-tauri/src/lib.rs`, `main.rs` | Delete Tauri composition, invoke handlers, window lifetime logic and emit bridges after the frontend callers use HTTP. Do not create compatibility HTTP endpoints named after every Tauri command. |
| `apps/desktop/src-tauri/src/import_scan.rs` | Delete the native existing-team scanner and file grants. No replacement import is required for fresh v1. |
| `apps/desktop/src/lib/import/`, `src/lib/components/BringInDialog.svelte`, `BringInReview.svelte`, `e2e/bring-in.spec.ts` | Delete old-team import UI, adapters, import fixtures and tests; remove links/after-create prompts and import-only state from callers. Agent provisioning/hiring stays. |
| `apps/desktop/src/lib/office/tauri.ts`, `org/tauri.ts`, `work/tauri.ts`, `org/tauri.test.ts` | Delete invoke/listen implementations and bridge-specific tests; implement and test HTTP adapters against the real API contract. The import adapter is removed with its feature. |
| `apps/desktop/src/lib/open-external.ts` | Remove native open command; use ordinary validated browser links. Retain safe URL handling in chat rendering and callers. |
| `apps/desktop/src-tauri/capabilities/`, `permissions/`, `tauri.conf.json`, `build.rs`, `launch-agents/` | Delete Tauri capability/build/package artifacts and the bundle-relative launch agent. Generate instance-specific service definitions from setup. |
| `apps/desktop/src-tauri/icons/` | Remove shell-only packaging assets. Retain the web application's own logo/favicon in their existing UI locations. |
| `apps/desktop/scripts/install-dev.sh`, `stage-office.sh`, `INSTALLING-THE-OFFICE.md` | Remove old app-bundle staging/signing/install flow. Replace release/install instructions with standalone payload verification and instance setup. |
| Tauri-only `Cargo.toml` workspace member and dependencies; Tauri scripts/dependencies in `apps/desktop/package.json` | Remove after callers are updated; regenerate Cargo and Bun lockfiles. Do not remove shared platform dependencies still used by the server. |
| `apps/desktop/vite.config.ts`, shell layout drag regions and native keyboard hints | Remove `TAURI_DEV_HOST`, Tauri HMR/watch assumptions, `data-tauri-drag-region` and native-only controls. Keep Vite/Svelte and browser-accessible shortcuts. |

`src-tauri/` can then be removed as a directory. Its algorithms and tests listed in
section 2 need a disposition before that final deletion; deleting the entire
frontend directory would discard the browser application we intend to keep.

## 2. Retain behavior; replace its desktop wiring

| Existing path | Retained responsibility and target |
| --- | --- |
| `apps/desktop/src-tauri/src/real.rs`, `office.rs` | Inventory domain calls, error mapping and status semantics. HQ HTTP adapter calls the office domain/local client with authenticated scope. Remove desktop install-on-read and privileged app binding. |
| `apps/desktop/src-tauri/src/org_feed.rs`, `status_feed.rs`, `pacing.rs` | Retain useful reconnection/order/coalescing behavior and tests in the server event adapter or browser client. Preserve snapshot-position correctness; no Tauri emitter remains. |
| `apps/desktop/src-tauri/src/connections.rs` | Remove local desktop organization discovery/storage. HQ returns authorized organizations; browser preference may remember a selected organization, never an office credential. |
| `apps/desktop/src-tauri/src/org_command.rs`, `org_agent_rules.rs`, `org_engine.rs`, `org_engine/projects.rs`, `work_engine.rs` | Audit for rules/test vectors absent from the canonical Rust domain. Transfer any real missing rule to its owning crate with a meaningful test, then delete shell duplicates. Do not make a mock engine the production authority. |
| `apps/desktop/src-tauri/src/mock.rs`, `org_mock.rs` | Delete desktop mock runtime. Preserve only fixtures/parity vectors that still test retained domain behavior. |
| `apps/desktop/src-tauri/src/stdio.rs` | Delete shell-only startup use. Preserve equivalent descriptor hygiene in host/office process spawning; do not delete the office's control-pipe fd protections. |
| `apps/desktop/src/lib/office/api.ts`, `org/api.ts`, `work/index.ts` | Remove `inTauri()` selection and automatic browser mocks. Separate test factories from production HTTP exports and update every consumer. Remove login-item operations from the browser interface. |
| `apps/desktop/src/lib/proto.ts`, `proto-work.ts` | Retain supported domain shapes; update HTTP envelope, authentication/error and command/event types. Delete app-specific `LoginState`/bridge-only fields once no caller remains. |
| `apps/desktop/src/routes/`, `src/lib/components/`, `src/app.css`, `svelte.config.js`, `static/` | Retain and move the actual web application to `apps/org-ui/`, including its static adapter and styling. Change onboarding/navigation to HQ login and authorized organizations; remove desktop-only/import controls. Keep Tailwind Plus within this product directory. |
| `apps/desktop/test-vectors/`, frontend unit/e2e tests | Retain domain and UI behavior tests. Replace Tauri/mock-only integration assumptions with real HTTP fixtures; delete tests for explicitly retired features. |

### 2.1 B1-03 shell audit and deletion handoff

Audited against `v1` at `0a83d76` (2026-11-10), then integrated with F1-01 at
`a0748a0`. Relative source paths below now live under `apps/org-ui`; historical
`apps/desktop` references retain their audit provenance. This audit does not implement the gateway, projects/areas or runtime,
and does not delete the shell. B1-10 remains the owner of project/area commands.

| File | Observed behavior and disposition |
| --- | --- |
| `src-tauri/src/org_engine.rs` | Delete the mock reducer after preserving the cases. Team creation/rename/move/lead/delete, memberships/titles, people/agents, reporting, offboarding and project-team links already have canonical rules in `org-office::rules`. The new canonical reader exercises 148 of the 252 shared scenarios, including errors, version checks and refusal atomicity. Only one exercised scenario has an explicit resulting-state assertion (removal of a project-team link); the other 35 scenarios with `after` assertions are preserved in the B1-10 deferred set. Do not transplant the mock's FNV fingerprint, 1,000-answer cache, JSON-file upgrade or cloned-state commit mechanism: the real office already owns transactional log/dedup/storage behavior. |
| `src-tauri/src/org_engine/projects.rs` | Preserve the exact project/area cases for B1-10. Missing canonical behavior includes project key derivation/reservations, case-insensitive names, project metadata/archive, General areas, area name/team/lead validation and reconciliation after membership, offboarding, team deletion or project-team removal. Do not transplant fabricated repository paths or pretend repository creation occurred. The canonical command/write ports do not implement this model yet. |
| `src-tauri/src/org_agent_rules.rs` | Delete the duplicate validators. `org-office::rules::decide` already enforces trimmed/control-free bounded profile text, handle uniqueness/reservations, known runtimes, schedule/run-mode consistency and offboard restrictions. Its runtime-alias checks are stricter than the mock's literal-name check and stay authoritative. Legacy caller-supplied `home` validation is not a new standalone capability: X-02/B1-07 retire home overrides under protocol v2 and report managed paths read-only. |
| `src-tauri/src/org_command.rs` | Delete the app-local tagged enum and dispatch mirror. Reuse typed `org-proto` contracts and canonical request parsers at the authorized gateway/worker boundary. Merely having a wire call type does not mean the worker implements it. |
| `src-tauri/src/work_engine.rs` | Delete the mock engine after the canonical vector reader is preserved. `org-work` already owns task/bug transitions, text/ID checks, project/team/recipient checks, checklist guards, fix-link edge rules, numbering, idempotency and time figures through its transaction/read ports. The canonical implementation additionally enforces caller authority, ULID age, durable refusals, per-field conflicts and versioned waiting events; do not replace those with the mock's loose IDs, whole-record version checks or answer cache. See the explicit work-vector differences below. Paged snapshot/list integration belongs to B1-11. |
| `src-tauri/src/org_mock.rs` | Keep invented seed data only as test fixtures. The preserved `org-office/test-vectors/organization.json` is checked against both the legacy Rust seed and the TypeScript seed. Canonical rules do not depend on an application library. |
| `src-tauri/src/mock.rs` | Delete the mock office/runtime. Organization-name vectors already live in `org-proto/test-vectors/organization-names.json` and are exercised by protocol tests. Keep the explicit distinction between test data and unavailable production capabilities. |
| `src-tauri/src/lib.rs` | Delete invoke handlers, Tauri emitters, startup locks/epochs and desktop restart-on-error orchestration. Preserve canonical payload shapes separately from app-local `CommandError.source`, `OrgView.sample` and `Connection` wrappers. A browser read must never install/start a service. Unknown mutation outcomes must remain unknown until looked up using the same operation ID. |
| `src-tauri/src/office.rs` | Delete the desktop `OfficeConnection`/login-service seam and setup-only errors (`not_installed`, `needs_approval`, `turned_off`). Preserve typed operations, read-only status checks and an explicit `unavailable` result when an area is not implemented; no production mock fallback. |
| `src-tauri/src/real.rs` | Re-home typed host/office calls, scope checks, bounded reconnect and outcome handling in B1-05/B1-06/B1-08/B1-09. Preserve the error/retry distinctions below. Retire app-token binding, code-signature desktop admission, account-wide discovery and automatic login-service installation. Gateway authority comes from X-02, never browser credentials masquerading as a desktop token. |
| `src-tauri/src/org_feed.rs` | Reproduce snapshot-position-to-subscription continuity on the same authenticated worker channel; on disconnect or `snapshot_required`, reconnect, reauthorize, take a new snapshot and resume from its position. Emit baseline after subscription succeeds, and liveness loss once rather than on every failed retry. Unknown/malformed event payloads do not terminate the stream. Retain stop responsiveness and retry-during-connect handling. |
| `src-tauri/src/status_feed.rs` | Reproduce the ordering `hello/capability -> subscribe -> office.list` on one channel, followed by changes; a list taken before subscribing can lose an update. Reconnect starts with a fresh list. Do not subscribe to a host lacking the capability or start the host from this listener. |
| `src-tauri/src/pacing.rs` | Reproduce bounded reconnect pauses `1, 2, 4, 8, 16, 30, 30...` seconds. Reset before the next wait only after a connection lasted at least 10 seconds; an immediately closed subscription must not create a one-second retry loop. Check stop at most every 250 ms; Retry shortens remaining delay to one second, never zero. |
| `src-tauri/src/connections.rs` | Delete local desktop connection discovery and `connections.json` persistence. Browser organization selection is a preference; authorized organization/office discovery comes from the server. Do not carry filesystem endpoints or cached desktop authority into browser state. |
| `src-tauri/src/stdio.rs` | Delete only shell startup wiring. Preserve existing host/worker descriptor hygiene (`org-server::fdswap` and host spawning); no change to control-pipe ownership is needed by this audit. |
| `src/lib/org/live.ts` and `load.ts` | Retain UI-side ordering/coalescing when transport changes: compare `(epoch, seq)`, keep one reload in flight per organization, catch up after a burst, accept the newly loaded baseline after restore and stop retrying a reload that made no progress. Coalescing is here, not in the Rust feed modules. F1-06 must coordinate one shared browser stream across tabs rather than one stream per shell organization. |
| `test-vectors/org-commands.json` | Moved intact to `crates/org-office/test-vectors/org-commands.json`. Canonical reader: `org-office/src/rules/vectors.rs`; legacy Rust and TypeScript readers now use the same domain-owned file. See `deferred-org-commands.json` for every case not yet supported canonically. |
| `test-vectors/work-commands.json` | Moved to `crates/org-work/test-vectors/work-commands.json`. Canonical reader: `org-work/src/tests/vectors.rs`, through `WorkService`/real read construction with disposable in-memory ports. Preserve all 29 legacy scenarios and identify deliberate canonical differences explicitly; do not silently substitute actual results or current versions. |
| `test-vectors/command-shapes.json` | Keep as a legacy desktop-wrapper fixture until F1-03 removes those wrappers. Canonical `ApiError`, `OfficeInfo`, `Organization`, `OrgSnapshot` and `CommandAccepted` payloads are separately retained in `crates/org-proto/test-vectors/office-wire-shapes.json`, checked by Rust serialization and TypeScript tests. `source`, `sample`, and the connection-pair wrapper are retired rather than promoted into the new HTTP contract. |

**Explicit organization gaps.** `org-office/test-vectors/deferred-org-commands.json`
lists all 104 deferred cases by name: 97 need an unimplemented project/area command
in setup or execution; seven more require area-state/reconciliation support in
otherwise supported commands. The canonical reader covers every other case, checks
the total and exact manifest membership, verifies deferred methods are unavailable
and checks area writes are refused without mutation. Once B1-10 supports a missing
capability these sentinels fail, requiring its positive cases to be enabled. The
original positive expectations remain in the shared fixture and in the legacy
mock runners until those runners are retired; no deferred case is claimed as a
working real-server feature. Missing capabilities remain explicitly unavailable.

**Work-vector differences.** Short legacy change/link aliases have explicit fixed
ULID inputs in each affected step's `canonical.call`, preserving equality for
retry tests and leaving blank/unknown IDs invalid. No runtime ID or version
substitution occurs. Each override has a contract reason; legacy calls and
expectations remain unchanged. The canonical library's versioned waiting
events, omitted default wire fields and documented error precedence must be
asserted explicitly rather than changing production behavior to match the mock.
Task `reopen_count` is not the bug reopen counter in the canonical contract.
Adding/removing a fix link changes the task; the bug's `fixed_by` is a read model,
not an invented extra versioned bug update. Preserve those distinctions in both
the vector runner and B1-11 gateway integration.

**`real.rs` error and retry disposition.** Preserve the behavior, not desktop text
that tells a browser to manipulate local installation:

| Existing result | Meaning to retain in gateway/client integration |
| --- | --- |
| Office `ApiError` | Pass through the typed domain refusal unchanged; authorization/validation stays server-owned. |
| `not_ready` before admission | Bounded backoff is safe only when the server guarantees nothing ran. After the deadline, show unavailable/starting; do not restart a healthy host. |
| Too-large request before send | Validation/size refusal, not an unknown committed mutation. The gateway contract chooses its typed code. |
| Wrong version, unreachable office, invalid endpoint, malformed response | Explicit unavailable/update/protocol failure; never substitute mock data or infer success. |
| Mutation timeout/disconnect/I/O after possible send | Preserve unknown outcome and the same command/change ID for lookup/retry. `create_organization` was deliberately sent once; `org_command` retained its `command_id`. Do not convert uncertain outcomes to a fresh operation. |
| Lost cached connection | Evict only the exact failed `Arc<Client>`; another caller may already have replaced it. Drop outside the cache lock. Reconnect/re-resolve scope once, with bounded resource use. |
| Starting/offline/pending/unavailable/update-needed status | Show the reason/freshness and avoid futile calls; a remembered status may be refreshed once. Status is not authorization and must not choose an office belonging to another organization. |
| Restart conflict (already starting) | Idempotent acknowledgement, not an instruction to restart the host again. Keep permission/capability failures distinct. |

**Tests to reproduce when the gateway/stream replaces the shell.** Keep the
`org_feed` tests for snapshot-position subscription, unknown events, reconnect,
`snapshot_required`, immediate-close backoff, one liveness transition, stop,
bounded Retry and Retry-during-connect. Keep the five `status_feed` cases for
list/event ordering, unreadable events, reconnect, missing capability and stop;
keep all three pacer cases. The `real.rs` tests also cover create-once after a lost
reply, no timeout resend, one failed-connection replacement, cross-organization
office refusal, malformed office IDs, unavailable reasons and fail-closed binding.
Retire installation-specific tests with their implementation; replace app binding
tests with X-02 principal/channel admission tests. This is a B1-08/F1-06 handoff,
not a claim that an HTTP listener or shared browser stream already exists.

F1-03 may remove shell implementation after these independent crate readers and
fixture references land and its production callers switch to the HTTP seam or
explicit unavailable states. It must keep domain-owned fixtures/readers, the
canonical wire payload gate and the documented future test obligations. No
Tauri-only type or mock library becomes a dependency of a canonical crate.

## 3. Production mock audit

The following are observed at the base commit, not implementation guesses:

| Area | Observed wiring | Required v1 result |
| --- | --- | --- |
| Office | `src/lib/office/api.ts` chooses `createMockOffice` outside Tauri | Real HTTP bootstrap/status/organization APIs; test-only injection |
| Organization | `src/lib/org/api.ts` selects a Tauri implementation or browser mock | Authenticated snapshot/command/feed APIs with existing version checks |
| Work | `src/lib/work/index.ts` selects browser mock or shell path; comments say the real office work surface is pending | Audit and implement each advertised task/bug/request operation; API outage never becomes sample work |
| Chat | `src/lib/chat/index.ts` always exports `createMockChat` | Real conversation/message operations or an explicit unavailable feature state; no fabricated reply |
| Processes | `src/lib/modules/processes/world.ts` builds `sampleWorld` | Connect production views to real process documents when supported; keep samples only in fixtures |
| Copilot and other staged features | Explicit mock modules exist; no complete backend audit is claimed here | Inventory consumers during implementation; report unsupported capabilities instead of making sample results look real |

The pivot does not promise that every mocked feature is implemented by replacing
transport alone. Retained v1 requirements still need real server behavior and
acceptance evidence; features deliberately deferred need an explicit spec change.

## 4. Keep and adapt the server foundations

| Existing area | Decision |
| --- | --- |
| `apps/org-office-host/` | Retain catalog, supervision, backoff, locks, key custody and crash recovery. Replace bundle-relative resolution, fixed home, global service name, app-only peer admission and account-wide key cleanup with instance-scoped equivalents. |
| `apps/org-office-host/src/app_auth.rs` and related code-signing checks | Retire signed-desktop authorization. Retain trusted helper admission and private key handoff. Standalone signed packaging and Linux credentials need real validation. |
| `apps/org-server/` | Retain the renamed worker (Cargo package/binary `org-server`) and durable message/domain dispatch. Keep browser HTTP/TLS in the planned instance `org-gateway`; compose local IPC, runtime and replication at the explicit office boundary. |
| `crates/org-office/` | Retain canonical rules, SQLCipher storage, one writer, transactional outbox/sequence allocation and private filesystem handling. |
| `crates/org-client/`, `org-proto/`, `org-office-launch/` | Retain local agent/host/worker contracts where needed. Change instance discovery and browser principal authorization; remove unused app-only calls/types and tests in the same change. Versioning for the new release remains; old desktop callers need no bridge. |
| `apps/org-agent/`, `crates/org-mailbox/` | Retain employee tooling and messaging semantics. Bind from injected instance/office/session identity; do not choose the wrong default instance from ambient HOME. Install the small helper from the pinned release. |
| `crates/org-spool/` | Keep only its explicitly supported offline/person-started CLI behavior and tests. Office-managed agents cannot bypass binding through spool fallback. Legacy aliases are not required; preserve live Phase A coordination through ADR-0003's conditional verified cutover before switching that team. |
| `crates/org-runtime/`, `org-work/`, `org-copilot/` | Retain contracts/domain logic. Runtime adapters/session orchestration remain necessary; a static web server is not their implementation. |
| Proposed `org-sync`, `org-sync-proto`, `org-pki` | Implement the approved office semantics; these crates are absent on the inspected branch. No existing replication implementation can be merely moved. |

## 5. Documentation and acceptance changes

| Source | New precedence / disposition |
| --- | --- |
| ADR-0001 | Replace local desktop client, app bundle and Tauri bridge with ADR-0005; preserve offices/domain/storage ownership |
| ADR-0002 | Expand platform key custody/crypto linkage and instance scope; preserve encryption, private SQL and durability |
| ADR-0003 | Human browser authorization replaces app-token entry points; bridge lifetime/credentials can no longer depend on a desktop window; keep agent binding and message permissions |
| `offices-hq-and-satellites.md`, `office-host-boundary.md` | Browser connects to HQ; enrollment has CLI and web approval; namespace the host; implement remote sync in v1; preserve office ownership and transport invariants |
| `create-organization.md` | Replace desktop launch/SMAppService sequence with authenticated HQ setup/create flow |
| `offices-and-teams-ux.md` | Retire MENU criteria to v2; rewrite OPEN/JOIN/OFC access semantics and remove the browser-machine locality assumption |
| `our-org-in-the-app.md` | Retire IMP-1 through IMP-11 and old-team import; retain roster/hiring/lifecycle criteria and create a fresh test organization |
| `feature-requests-ux.md` | Keep in-browser capture; global shortcut/menu capture is v2; remove the native offline request outbox requirement |
| `dogfood-run.md`, `local-hq.md` | Fresh-instance acceptance and standalone setup plan; no old-team migration or bundle launch prerequisites |
| `agent-sessions.md` | Resolve roots per instance; supervision lives on the office machine; native embedded terminal becomes explicit local CLI takeover or a separately designed browser feature |
| Root/desktop README and TECH-STACK | Describe target vs current implementation honestly and point to this pivot; implementation later removes Tauri build instructions |

## 6. Deletion acceptance

Before considering the implementation complete:

- Retained browser sources/package use `apps/org-ui` / `org-ui`; server sources,
  Cargo package, worker executable and Rust imports use `apps/org-server`,
  `org-server` and `org_server`. Workspace, test, build and packaging references
  agree. Historical audit references are clearly labeled.
- The domain-module/vertical-slice reorganization follows the public boundaries
  and shared-code rules in [project structure](project-structure.md).

- The Rust workspace and frontend dependency graph contain no Tauri member or
  package. Browser production imports contain no `@tauri-apps/*`, `invoke`, native
  event listener, Tauri-global test or desktop-only service operation.
- No production path selects mocks because the API failed or Tauri is absent.
  Retained v1 flows have real API tests; fixtures are explicit and cannot be enabled
  accidentally in the release frontend.
- Existing-team import, native capture, menu/tray and bundle installation controls
  are gone from v1 navigation, scripts, tests and release packaging.
- Per-office locking, authenticated agent tooling, key custody, durability, bounded
  resource cleanup and meaningful domain tests survive the removal.
- Named-instance setup, macOS/Linux packaging and the HQ/satellite walkthrough pass
  [SRV-1 through SRV-32](../specs/server-installation-and-access-ux.md#6-acceptance-criteria).

No source deletions, Cargo/Clippy checks or runtime acceptance passes are claimed by
this documentation-only audit. The audit's evidence is the inspected source and
contracts; implementation must run the applicable repository checks.
