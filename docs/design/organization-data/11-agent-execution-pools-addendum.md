# Agent execution and subscription pools: implementation contract

Status: **implementation specification, revision 5**. This document defines the additive data model, process ownership, commands, state transitions and acceptance criteria for this feature. Implement the decisions here; do not substitute an independently chosen process topology or retry policy.

The SQL extension in §12 applies after the structural `schema.sql`; it is integration input for the implementation team's migration, not a migration already applied to the application. Existing employee IDs and organization/team/project/task relationships are preserved.

Requirements source: owner-supplied `requirements_design_document_subscription_pool_model_profile_architecture.md`, SHA-256 `c3189b7af897be77cc9e8d4214edcc71a66039f3c14a95f6d08d3da5678c43dc`. Provider commands/model names in that source are illustrative. Adapter conformance tests establish what an installed version supports; unsupported capability returns a defined error instead of leaving an architecture decision open.

## 1. Fixed decisions and scope

| Topic | Required implementation |
| --- | --- |
| Identity | An agent is an existing employee of kind agent, extended by exactly zero or one `agent_execution_configs` row. People have no execution configuration. |
| Runtime topology | One `agentic-org-office` process per office. Its internal agent supervisor hosts a logical controller and one main-process slot per placed agent. There is no separate agent-supervisor daemon. |
| Launch authority | The supervisor alone selects routes, prepares child environments, launches/drives/stops agent processes and recovers its children. |
| Pool configuration | Agents select one model profile and canonical model ID. Profiles contain only an ordered list of connection IDs, never a manual model map. |
| Subscription identity | One organization-wide connection per provider subscription. Offices bind that same connection locally; pools cannot manufacture additional quota by naming the same subscription twice. |
| Connection capabilities | Discover/cache entitlement and model-ID translation per connection and office. Unknown or stale auth/model evidence cannot qualify a route. |
| Shared quota | HQ's organization service coordinates turn permits across offices. Per-connection concurrency defaults to one and is explicitly configurable. |
| Failover | Same target model, first eligible connection by priority. Each connection is tried at most once per dispatch cycle. Replay/resume requires the proof rules in §7. |
| Fast iteration | Reuse an idle verified process when its connection, model, workspace and launch digest match. Otherwise perform a controlled switch. No Git fetch, model discovery or authentication flow runs on the selection hot path. |
| Persistence | Organization config in Git; local auth and execution journal outside Git; SQLite is a projection. Accounting remains durable as defined in [usage accounting](10-usage-and-cost-accounting.md). |
| Delivery scope | Configuration, discovery, explicit turn execution, supervision, permits, safe switching and accounting capture. Business-process engines, automatic scheduling, delegated employees, memory consolidation and budget-triggered pausing are excluded. |

The owner-approved executable structure uses agentic-org as the human CLI/service host, agentic-org-web as the browser server, agentic-org-office as the office worker, and agent-os as the separate agent CLI. It supersedes revision 1's dedicated supervisor process. The existing-employee extension and supervision responsibilities remain. These are target names and boundaries; this addendum does not rename implemented packages or claim unfinished components exist.

## 2. Executables, modules and exclusive ownership

| Target executable | Responsibility | Inspected v1 source / disposition |
| --- | --- | --- |
| `agentic-org` | Human management CLI; `serve` runs the long-running installation/service host, release manager and office-key custodian. | The planned setup CLI plus existing `org-office-host` lifecycle behavior. Extract reusable host behavior into a library; do not link app implementations together. |
| `agentic-org-web` | Browser HTTPS, authentication, static org-ui assets, browser APIs and live events. One per installation hosting HQ offices; none for a satellite-only installation. | Planned `org-gateway`; rename the target executable, preserving its restricted authority. |
| `agentic-org-office` | One HQ or satellite office worker, with data services, replication and internal agent supervision. | Existing `org-server`; executable rename is implementation work. |
| `agent-os` | Small agent CLI for authorized messages, work operations and other tools. Invoked on demand, not a permanent daemon. | Existing `org-agent`; keep distinct from the human CLI and host key-access identity. |

These names define executables, not a mandatory rename of every org-* library or SQL table. org-ui stays a static asset build, with no Node server. Provider agent CLIs are external runtime dependencies. An API-backed agent may require an isolated worker implementing a complete agent loop; add that child only when its backend ships, not as another mandatory service. No separate updater, synchronization or agent-supervisor daemon is required.

```mermaid
flowchart TD
    Manager[OS service manager] --> Host[agentic-org serve]
    Host --> Web[agentic-org-web: HQ browser service]
    Host --> API
    Web -->|typed authorized operations| API
    subgraph Office[agentic-org-office: one process per office]
        API[Office APIs and single data writer] --> Supervisor[Agent supervisor module]
        Supervisor --> Router[Connection router]
        Router --> Runner[Session runner]
        Runner --> OS[org-supervise library]
    end
    OS --> Runtime[Native agent or API-backed worker]
    Runtime --> AgentCLI[agent-os: invoked as a tool]
    AgentCLI -->|employee-scoped local operations| API
```

The OS service manager starts/restarts agentic-org serve. Its host module starts and monitors office workers and the optional web child. Each office worker acquires its exclusive office/ownership locks, replays its execution journal and reconciles marked children before admitting starts. A worker restart requires agent recovery; this version does not promise uninterrupted agent execution across worker restarts. Host and worker generations are distinct from employee placement generation. Host shutdown stops workers in order; host reconciliation must not race the live worker's ownership of agent children.

Inside agentic-org-office, the agent supervisor owns employee lifecycle, readiness, environment setup and recovery. The connection router owns pool selection, cooldown/permit decisions and failover. The session runner owns protocol pumping and invokes org-supervise for OS operations. Existing org-runtime remains the adapter contract; the planned org-office-sessions integration composes these capabilities without changing pure domain modules into process-control code. Binaries remain thin. New shared wire/launch contracts follow the repository's existing contract ownership/review rules.

The worker has one SQLite writer and one agent recovery owner. Its internal supervisor submits journal/projection changes through the office data service; it does not open another writer or communicate with a sibling supervisor daemon. The execution journal remains authoritative for process ownership, turn/attempt changes and command receipts; SQL rows are projections. Integrate existing owned-recovery records into this path before enabling launches. Organization configuration and HQ coordination keep their authorities (§3/§6).

**Release lifecycle.** agentic-org owns online downloads and offline local-bundle installation. Stage and verify a complete platform release, including every framework executable and web assets, before activation. Check signed metadata and checksums against installed trust; offline install/upgrade performs no network requests. Startup uses the pinned installed release even if update checking fails. Background checks do not imply immediate activation: apply the configured policy, drain affected work, switch the release pin, restart through the OS service manager and verify readiness. Preserve the prior release and compatible recoverable data state; executable-only rollback after an incompatible schema change is forbidden. Updating the running host itself needs platform-aware replacement/restart. An offline bundle is not a promise of offline cloud-model access.

**Satellite operation.** HQ and satellite are roles of the same office executable, not different binaries. A satellite enrolls once and reconnects outbound to HQ's authenticated federation endpoint using its office identity; browser authentication and federation TLS remain separate boundaries. Agent tools call the local worker through agent-os. The worker replicates relevant shared data and uses local repository caches/worktrees; teams/projects remain organization-wide. Disconnected tasks/features can be saved with stable IDs and pending readable numbers, as defined in document 06. The current HQ permit contract still blocks new model turns without coordinator access and stops active turns when renewal cannot be maintained. Reserved offline capacity is a later explicit extension, not an implied exception. HQ/satellite protocol compatibility is checked during independently activated installation updates.

The host retains office-key custody. The web process and agent-os receive only their scoped capabilities, never host key-access entitlement or database keys. Moving host behavior into agentic-org serve must preserve the existing platform credential and signed-launch boundaries; reusing an executable name does not make human, host and agent authority interchangeable.

### 2.1 Initial setup creates HQ only

```bash
agentic-org setup
```

After the verified platform executable is obtained, this single command selects/reserves the named installation, prepares a complete verified release, creates HQ and its administrator/organization through protected bootstrap operations, registers/starts the per-user service and probes readiness. Show the HQ browser URL and separate provider-runtime readiness. Preserve the recovery-code delivery/deferred-collection rules in the [installation UX](../../specs/server-installation-and-access-ux.md). No separate install or service-install command is required. The service manager runs agentic-org serve; foreground serve uses the same host behavior.

The initial wizard, config schema and automation accept HQ creation only. No satellite choice, invitation or --hq join option appears. Reject satellite requests before provisioning; never reinterpret initial setup as enrollment or convert an existing satellite-only installation into HQ. Satellites are added later from authenticated HQ UI or an HQ-issued onboarding command, as a separate flow. That flow may reuse installation/service internals; it is not a mode or prerequisite of initial setup. Its public command spelling is outside this setup contract.

```bash
agentic-org setup --bundle ./release-linux-x64.tar.gz --offline
```

Offline setup verifies a complete local bundle, provisions local HQ where platform requirements are present and performs no external requests. Background processes inherit an offline policy until online operation is explicitly enabled; private local control remains available. Missing prerequisites produce actionable errors, not downloads. Satellite enrollment is outside initial setup in both online and offline modes.

| Command | User-facing meaning |
| --- | --- |
| agentic-org setup | Complete initial HQ setup, including service registration/start. |
| agentic-org serve | Run the configured host in the foreground; also the OS service entry point. |
| agentic-org service start / stop / status | Maintain an already configured installation; registration is automatic during setup. |

Use one shared implementation for release verification and service provisioning, not nested shell commands. Every irreversible setup stage has a stable operation ID and durable marker. Repeating/interruption/concurrency resumes existing HQ identity/configuration/data/service without duplicates, implicit upgrades or role changes. A missing service manager leaves completed configuration intact and reports foreground serve instructions. Credentials/recovery material never enter diagnostic output. [Standalone setup](../standalone-server.md#4-setup-and-service-lifecycle) and its UX are the shared installation contract, rather than a separate installer defined by this addendum.

## 3. Storage authority and exact paths

```text
<organization repository>/
├── provider-connections/<connection-id>.yaml
├── model-profiles/<profile-id>.yaml
└── teams/<team-id>/.../agents/<employee-id>/
    ├── profile.md
    └── execution.md

<data-root>/<organization-id>/durable/
├── authority/agent-execution/              # HQ placement generations, permits and cooldowns
└── local/offices/<office-id>/
    ├── connections/<connection-id>/binding.yaml
    ├── auth/<connection-id>/               # Private CLI state, or external vault binding
    └── supervision/
        ├── supervisor.lock
        ├── journal/<sequence>.json
        ├── turns/<turn-id>/input.json
        ├── checkpoints/<checkpoint-id>.json
        └── tools/<operation-id>.json

<data-root>/<organization-id>/runtime/<office-id>/
├── supervisor/generated/<process-id>/     # Rendered adapter/native-agent configuration
└── teams/.../agents/<employee-id>/...      # Existing task/project worktrees
```

| Projection tables | Canonical source |
| --- | --- |
| `provider_connections`, `model_profiles` | Same-named YAML fields and common attribution; profiles additionally contain `connection_ids[]`. |
| `model_profile_connections` | Profile `connection_ids[]`; zero-based array position plus one becomes priority. |
| `agent_execution_configs` | `execution.md` front matter: employee/profile/model IDs, revision/schema, enabled, tools_config; body is system_prompt_markdown. Common attribution follows [record contracts](08-record-contracts.md). |
| `office_connection_bindings` | Local binding YAML with same-named fields. Never shared Git content. |
| `office_connection_state`, `office_connection_models` | Supervisor-owned discovery/auth observations, rebuilt from verified probes after cache loss. Their journal snapshots improve startup but are not fresh proof after restart. |
| `agent_placements.placement_generation` | HQ agent-execution journal; current placement row remains the existing employee-to-office projection. |
| `connection_quota_state`, `connection_turn_permits` | HQ agent-execution journal; atomically committed before permit acknowledgement. |
| `agent_execution_turns`, `agent_process_instances`, `agent_execution_attempts`, `supervisor_command_receipts` | Supervisor journal transactions and immutable input/checkpoint/tool files. |

Journal records are ordered UTF-8 JSON transaction envelopes: `{schema_version, sequence, operation_id, payload_sha256, mutations, response}`. Write a complete transaction to a temporary file, fsync it, atomically rename to its sequence name, then fsync the parent directory before acknowledgement. Mutations contain full same-named row snapshots or explicit projection removals. A projector applies each sequence once in one SQLite transaction; a gap or checksum conflict blocks new execution until recovered. Asynchronous fact transactions use null operation_id, payload_sha256 and response and create no command receipt. Input/checkpoint/tool files referenced by a transaction are fsynced before publishing it. Canonical JSON expands defaults/nulls, sorts keys and uses compact UTF-8 encoding; digests use the [accounting](10-usage-and-cost-accounting.md) SHA-256 convention.

Credentials never enter organization Git, command payloads, journal events or logs. The local binding holds only a vault/keychain reference or a private auth-directory path. Auth directories are owned by the office user with mode 0700; generated files are 0600. Rebind on a new machine and probe again. Native auth variables require a reviewed adapter allowlist; HTTP keys are resolved into the approved provider-client context, not ordinary tool environments.

Keep live ownership evidence until cleanup is proven. A fresh office cache replays identities/config, HQ authority data and supervisor journals, then performs marked-process recovery before starts. If local recovery evidence or input/checkpoints were lost while a child may exist, return `RECOVERY_REQUIRED`; do not infer that deletion of a cache ended the process. Recovery on a different machine requires a new placement generation and the old office's cleanup acknowledgement or an explicitly verified machine fence; automatic timeout-only reassignment is outside this version.

This explicitly extends document 04's runtime-only placement contract with a small durable coordination/fencing journal outside Git; employee home-team and organization membership remain unchanged. HQ owns monotonically increasing placement_generation per employee. Moving an agent first requires the previous supervisor's verified cleanup acknowledgement or an explicitly verified machine fence; HQ then journals the new office and incremented generation atomically. Every command, permit and attempt carries that generation. The migration default of one is a projection bootstrap, not permission to start a legacy process: recovery must finish before HQ admits its first generation. Lost authority evidence blocks starts until restored. Historical attempts retain their original office/generation independently of current placement.

## 4. Configuration transactions and discovery

All IDs use the existing stable-ID contract. `workspace_id` from the upload maps to organization_id. Every foreign reference is organization-scoped. Provider/profile keys are organization-unique readable strings; no new human number counter is required.

For `subscription_cli`, `subscription_identity` is required: the provider adapter's canonical, non-secret identity for the actual subscription entitlement/quota account, not a connection label, credential path, model name or office ID. HQ registers it once per `(organization_id, provider_type, subscription_identity)`. Two different connection IDs for that same subscription are rejected with `SUBSCRIPTION_ALREADY_REGISTERED`, returning the existing connection ID. Disabled/archived definitions also reserve their identity; reuse/re-enable the existing definition. One connection may appear in several profiles and have one local binding per office, while all those uses share its HQ permit/cooldown state. Multiple subscription connections in a pool represent distinct subscriptions.

Identity normalization and registration happen before creating the default profile. Provider adapters use a canonical provider namespace and verified subscription identity; if the provider exposes no identity API, an explicit operator-attested mapping to the registered subscription is required. Unverified aliases cannot become runnable routes. SQL enforces uniqueness of the registered identity, but cannot prove that two credential sets reach the same external account. Bind/refresh must compare account evidence with the registered identity and return `SUBSCRIPTION_IDENTITY_MISMATCH` on a different subscription, or `SUBSCRIPTION_IDENTITY_UNVERIFIED` without evidence/attestation. These states are not ready. Credential rotation preserves identity; changing the subscription requires selecting/registering its own connection. API/cloud connections that do not represent a subscription leave this field null; this rule does not claim to identify all provider-wide API rate-limit groups.

Connection creation is one idempotent organization transaction: create the connection, an auto-generated model profile, and its one membership. The profile records generated_for_connection_id. Exactly one generated default is allowed per connection. Auto-generated profiles contain exactly their source connection; converting one to a manual pool clears the generated designation in that same revisioned transaction. Profile membership is 1..64 connections in contiguous priority order. Reorder by replacing the membership set inside one transaction, then validate before commit; do not perform individually visible priority swaps.

Archiving or deleting an assigned profile returns `PROFILE_IN_USE` with affected employee IDs. Referenced connections cannot be deleted through a membership cascade. Disable/retire definitions with history and explicitly reassign/detach before deletion. A registered subscription's provider/identity pair is immutable. Other provider/auth-mode/driver/endpoint changes are revisioned and must preserve any registered subscription identity; a supervisor never mutates a live child's environment to adopt them. Missing execution config means an employee is not runnable, not that the framework chooses a default model.

The supervisor probes at startup, after binding changes, on explicit RefreshConnection and every 60 seconds while a connection is in use. Auth/model observations are valid for at most 300 seconds. A changed binding revision invalidates all previous observations immediately. Driver preflight must prove the installed version and the intended account context; provider account identity evidence is retained in account_identity_ref, with explicit operator attestation required when a provider exposes no account identity API. A public `/models` list alone is not entitlement proof.

Canonical model IDs and driver-specific model/deployment IDs are stored on discovered connection observations. Profiles contain no manual per-model mapping. The SQL union view lists discovered confirmed support per office; ListProfileModels returns `{model_id, supporting_connections, first_priority, as_of, stale, ready_connections}` per model. ready_connections counts locally fresh compatible evidence, never guarantees quota admission. Employee-specific policy readiness is returned by GetAgentReadiness. Requested models remain valid declarative configuration while discovery is pending, with readiness `MODEL_UNVERIFIED`. They are not silently replaced or removed from agent files.

```yaml
# execution.md front matter; common attribution fields omitted here for readability
schema_version: 1
employee_id: existing-agent-employee-id
revision: 1
enabled: true
model_profile_id: subscription-pool-id
model_id: canonical-target-model-id
tools_config: [approved-shell-tool-id, approved-org-tool-id]
```

The associated organization files have the following serialization (nullable common attribution fields omitted, created_by_kind and updated_by_kind default to unknown):

```yaml
# provider-connections/subscription-a.yaml
organization_id: example-org-id
id: subscription-a
schema_version: 1
revision: 1
key: subscription-a
name: Subscription A
provider_type: example-provider
auth_mode: subscription_cli
subscription_identity: provider-canonical-subscription-a
runtime_driver: verified-native-driver
endpoint_url: null
enabled: true
archived: false
max_concurrent_turns: 1
```

```yaml
# model-profiles/primary-pool.yaml
organization_id: example-org-id
id: primary-pool
schema_version: 1
revision: 1
key: primary-pool
name: Primary pool
description: Same-model fallback across two verified accounts
is_auto_generated: false
generated_for_connection_id: null
archived: false
connection_ids: [subscription-a, subscription-b]
```

subscription-b must represent a different verified subscription, with its own connection definition and office-local binding before it is eligible. Another office using subscription-a binds the existing subscription-a connection; it does not create subscription-c for that same subscription. This manually assembled pool coexists with each connection's generated single-member default profile. Boolean YAML values become SQLite 0/1; tools_config and connection_ids serialize arrays rather than JSON strings. File and record IDs must match; reject unknown fields or unsupported schema versions. Example IDs denote existing stable IDs, not a new ID format.

For a concrete dispatch: an agent in the east office requests the canonical target model from primary-pool. Both connections have fresh confirmed entitlement. HQ grants the subscription-a permit; its first request returns 429 before any effect. The supervisor records cooldown/attempt usage, verifies cleanup, releases that permit, then requests subscription-b's permit and launches with its isolated binding. Success produces one logical turn, two attempts and route-specific usage. If the first attempt had an uncertain tool result, the same example stops at recovery_required instead of starting subscription-b.

The body is the system prompt. Existing employee identity, role, home team and reporting lines remain in profile.md. Effective role/process authority is resolved by the office and snapshotted with tool/policy revisions; execution.md cannot grant itself extra permissions.

## 5. Commands and admission

The office exposes execution operations through its existing authenticated service boundary and dispatches typed commands to the internal supervisor. There is no additional supervisor socket, framing protocol or server-to-supervisor service credential. Existing local IPC and federation transports retain their own framing and peer admission. Execution command content is bounded to 1 MiB; larger inputs use approved immutable local content references. Validate the admitted principal, organization, office and operation scope before internal dispatch. An employee token permits only the operations explicitly authorized for that employee; it is not host administration authority.

Every internal command envelope contains `{protocol_version:1, organization_id, office_id, operation_id, command, payload, payload_sha256}`; protocol_version is the internal execution-contract version, independent of existing transport API versions. Validate scope/authentication before admission. payload_sha256 hashes canonical `{command, organization_id, office_id, payload}` only; transport capabilities and worker generation are excluded so an original operation can be retried after restart. Mutation payloads include expected object revisions/placement generation; responses are `{operation_id, outcome, code, data}`. Journal the result and associated state atomically before replying. Same ID/digest returns the original response; same ID/different payload returns `IDEMPOTENCY_CONFLICT` without side effects. A lost acknowledgement is resolved by `GetOperation(operation_id)` or the same-ID retry, never a newly generated command. Keep receipts while their turn/process/config revision is retained; no time-only dedupe expiry is introduced.

| Command | Required payload / result |
| --- | --- |
| `RunTurn` | turn_id, employee_id, placement_generation, expected_agent_config_revision, expected_profile_revision, task_id or null, workspace_key, immutable input_ref/input_sha256. Returns accepted turn identity. A second open turn for the employee returns BUSY. |
| `ResumeTurn` | turn_id, expected_turn_revision; recovery-required turns also require recovery `{checkpoint_ref, receipt_dispositions:[{operation_id, outcome, result_ref, evidence_ref}]}`. Outcome is confirmed_completed or confirmed_not_started, with evidence verified by the tool broker/operator recovery authority. Unknown outcomes remain recovery_required. Retries a blocked turn or resumes only after validated reconciliation. Preserves turn ID, model and workspace. |
| `CancelTurn` | turn_id, expected_turn_revision. Stops current work and becomes cancelled only after completion/cleanup facts are known. |
| `StopAgent` | employee_id, placement_generation. Stops active and idle main processes. It creates no future automatic wake or budget policy. |
| `BindConnection` | connection_id, expected_binding_revision, local credential reference, verified executable path where required. Invalidates observations. Raw credentials use the local auth broker, not this payload. |
| `RefreshConnection` | connection_id. Returns refreshed status/catalog or structured unavailable status. Never starts an employee LLM turn. |
| Queries | GetOperation, GetTurn, GetAgentReadiness, ListProfileModels. Queries have no mutation receipt and expose no secret values. |

The office validates placement/config revisions and dispatches through the internal supervisor against the same admitted snapshot. No new starts occur while the worker is stopping, recovering or has lost its host-control authority. The existing host/worker launch contract controls that lifecycle; there is no separate server-to-supervisor lease. HQ turn-permit renewal remains mandatory under §6. Unresolved cleanup retains the process/turn slot. Client deadlines of 10 seconds are response deadlines only; they do not prove a command failed or cancel accepted work.

Run input is a private immutable JSON file containing the resolved employee/role/system instructions, visible messages/attachments by content reference, approved tool/policy manifests, work scope, requested model and ordered pool snapshot. Large content uses content-addressed local references; it is not embedded past the frame limit. Resolve paths against the office's approved roots and reject traversal/symlink substitution. The same turn keeps its original target model, profile order, workspace and input digest through every attempt. Connection disabling, revoked authority and invalid credentials override old eligibility; ordinary profile/prompt changes take effect on the next turn. Resume does not silently broaden the admitted policy.

## 6. Shared quota and deterministic selection

HQ's organization service is the single permit/cooldown coordinator. It journals before acknowledging and updates its projection in one `BEGIN IMMEDIATE` transaction: expire overdue active permits to release_pending; read connection and applicable model cooldowns; count active plus release_pending permits; grant only below max_concurrent_turns. Permission to proceed is a scoped opaque permit ID bound to employee/office/placement/model and coordinator generation. There is no global counter on the agent and no quota authority in an office's cached healthy flag.

Permits last 60 seconds and renew every 10 seconds while a model turn is active. Compute local deadlines conservatively from the monotonic request-send time and the returned remaining duration. Start stop/recovery with at least 20 seconds remaining when renewal fails. **Expiry does not free capacity:** release_pending continues to count until a supervisor proves the model turn ended with no outstanding work, or proves its process/descendants gone. Unknown cleanup is not a release acknowledgement. Warm verified-idle processes consume no turn permit. Coordinator restart replays its journal and marks prior active permits release_pending before issuing new permits; affected supervisors stop/reconcile rather than reusing an old coordinator generation.

A 429/quota failure is connection-wide unless verified provider evidence identifies a model-specific window. Use the provider reset/Retry-After value when present; otherwise cooldown is 900 seconds. Cooldown expiry permits another guarded attempt if local auth/entitlement evidence is still fresh; it is not an assertion that quota is known available. Unsupported quota probes leave quota status unknown and can still obtain a bounded permit, with that uncertainty exposed. Auth-expired is an office-binding observation until verified reauthentication. Never convert a tool/policy denial into permission to try a less restrictive driver.

For each RunTurn/ResumeTurn dispatch cycle:

1. Read the frozen ordered pool and admitted policy. Reject archived profile, disabled execution config, inactive employee or stale placement.
2. In ascending priority, check connection enabled/non-archived, local binding, exact binding revision, installed driver/verified version, fresh auth readiness, confirmed model entitlement and policy/tool/context compatibility. Unknown/stale evidence produces a skip code and asynchronous refresh, not hot-path discovery.
3. Ask HQ for a turn permit. A full/cooling-down connection is skipped; coordinator unavailability blocks dispatch rather than bypassing global admission.
4. Reserve the employee's exclusive process slot. Reuse an idle process only if connection, driver/version, canonical model, workspace and launch digest match. The digest covers connection definition revision/endpoint, canonical and resolved model IDs, driver version/executable fingerprint, workspace, resolved instructions/tool-policy revisions and binding revision, never plaintext secret values. Validate the process row matches the admitted connection/driver/model before attaching an attempt.
5. Otherwise stop/reap/recover the previous process, create a new mark/instance, fsync planned ownership, prepare the explicit environment/launch plan, spawn through org-supervise, fsync PID/start/boot identity, then admit input. No replacement starts while cleanup is unresolved.
6. Journal attempt and turn transitions, pump protocol, persist usage before final completion acknowledgement, release the permit after a verified terminal/idle boundary, and keep the process warm for compatible subsequent turns. Configuration changes do not rewrite the current attempt.

Each connection is attempted at most once in a dispatch cycle. Exhaustion yields `POOL_EXHAUSTED` with ordered per-connection reasons and any known earliest cooldown time; the turn becomes blocked. Explicit ResumeTurn starts another bounded cycle, not an unbounded retry loop. Concurrent input is BUSY; this version has no implicit scheduling queue.

## 7. Failover, checkpoints and side effects

Retry-eligible driver errors are rate_limited, quota_exhausted, auth_expired and model_unavailable. Invalid input, policy denial, unsupported capability and unknown protocol/provider errors are not automatic retry instructions. A switching attempt retains usage already consumed, ends/reconciles the old process where required, releases/quarantines its permit and selects the next eligible connection for the same logical turn.

| Proven execution boundary | Required action |
| --- | --- |
| Before any external/tool effect; adapter proves effect_state none | Replay the frozen turn on the next connection after old-process cleanup. Provisional streamed text is attempt output, not a second committed final reply. |
| Effects occurred; a verified compatible checkpoint includes completed operation receipts and no unknown in-flight effect | Resume after that checkpoint. Do not replay completed tools. |
| Effect state unknown, missing checkpoint/receipt or unsupported cross-driver/account resume | Stop/reconcile and set RECOVERY_REQUIRED. Keep the task/worktree and evidence; do not blindly retry. |

A checkpoint is an immutable private JSON object with schema_version, checkpoint_id, turn_id, attempt_id, employee_id, workspace_key, input/policy digests, visible message/event watermark, completed tool receipt references, any native conversation-state reference, format_id, portability and content_sha256. Portability is one of same_process, same_connection, same_driver or portable. A destination adapter must explicitly accept that format/version/scope and pass its conformance tests; same vendor/model name is insufficient. Missing context-window capacity returns CONTEXT_LIMIT, not silent truncation. Opaque account-bound CLI state cannot be relabelled portable.

Every brokered effect has a durable tool receipt `{operation_id, payload_sha256, turn_id, status, result_ref}`. Journal pending before invocation and terminal outcome before acknowledging its result. Same operation/payload returns the recorded outcome; a different payload conflicts. A crash between side effect and receipt completion produces outcome_unknown and requires reconciliation, not an automatic second invocation. Native unbrokered tools need verified start/completion visibility; otherwise the adapter marks effects unknown from the point visibility is lost. Message sends, repository commits and deployment calls are effects too. Pending approvals from an old attempt are cancelled and cannot authorize a replacement.

Native CLI environments change only on a new process launch. When an API-backed backend ships, its isolated HTTP agent worker owns the common agent loop and exposes a versioned turn/usage/tool protocol; a raw SDK completion is not a complete agent runtime. Supervisor-approved tool requests go through the tool broker. API secrets enter only the approved provider-client context; ordinary tool children do not inherit them. An in-process connection switch is allowed only when the worker advertises verified compatible switching and has no in-flight effect; the supervisor remains the sole switch authority.

This qualifies source FR-6 deliberately: automatic mid-turn failover is required at a proven safe replay/resume boundary. RECOVERY_REQUIRED is the specified fallback where the harness cannot prove safety, not an unimplemented decision for the team to invent.

## 8. Driver contract and failures

Each registered driver supplies versioned preflight/discovery, capabilities, launch planning, normalized protocol events, checkpoint export/import and stop instructions. Reuse org-runtime's sans-I/O separation: adapters plan/translate, the supervisor performs I/O and org-supervise owns OS control. The HTTP-worker protocol adapter follows the same boundary. The connection's runtime_driver selects a registered implementation; unknown or unverified installed versions cannot run.

Driver manifest requirements are auth modes, supported isolation mechanism, model discovery evidence, tool/policy enforcement, context limits, side-effect visibility, checkpoint formats/portability, usage semantics and stop/cleanup support. A runtime cannot become eligible merely because it answers text requests. The adapter test suite uses fake drivers for deterministic core tests and installed-version fixtures for every production driver. Do not execute the upload's onboarding commands or claim its example models/variables are supported without that evidence.

| Error code | Stable outcome |
| --- | --- |
| STALE_CONFIG / STALE_PLACEMENT / IDEMPOTENCY_CONFLICT | Reject before admission; no process or permit created. |
| PROFILE_IN_USE / INVALID_PROFILE_MEMBERSHIP | Reject configuration transaction atomically. |
| BINDING_MISSING / AUTH_EXPIRED / DRIVER_UNAVAILABLE / DRIVER_UNVERIFIED | Skip route; return readiness evidence and refresh/rebind action. |
| MODEL_UNVERIFIED / MODEL_NOT_ENTITLED / POLICY_UNSUPPORTED / CONTEXT_LIMIT | Skip route; never change the requested model or drop policy. |
| COORDINATOR_UNAVAILABLE / POOL_EXHAUSTED | Accepted turn becomes blocked; no uncontrolled start. |
| PROCESS_CLEANUP_PENDING / RECOVERY_REQUIRED | Keep ownership/open-turn slots until explicit reconciliation. |
| UNKNOWN_COMMAND_OUTCOME | Resolve original operation ID; do not resubmit a fresh start. |

## 9. States and accounting

```mermaid
stateDiagram-v2
    [*] --> accepted
    accepted --> running: route and permit admitted
    accepted --> blocked: no eligible route
    running --> switching: retry-eligible failure
    switching --> running: safe next attempt
    switching --> recovery_required: no proven safe continuation
    switching --> blocked: pool exhausted after verified cleanup
    running --> succeeded: verified completion and usage capture
    running --> failed: terminal non-retry error and reconciled effects
    accepted --> cancelled: cancellation
    running --> cancelled: cancellation with verified stop
    blocked --> accepted: explicit ResumeTurn
    recovery_required --> accepted: explicit validated recovery
    recovery_required --> cancelled: explicit reconciled cancellation
    succeeded --> [*]
    failed --> [*]
    cancelled --> [*]
```

Process states are planned → starting → idle/busy → stopping → gone; any uncertain exit becomes cleanup_required and retains the exclusive slot. Turns in accepted/running/switching/blocked/recovery_required are open. The SQL unique indexes enforce one open turn and one not-gone main process per employee. Transition validation occurs in journal transactions; arbitrary SQL updates are not a public execution API. Cancellation with unknown effects remains recovery_required until reconciled. Turn success is not task closure; task lifecycle remains a separate contract.

Every chargeable attempt captures actual consuming employee/one-off context and work attribution through [usage accounting](10-usage-and-cost-accounting.md). Add the following routing object to source_metadata_json before hashing each new event:

```json
{"routing":{"turn_id":"logical-turn-id","attempt_id":"attempt-id","process_instance_id":"process-id","model_profile_id":"profile-id","provider_connection_id":"selected-connection-id","runtime_driver":"driver-id","requested_model_id":"canonical-id","resolved_model_id":"driver-model-id","agent_config_revision":1,"profile_revision":1,"binding_revision":1}}
```

Provider/model and price_id describe the actual route used, not the first pool member. Retain usage on failed/partial attempts, preserve unknown counts and distinguish normalization of overlapping aggregate/component reports. A failover is not permission to count the same consumed event twice. Runtime attempt projections may later be discarded after termination and accounting handoff; minimal routing evidence remains in the durable usage archive without a foreign key to a live process. Do not rewrite old immutable events to backfill routing fields. Spending-limit enforcement and automatic budget pauses remain excluded; provider quota cooldown is a different mechanism.

## 10. Required acceptance criteria

| ID | Passing evidence |
| --- | --- |
| AE-01 | Person execution config rejected; existing employee IDs/handles/roles/team relationships remain unchanged. |
| AE-02 | Connection registration creates exactly one default pool/member under same-ID retries; generated profile invariants survive restore. |
| AE-03 | Duplicate/out-of-range priorities and cross-organization links fail; reorder is atomic; assigned/used profiles cannot be silently deleted. |
| AE-04 | Two simultaneous RunTurn commands for one employee admit at most one open turn/process; lost ACK replay starts no second child. |
| AE-05 | Isolated bindings select intended accounts; no secrets/global-environment mutation; binding revision invalidates cached readiness. |
| AE-06 | Union is derived; stale/unknown entitlement or unsupported policy does not become runnable; pool order is deterministic. |
| AE-07 | Two offices competing for one configured slot receive at most one active permit; expiry/unresolved cleanup does not free capacity. |
| AE-08 | 429 updates correct quota scope, default cooldown is 900 seconds, attempt count per cycle is bounded by pool size. |
| AE-09 | Pre-effect replay uses the next connection; recorded-effect resume does not repeat a tool; unknown effect enters recovery_required. |
| AE-10 | Crashes before spawn, after spawn before identity acknowledgement, during tools and during supervisor restart all retain evidence and block unsafe replacement. |
| AE-11 | Warm compatible process is reused; changed binding/model/workspace/policy digest causes controlled cleanup/relaunch. |
| AE-12 | Actual successful and failed route usage survives fresh-cache reconstruction; tokens/cost are not assigned to the configured primary when backup consumed them. |
| AE-13 | Cached candidate selection for up to 64 members has p95 below 5 ms over 10,000 warmed lookups on recorded CI hardware. Report quota/network/probe/startup time separately; no performance claim without the result. |
| AE-14 | Every production adapter has pinned-version auth/model/tool/checkpoint/stop/usage fixtures. Unsupported capability returns its defined error, never a permissive fallback. |
| AE-15 | The office worker's supervisor is its single agent recovery owner; host recovery cannot race that live owner. No new placement starts until old ownership is fenced/reconciled. |
| AE-16 | Fresh setup downloads/verifies one complete release, creates HQ only and registers/starts one service. No satellite option is offered; satellite requests are rejected before provisioning. Interrupted/repeated/concurrent setup reuses durable steps and creates no duplicate service, identity or office. HQ-initiated satellite onboarding is a separate flow. |
| AE-17 | Offline HQ setup performs zero external requests, rejects untrusted/incomplete bundles, and never starts satellite enrollment. Missing service-manager support reports foreground instructions; serve starts installed releases without a download prerequisite. |
| AE-18 | Concurrent registration of the same subscription admits one connection/default profile. Disabled/archived duplicates are rejected; distinct subscriptions are accepted. Two office bindings reuse one connection and its quota/cooldown state. Mismatched or unverified binding identity never becomes ready; credential rotation cannot create fresh quota under another connection ID. |

## 11. Implementation sequence and integration boundaries

1. Add the §12 migration and serializers/projectors with AE-01..03 and the identity uniqueness part of AE-18. Preserve employee identity; do not create a second agents table. Integrate document 04 and schema.sql in that implementation change.
2. Add configuration/discovery handlers and typed local credential bindings, using fake driver fixtures first (AE-05..06, AE-18).
3. Implement HQ permit coordinator and office command/receipt journal (AE-04, AE-07..08).
4. Integrate supervisor/router/session-runner modules inside agentic-org-office, host startup/launch admission and the existing recovery adapter (AE-10..11, AE-15). Apply the executable names in §2 with existing signed-launch/credential tests; retain agent-os as the small separate CLI.
5. Implement native adapters and, when its backend ships, the HTTP worker/agent loop behind the same port; gate each production backend on AE-09/14. Unsupported drivers can remain explicitly unavailable while supported backends ship.
6. Connect durable usage capture/routing evidence and restoration (AE-12), then report the performance gate (AE-13).

The first release accepts explicit turn commands only. Automatic schedules, long-term memory/consolidation, native delegate orchestration and cross-agent messaging policy are not hidden prerequisites; callers submit authorized input to this execution service. Later features use this service rather than creating another process owner. Provider/client libraries and platform launch extensions remain implementation work, but the ownership, state and failure behavior above are fixed.

## 12. SQL extension and file mapping

Apply this extension after [schema.sql](schema.sql) during implementation. Existing tables are referenced, not recreated. The one added existing-table column is agent_placements.placement_generation; triggers reject generation regression, office moves without an increment, and changing a configured agent into a person. The HQ service still verifies cleanup/fencing; a SQL generation check alone cannot prove an old process stopped. SQLite STRICT types and foreign keys are required. Use schema-versioned migrations and rebuild projections from the named authorities. Common attribution for authored organization records follows [record contracts](08-record-contracts.md); legacy unknown metadata is not fabricated.

```mermaid
erDiagram
    employees ||--o| agent_execution_configs : extends_agent
    model_profiles ||--o{ agent_execution_configs : selected_pool
    model_profiles ||--o{ model_profile_connections : ordered_members
    provider_connections ||--o{ model_profile_connections : supplies
    provider_connections o|--o| model_profiles : generated_default
    provider_connections {
        TEXT organization_id PK, FK "required"
        TEXT id PK "required"
        INTEGER schema_version "required"
        INTEGER revision "required"
        TEXT key "required"
        TEXT name "required"
        TEXT provider_type "required"
        TEXT auth_mode "required"
        TEXT subscription_identity "required for subscription_cli"
        TEXT runtime_driver "required"
        TEXT endpoint_url "optional"
        INTEGER enabled "required"
        INTEGER archived "required"
        INTEGER max_concurrent_turns "required"
        TEXT created_at "optional"
        TEXT created_by_kind "required"
        TEXT created_by_id FK "optional"
        TEXT updated_at "optional"
        TEXT updated_by_kind "required"
        TEXT updated_by_id FK "optional"
    }
    model_profiles {
        TEXT organization_id PK, FK "required"
        TEXT id PK "required"
        INTEGER schema_version "required"
        INTEGER revision "required"
        TEXT key "required"
        TEXT name "required"
        TEXT description "required"
        INTEGER is_auto_generated "required"
        TEXT generated_for_connection_id FK "optional"
        INTEGER archived "required"
        TEXT created_at "optional"
        TEXT created_by_kind "required"
        TEXT created_by_id FK "optional"
        TEXT updated_at "optional"
        TEXT updated_by_kind "required"
        TEXT updated_by_id FK "optional"
    }
    model_profile_connections {
        TEXT organization_id PK, FK "required"
        TEXT model_profile_id PK, FK "required"
        TEXT connection_id PK, FK "required"
        INTEGER priority "required"
    }
    agent_execution_configs {
        TEXT organization_id PK, FK "required"
        TEXT employee_id PK, FK "required"
        INTEGER schema_version "required"
        INTEGER revision "required"
        TEXT model_profile_id FK "required"
        TEXT model_id "required"
        TEXT system_prompt_markdown "required"
        TEXT tools_config_json "required"
        INTEGER enabled "required"
        TEXT created_at "optional"
        TEXT created_by_kind "required"
        TEXT created_by_id FK "optional"
        TEXT updated_at "optional"
        TEXT updated_by_kind "required"
        TEXT updated_by_id FK "optional"
    }
```

```mermaid
erDiagram
    employees ||--o| agent_placements : placed_agent
    offices ||--o{ agent_placements : hosts
    offices ||--o{ office_connection_bindings : local_auth
    provider_connections ||--o{ office_connection_bindings : bound_connection
    office_connection_bindings ||--o| office_connection_state : observed_auth
    office_connection_bindings ||--o{ office_connection_models : discovered_models
    provider_connections ||--o{ connection_quota_state : shared_observation
    provider_connections ||--o{ connection_turn_permits : grants
    agent_execution_configs ||--o{ agent_execution_turns : receives
    agent_execution_configs ||--o{ agent_process_instances : owns
    agent_execution_turns ||--o{ agent_execution_attempts : bounded_attempts
    agent_process_instances o|--o{ agent_execution_attempts : executes
    connection_turn_permits ||--o{ agent_execution_attempts : authorizes
    offices ||--o{ supervisor_command_receipts : deduplicates
    agent_placements {
        TEXT organization_id PK, FK "required"
        TEXT employee_id PK, FK "required"
        TEXT office_id FK "required"
        INTEGER placement_generation "required"
    }
    office_connection_bindings {
        TEXT organization_id PK, FK "required"
        TEXT office_id PK, FK "required"
        TEXT connection_id PK, FK "required"
        INTEGER revision "required"
        TEXT auth_context_ref "required"
        TEXT runtime_binary_path "optional"
        TEXT account_identity_ref "optional"
        TEXT updated_at "required"
    }
    office_connection_state {
        TEXT organization_id PK, FK "required"
        TEXT office_id PK, FK "required"
        TEXT connection_id PK, FK "required"
        TEXT status "required"
        INTEGER binding_revision "required"
        TEXT driver_version "optional"
        TEXT observed_at "optional"
        TEXT valid_until "optional"
        TEXT issue_code "optional"
    }
    office_connection_models {
        TEXT organization_id PK, FK "required"
        TEXT office_id PK, FK "required"
        TEXT connection_id PK, FK "required"
        TEXT model_id PK "required"
        TEXT driver_model_id "required"
        TEXT entitlement "required"
        TEXT capabilities_json "required"
        TEXT observed_at "required"
        TEXT valid_until "required"
    }
    connection_quota_state {
        TEXT organization_id PK, FK "required"
        TEXT connection_id PK, FK "required"
        TEXT scope_key PK "required"
        TEXT model_id "optional"
        TEXT status "required"
        TEXT cooldown_until "optional"
        TEXT observed_at "required"
        TEXT observed_by_office_id FK "required"
        INTEGER coordinator_generation "required"
    }
    connection_turn_permits {
        TEXT organization_id PK, FK "required"
        TEXT id PK "required"
        TEXT connection_id FK "required"
        TEXT office_id FK "required"
        TEXT employee_id FK "required"
        INTEGER placement_generation "required"
        TEXT model_id "required"
        TEXT state "required"
        TEXT issued_at "required"
        TEXT expires_at "required"
        INTEGER coordinator_generation "required"
    }
    agent_execution_turns {
        TEXT organization_id PK, FK "required"
        TEXT id PK "required"
        TEXT employee_id FK "required"
        TEXT office_id FK "required"
        INTEGER placement_generation "required"
        INTEGER revision "required"
        TEXT task_id FK "optional"
        TEXT task_kind FK "required"
        TEXT workspace_key "required"
        TEXT input_ref "required"
        TEXT input_sha256 "required"
        INTEGER agent_config_revision "required"
        TEXT model_profile_id FK "required"
        INTEGER profile_revision "required"
        TEXT model_id "required"
        TEXT pool_snapshot_json "required"
        TEXT status "required"
        TEXT created_at "required"
        TEXT ended_at "optional"
        TEXT error_code "optional"
    }
    agent_process_instances {
        TEXT organization_id PK, FK "required"
        TEXT id PK "required"
        TEXT employee_id FK "required"
        TEXT office_id FK "required"
        INTEGER placement_generation "required"
        TEXT connection_id FK "required"
        TEXT runtime_driver "required"
        TEXT driver_version "required"
        TEXT model_id "required"
        TEXT resolved_model_id "required"
        INTEGER connection_revision "required"
        TEXT workspace_key "required"
        TEXT launch_digest "required"
        INTEGER binding_revision "required"
        TEXT session_mark "required"
        INTEGER pid "optional"
        INTEGER pgid "optional"
        TEXT process_start_identity "optional"
        TEXT boot_id "optional"
        TEXT state "required"
        TEXT updated_at "required"
    }
    agent_execution_attempts {
        TEXT organization_id PK, FK "required"
        TEXT id PK "required"
        TEXT turn_id FK "required"
        INTEGER attempt_no "required"
        TEXT employee_id FK "required"
        TEXT office_id FK "required"
        INTEGER placement_generation FK "required"
        TEXT connection_id FK "required"
        TEXT permit_id FK "required"
        TEXT process_instance_id FK "optional"
        TEXT runtime_driver "required"
        TEXT driver_version "required"
        TEXT requested_model_id FK "required"
        TEXT resolved_model_id "required"
        INTEGER connection_revision "required"
        INTEGER binding_revision "required"
        TEXT state "required"
        TEXT effect_state "required"
        TEXT checkpoint_ref "optional"
        TEXT predecessor_attempt_id FK "optional"
        TEXT started_at "optional"
        TEXT ended_at "optional"
        TEXT error_code "optional"
    }
    supervisor_command_receipts {
        TEXT organization_id PK, FK "required"
        TEXT office_id PK, FK "required"
        TEXT operation_id PK "required"
        TEXT command "required"
        TEXT employee_id FK "optional"
        TEXT payload_sha256 "required"
        TEXT outcome "required"
        TEXT response_json "required"
        TEXT recorded_at "required"
    }
```

```sql
ALTER TABLE agent_placements ADD COLUMN placement_generation INTEGER NOT NULL DEFAULT 1 CHECK (placement_generation>=1);

CREATE TABLE provider_connections (
organization_id TEXT NOT NULL REFERENCES organizations(id),
  id TEXT NOT NULL,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
key TEXT NOT NULL CHECK (length(key)>0),
  name TEXT NOT NULL,
  provider_type TEXT NOT NULL CHECK (length(provider_type)>0),
  auth_mode TEXT NOT NULL CHECK (auth_mode IN ('subscription_cli','api_key','cloud_iam')),
  subscription_identity TEXT CHECK (subscription_identity IS NULL OR length(trim(subscription_identity))>0),
  runtime_driver TEXT NOT NULL,
  endpoint_url TEXT,
  enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
  archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0,1)),
  max_concurrent_turns INTEGER NOT NULL DEFAULT 1 CHECK (max_concurrent_turns>=1),
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee','owner','system','unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee','owner','system','unknown')),
  updated_by_id TEXT,
  PRIMARY KEY (organization_id,id),
UNIQUE (organization_id,key),
  UNIQUE (organization_id,provider_type,subscription_identity),
  CHECK (auth_mode<>'subscription_cli' OR subscription_identity IS NOT NULL),
  CHECK ((created_by_kind = 'employee' AND created_by_id IS NOT NULL) OR (created_by_kind <> 'employee' AND created_by_id IS NULL)),
  CHECK ((updated_by_kind = 'employee' AND updated_by_id IS NOT NULL) OR (updated_by_kind <> 'employee' AND updated_by_id IS NULL)),
  FOREIGN KEY (organization_id,created_by_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,updated_by_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TRIGGER registered_subscription_cannot_be_replaced
BEFORE INSERT ON provider_connections
WHEN EXISTS (
  SELECT 1 FROM provider_connections p
  WHERE p.organization_id=NEW.organization_id AND p.subscription_identity IS NOT NULL
    AND (p.id=NEW.id OR p.key=NEW.key OR
      (p.provider_type=NEW.provider_type AND p.subscription_identity=NEW.subscription_identity)))
BEGIN SELECT RAISE(ABORT,'registered subscription cannot be replaced; resolve existing connection'); END;

CREATE TRIGGER registered_subscription_cannot_be_displaced
BEFORE UPDATE ON provider_connections
WHEN EXISTS (
  SELECT 1 FROM provider_connections p
  WHERE p.organization_id=NEW.organization_id AND p.subscription_identity IS NOT NULL
    AND NOT (p.organization_id=OLD.organization_id AND p.id=OLD.id)
    AND (p.id=NEW.id OR p.key=NEW.key OR
      (p.provider_type=NEW.provider_type AND p.subscription_identity=NEW.subscription_identity)))
BEGIN SELECT RAISE(ABORT,'registered subscription cannot be displaced by another connection'); END;

CREATE TRIGGER registered_subscription_identity_is_immutable
BEFORE UPDATE OF provider_type,subscription_identity ON provider_connections
WHEN OLD.subscription_identity IS NOT NULL AND
  (NEW.provider_type IS NOT OLD.provider_type OR NEW.subscription_identity IS NOT OLD.subscription_identity)
BEGIN SELECT RAISE(ABORT,'registered subscription identity is immutable'); END;

CREATE TABLE model_profiles (
organization_id TEXT NOT NULL REFERENCES organizations(id),
  id TEXT NOT NULL,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
key TEXT NOT NULL CHECK (length(key)>0),
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  is_auto_generated INTEGER NOT NULL DEFAULT 0 CHECK (is_auto_generated IN (0,1)),
  generated_for_connection_id TEXT,
  archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0,1)),
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee','owner','system','unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee','owner','system','unknown')),
  updated_by_id TEXT,
  PRIMARY KEY (organization_id,id),
UNIQUE (organization_id,key),
  UNIQUE (organization_id,generated_for_connection_id),
  CHECK ((is_auto_generated=1 AND generated_for_connection_id IS NOT NULL) OR (is_auto_generated=0 AND generated_for_connection_id IS NULL)),
  FOREIGN KEY (organization_id,generated_for_connection_id) REFERENCES provider_connections(organization_id,id) ON DELETE RESTRICT,
  CHECK ((created_by_kind = 'employee' AND created_by_id IS NOT NULL) OR (created_by_kind <> 'employee' AND created_by_id IS NULL)),
  CHECK ((updated_by_kind = 'employee' AND updated_by_id IS NOT NULL) OR (updated_by_kind <> 'employee' AND updated_by_id IS NULL)),
  FOREIGN KEY (organization_id,created_by_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,updated_by_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE model_profile_connections (
organization_id TEXT NOT NULL,
  model_profile_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  priority INTEGER NOT NULL CHECK (priority BETWEEN 1 AND 64),
  PRIMARY KEY (organization_id,model_profile_id,connection_id),
  UNIQUE (organization_id,model_profile_id,priority),
  FOREIGN KEY (organization_id,model_profile_id) REFERENCES model_profiles(organization_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,connection_id) REFERENCES provider_connections(organization_id,id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE agent_execution_configs (
organization_id TEXT NOT NULL REFERENCES organizations(id),
  employee_id TEXT NOT NULL,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
model_profile_id TEXT NOT NULL,
  model_id TEXT NOT NULL CHECK (length(model_id)>0),
  system_prompt_markdown TEXT NOT NULL CHECK (length(system_prompt_markdown)>0),
  tools_config_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(tools_config_json) AND json_type(tools_config_json)='array'),
  enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee','owner','system','unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee','owner','system','unknown')),
  updated_by_id TEXT,
  PRIMARY KEY (organization_id,employee_id),
FOREIGN KEY (organization_id,employee_id) REFERENCES employees(organization_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,model_profile_id) REFERENCES model_profiles(organization_id,id) ON DELETE RESTRICT,
  CHECK ((created_by_kind = 'employee' AND created_by_id IS NOT NULL) OR (created_by_kind <> 'employee' AND created_by_id IS NULL)),
  CHECK ((updated_by_kind = 'employee' AND updated_by_id IS NOT NULL) OR (updated_by_kind <> 'employee' AND updated_by_id IS NULL)),
  FOREIGN KEY (organization_id,created_by_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,updated_by_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE office_connection_bindings (
organization_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  revision INTEGER NOT NULL CHECK (revision>=1),
  auth_context_ref TEXT NOT NULL CHECK (length(auth_context_ref)>0),
  runtime_binary_path TEXT,
  account_identity_ref TEXT,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (organization_id,office_id,connection_id),
  FOREIGN KEY (organization_id,office_id) REFERENCES offices(organization_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,connection_id) REFERENCES provider_connections(organization_id,id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE office_connection_state (
organization_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('unknown','ready','auth_expired','unavailable')),
  binding_revision INTEGER NOT NULL CHECK (binding_revision>=1),
  driver_version TEXT,
  observed_at TEXT,
  valid_until TEXT,
  issue_code TEXT,
  PRIMARY KEY (organization_id,office_id,connection_id),
  CHECK (status<>'ready' OR (driver_version IS NOT NULL AND observed_at IS NOT NULL AND valid_until IS NOT NULL)),
  FOREIGN KEY (organization_id,office_id,connection_id) REFERENCES office_connection_bindings(organization_id,office_id,connection_id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE office_connection_models (
organization_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  model_id TEXT NOT NULL,
  driver_model_id TEXT NOT NULL,
  entitlement TEXT NOT NULL CHECK (entitlement IN ('confirmed','denied','unknown')),
  capabilities_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(capabilities_json) AND json_type(capabilities_json)='object'),
  observed_at TEXT NOT NULL,
  valid_until TEXT NOT NULL,
  PRIMARY KEY (organization_id,office_id,connection_id,model_id),
  FOREIGN KEY (organization_id,office_id,connection_id) REFERENCES office_connection_bindings(organization_id,office_id,connection_id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE connection_quota_state (
organization_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  scope_key TEXT NOT NULL,
  model_id TEXT,
  status TEXT NOT NULL CHECK (status IN ('unknown','open','cooldown')),
  cooldown_until TEXT,
  observed_at TEXT NOT NULL,
  observed_by_office_id TEXT NOT NULL,
  coordinator_generation INTEGER NOT NULL CHECK (coordinator_generation>=1),
  PRIMARY KEY (organization_id,connection_id,scope_key),
  CHECK ((model_id IS NULL AND scope_key='connection') OR (model_id IS NOT NULL AND scope_key='model:'||model_id)),
  CHECK (status<>'cooldown' OR cooldown_until IS NOT NULL),
  FOREIGN KEY (organization_id,connection_id) REFERENCES provider_connections(organization_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,observed_by_office_id) REFERENCES offices(organization_id,id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE connection_turn_permits (
organization_id TEXT NOT NULL,
  id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  employee_id TEXT NOT NULL,
  placement_generation INTEGER NOT NULL CHECK (placement_generation>=1),
  model_id TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('active','release_pending','released')),
  issued_at TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  coordinator_generation INTEGER NOT NULL CHECK (coordinator_generation>=1),
  PRIMARY KEY (organization_id,id),
  UNIQUE (organization_id,id,connection_id,employee_id,office_id,placement_generation,model_id),
  FOREIGN KEY (organization_id,connection_id) REFERENCES provider_connections(organization_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,office_id) REFERENCES offices(organization_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,employee_id) REFERENCES employees(organization_id,id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE agent_execution_turns (
organization_id TEXT NOT NULL,
  id TEXT NOT NULL,
  employee_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  placement_generation INTEGER NOT NULL CHECK (placement_generation>=1),
  revision INTEGER NOT NULL CHECK (revision>=1),
  task_id TEXT,
  task_kind TEXT NOT NULL DEFAULT 'task' CHECK (task_kind='task'),
  workspace_key TEXT NOT NULL,
  input_ref TEXT NOT NULL,
  input_sha256 TEXT NOT NULL CHECK (length(input_sha256)=64),
  agent_config_revision INTEGER NOT NULL CHECK (agent_config_revision>=1),
  model_profile_id TEXT NOT NULL,
  profile_revision INTEGER NOT NULL CHECK (profile_revision>=1),
  model_id TEXT NOT NULL,
  pool_snapshot_json TEXT NOT NULL CHECK (json_valid(pool_snapshot_json) AND json_type(pool_snapshot_json)='array'),
  status TEXT NOT NULL CHECK (status IN ('accepted','running','switching','blocked','recovery_required','succeeded','failed','cancelled')),
  created_at TEXT NOT NULL,
  ended_at TEXT,
  error_code TEXT,
  PRIMARY KEY (organization_id,id),
  UNIQUE (organization_id,id,employee_id,office_id,placement_generation),
  FOREIGN KEY (organization_id,employee_id) REFERENCES agent_execution_configs(organization_id,employee_id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,office_id) REFERENCES offices(organization_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,model_profile_id) REFERENCES model_profiles(organization_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,task_kind,task_id) REFERENCES record_refs(organization_id,kind,id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE agent_process_instances (
organization_id TEXT NOT NULL,
  id TEXT NOT NULL,
  employee_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  placement_generation INTEGER NOT NULL CHECK (placement_generation>=1),
  connection_id TEXT NOT NULL,
  runtime_driver TEXT NOT NULL,
  driver_version TEXT NOT NULL,
  model_id TEXT NOT NULL,
  resolved_model_id TEXT NOT NULL,
  connection_revision INTEGER NOT NULL CHECK (connection_revision>=1),
  workspace_key TEXT NOT NULL,
  launch_digest TEXT NOT NULL CHECK (length(launch_digest)=64),
  binding_revision INTEGER NOT NULL CHECK (binding_revision>=1),
  session_mark TEXT NOT NULL,
  pid INTEGER,
  pgid INTEGER,
  process_start_identity TEXT,
  boot_id TEXT,
  state TEXT NOT NULL CHECK (state IN ('planned','starting','idle','busy','stopping','cleanup_required','gone')),
  updated_at TEXT NOT NULL,
  PRIMARY KEY (organization_id,id),
  UNIQUE (organization_id,session_mark),
  UNIQUE (organization_id,id,employee_id,office_id,placement_generation),
  CHECK ((pid IS NULL AND pgid IS NULL AND process_start_identity IS NULL AND boot_id IS NULL)
    OR (pid IS NOT NULL AND pgid IS NOT NULL AND pid>0 AND pgid>0 AND process_start_identity IS NOT NULL AND boot_id IS NOT NULL)),
  CHECK (state NOT IN ('idle','busy') OR (pid IS NOT NULL AND process_start_identity IS NOT NULL AND boot_id IS NOT NULL)),
  FOREIGN KEY (organization_id,employee_id) REFERENCES agent_execution_configs(organization_id,employee_id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,office_id) REFERENCES offices(organization_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,connection_id) REFERENCES provider_connections(organization_id,id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE agent_execution_attempts (
organization_id TEXT NOT NULL,
  id TEXT NOT NULL,
  turn_id TEXT NOT NULL,
  attempt_no INTEGER NOT NULL CHECK (attempt_no>=1),
  employee_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  placement_generation INTEGER NOT NULL CHECK (placement_generation>=1),
  connection_id TEXT NOT NULL,
  permit_id TEXT NOT NULL,
  process_instance_id TEXT,
  runtime_driver TEXT NOT NULL,
  driver_version TEXT NOT NULL,
  requested_model_id TEXT NOT NULL,
  resolved_model_id TEXT NOT NULL,
  connection_revision INTEGER NOT NULL CHECK (connection_revision>=1),
  binding_revision INTEGER NOT NULL CHECK (binding_revision>=1),
  state TEXT NOT NULL CHECK (state IN ('selected','starting','running','switching','succeeded','failed','cancelled','recovery_required')),
  effect_state TEXT NOT NULL CHECK (effect_state IN ('none','recorded','unknown')),
  checkpoint_ref TEXT,
  predecessor_attempt_id TEXT,
  started_at TEXT,
  ended_at TEXT,
  error_code TEXT,
  PRIMARY KEY (organization_id,id),
  UNIQUE (organization_id,turn_id,attempt_no),
  UNIQUE (organization_id,id,turn_id),
  CHECK (predecessor_attempt_id IS NULL OR predecessor_attempt_id<>id),
  FOREIGN KEY (organization_id,turn_id,employee_id,office_id,placement_generation) REFERENCES agent_execution_turns(organization_id,id,employee_id,office_id,placement_generation) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,process_instance_id,employee_id,office_id,placement_generation) REFERENCES agent_process_instances(organization_id,id,employee_id,office_id,placement_generation) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,connection_id) REFERENCES provider_connections(organization_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,permit_id,connection_id,employee_id,office_id,placement_generation,requested_model_id) REFERENCES connection_turn_permits(organization_id,id,connection_id,employee_id,office_id,placement_generation,model_id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,predecessor_attempt_id,turn_id) REFERENCES agent_execution_attempts(organization_id,id,turn_id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE supervisor_command_receipts (
organization_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  operation_id TEXT NOT NULL,
  command TEXT NOT NULL CHECK (command IN ('RunTurn','ResumeTurn','CancelTurn','StopAgent','BindConnection','RefreshConnection')),
  employee_id TEXT,
  payload_sha256 TEXT NOT NULL CHECK (length(payload_sha256)=64),
  outcome TEXT NOT NULL CHECK (outcome IN ('accepted','rejected')),
  response_json TEXT NOT NULL CHECK (json_valid(response_json) AND json_type(response_json)='object'),
  recorded_at TEXT NOT NULL,
  PRIMARY KEY (organization_id,office_id,operation_id),
  FOREIGN KEY (organization_id,office_id) REFERENCES offices(organization_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,employee_id) REFERENCES employees(organization_id,id) ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER placement_generation_must_advance BEFORE UPDATE ON agent_placements
WHEN NEW.placement_generation<OLD.placement_generation OR (NEW.office_id<>OLD.office_id AND NEW.placement_generation<=OLD.placement_generation)
BEGIN SELECT RAISE(ABORT,'office changes require a newer placement generation'); END;
CREATE INDEX agent_configs_by_profile ON agent_execution_configs(organization_id,model_profile_id);
CREATE INDEX profiles_by_connection ON model_profile_connections(organization_id,connection_id);
CREATE INDEX attempts_by_turn ON agent_execution_attempts(organization_id,turn_id,state);
CREATE INDEX permits_by_connection ON connection_turn_permits(organization_id,connection_id,state);
CREATE UNIQUE INDEX one_open_turn_per_agent ON agent_execution_turns(organization_id,employee_id)
WHERE status IN ('accepted','running','switching','blocked','recovery_required');
CREATE UNIQUE INDEX one_owned_main_process ON agent_process_instances(organization_id,employee_id)
WHERE state<>'gone';
CREATE TRIGGER agent_config_requires_agent BEFORE INSERT ON agent_execution_configs
BEGIN SELECT RAISE(ABORT,'execution config requires an agent employee') WHERE EXISTS (
 SELECT 1 FROM employees e WHERE e.organization_id=NEW.organization_id AND e.id=NEW.employee_id AND e.kind<>'agent'); END;
CREATE TRIGGER agent_config_update_requires_agent BEFORE UPDATE OF employee_id,organization_id ON agent_execution_configs
BEGIN SELECT RAISE(ABORT,'execution config requires an agent employee') WHERE EXISTS (
 SELECT 1 FROM employees e WHERE e.organization_id=NEW.organization_id AND e.id=NEW.employee_id AND e.kind<>'agent'); END;
CREATE TRIGGER configured_employee_stays_agent BEFORE UPDATE OF kind ON employees
WHEN NEW.kind<>'agent' AND EXISTS (SELECT 1 FROM agent_execution_configs c WHERE c.organization_id=OLD.organization_id AND c.employee_id=OLD.id)
BEGIN SELECT RAISE(ABORT,'configured employee must remain an agent'); END;

CREATE VIEW model_profile_model_union AS
SELECT m.organization_id,m.office_id,p.model_profile_id,m.model_id,
 COUNT(*) AS supporting_connections,MIN(p.priority) AS first_priority
FROM model_profile_connections p JOIN office_connection_models m
 ON m.organization_id=p.organization_id AND m.connection_id=p.connection_id
WHERE m.entitlement='confirmed'
GROUP BY m.organization_id,m.office_id,p.model_profile_id,m.model_id;
```

## Appendix A. Verified v1 starting point

Inspected `origin/v1` at **`ee84e7ba231fbef79373f641c3d24fd26a248be5`**. These are repository findings, not claims about processes running on a user's machine.

| Component | Verified role in that checkout | Evidence |
| --- | --- | --- |
| `org-office-host` | Starts and supervises an **office server** process, `org-server`. It is not an implemented subscription-pool agent supervisor. | [Spawner::spawn](https://github.com/vgmello/agentic-org/blob/ee84e7ba231fbef79373f641c3d24fd26a248be5/apps/org-office-host/src/supervisor/child.rs#L175) calls the host's suspended-spawn path with the server executable. |
| `org-supervise` | Implemented **library** for explicit environments, child spawning, ownership, stopping, cleanup, probes and recovery primitives. It is not a standalone supervisor daemon or a routing policy engine. | [Library boundary](https://github.com/vgmello/agentic-org/blob/ee84e7ba231fbef79373f641c3d24fd26a248be5/crates/org-supervise/src/lib.rs#L1), [spawn ownership API](https://github.com/vgmello/agentic-org/blob/ee84e7ba231fbef79373f641c3d24fd26a248be5/crates/org-supervise/src/spawn.rs#L43). |
| `org-runtime` | Runtime-adapter contract. `AgentRuntime::plan_session` produces a launch plan and protocol session; the adapter itself does not spawn or do I/O. | [AgentRuntime](https://github.com/vgmello/agentic-org/blob/ee84e7ba231fbef79373f641c3d24fd26a248be5/crates/org-runtime/src/runtime.rs#L10). |
| `org-supervise::runtime` | Maps launch plans into process preparation; its own source explicitly says it is not a complete session runner. | [Runtime mapping](https://github.com/vgmello/agentic-org/blob/ee84e7ba231fbef79373f641c3d24fd26a248be5/crates/org-supervise/src/runtime.rs#L1). |
| `org-server` | Contains a recovery-record adapter over office-owned process rows. This is a real integration component, not proof that the whole agent launch loop is wired. | [Owned recovery adapter](https://github.com/vgmello/agentic-org/blob/ee84e7ba231fbef79373f641c3d24fd26a248be5/apps/org-server/src/owned_recovery.rs#L1). |
| Employee supervisor/session pump | **Proposed design:** the office hosts a logical employee supervisor; `org-office-sessions` is intended to implement the session pump. No `org-office-sessions`, native runtime-adapter packages or `org-agent-supervisor` executable appear in the checked Cargo workspace. A complete native-agent launch integration was not found there. | [Employee-supervision proposal](https://github.com/vgmello/agentic-org/blob/ee84e7ba231fbef79373f641c3d24fd26a248be5/docs/design/employee-supervision.md#L1), [planned session integration](https://github.com/vgmello/agentic-org/blob/ee84e7ba231fbef79373f641c3d24fd26a248be5/docs/design/agent-sessions.md#L125), [workspace](https://github.com/vgmello/agentic-org/blob/ee84e7ba231fbef79373f641c3d24fd26a248be5/Cargo.toml). |

Therefore, v1 supplies useful low-level process machinery and a proposed employee-control design. It does not establish that a finished `org-agent-supervisor` process already launches agents. The current revision retains the office-hosted control loop and defines its supervisor/router/runner responsibilities without a separate supervisor daemon. The executable names in §2 are target renames; evidence links here intentionally retain inspected v1 names.

V1's environment preparation is intentionally restricted. Its reviewed runtime-switch list does not currently include the connection-specific auth-context variables shown in the upload. See [reviewed switches](https://github.com/vgmello/agentic-org/blob/ee84e7ba231fbef79373f641c3d24fd26a248be5/crates/org-supervise/src/runtime.rs#L137) and [explicit environment construction](https://github.com/vgmello/agentic-org/blob/ee84e7ba231fbef79373f641c3d24fd26a248be5/crates/org-supervise/src/environment.rs#L52). Auth isolation needs an adapter/environment-contract extension; arbitrary environment maps must not bypass that boundary.
