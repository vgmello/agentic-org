# A2 increment plan (planner, reviewed by the lead)

A2 increment plan (read-only; base origin/v1-integration 6d9b737a)

All file paths below are relative to `/Users/vgmello/projects/agentic-org-claude`.

**Main finding.** The current setup design cannot run as A2 describes. The setup continuation and the reverse bootstrap both carry what A2 forbids:
- `crates/org-host/src/instance/setup.rs:18-19` puts `login` and `organization: PlanV1` in setup's input. `setup.rs:75-78` records `hq_operation`, `provisioned` and `exited`.
- `crates/org-host/src/bootstrap.rs:247-363` takes a login and password and calls `HostDb::create` with a `FirstAccount`. Its `finish_bootstrap` waits for HQ provisioning (`hostdb.rs:134-153`).
- `org-host-control/src/ops.rs:854-861` (`BootstrapParams`) carries `login` and `password`.

I recommend replacing the reverse bootstrap rather than adapting it (decision D1).

## 1. What a person can do when finished (macOS)

```
$ agentic-org setup
Setting up instance "default".
Data: /Users/alex/Library/Application Support/agentic.org/instances/default
Address: http://127.0.0.1:7420  (Enter to accept, or type a port)
Disk encryption (FileVault): on.
File permissions: the instance folder and everything in it are readable only by alex (folders 700, files 600).
Agents run as alex (same-user mode): only the tool sandbox separates agents from the server's data.
Service registered (launchd, com.ellosoft.agentic-org.instance.<instance-id>).
Service running.
Instance ready. It has no administrator yet.

Open this link in a browser to create the administrator account.
It works once and expires in 24 hours (at 14:02 tomorrow):
  http://127.0.0.1:7420/register#k7Q2…
If it expires or is lost, run: agentic-org admin reset
```

- When FileVault is off: "Disk encryption is off. Agentic Org does not encrypt its data; turn on FileVault in System Settings › Privacy & Security." Setup warns and continues (ADR-0016:33-35).
- **Second run** prints no link and changes nothing. It ends "It has no administrator yet. For a new link, run: agentic-org admin reset" (spec:298-301).
- **`agentic-org admin reset [--name N]`**: a Touch ID or login-password dialog, then a new registration link. The previous link is cancelled (spec:402-405). Every row of the failure table at spec:431-440 applies to the macOS cases.
- **`agentic-org setup --non-interactive --config f.json`**: JSON status with `"administrator":"none"` and no link (standalone-server.md:270-277).
- **`agentic-org service start|stop|status [--name N]`**: thin wrappers over the same adapter. They are needed because the `admin reset` error copy points at `service start` (spec:439).
- **Linux or another OS:** setup refuses before any state changes: "Setup on Linux arrives in milestone two; nothing was changed."
- **The link page itself is A3.** Opening the link in A2 reaches the instance's address only.

## 2. Building blocks, in commit order (each commit carries its tests)

**B0 · Platform adapters**
- Contracts are owned by the consumer in `crates/org-host/src/platform/` (new):
  - `ServiceManager`: `reconcile(&ServiceSpec) -> Registered`, `start`, `stop`, `status -> ServiceState`. `ServiceSpec` holds the label, the absolute program path, args `["serve","--name",N]`, an explicit environment and log paths.
  - `DiskEncryption`: `state(root) -> On | Off | Unknown(reason)`.
  - `InstanceLocations`: the application base and the default data root.
  - `Presence`: moved from `crates/org-host/src/recovery/presence.rs` + `presence.m` to `platform/presence`. Recovery keeps using it.
- macOS implementations:
  - launchd: write the plist into an injected agents directory (production: `~/Library/LaunchAgents`). Then `launchctl bootstrap gui/<uid>`, `kickstart`, `print`, `bootout`. Settings: `KeepAlive{SuccessfulExit=false}`, `RunAtLoad`.
  - A plist with the same label but different bytes is refused: an unrelated job is never taken over (design:360). Identical and running means nothing is done, so a healthy daemon is not restarted (SRV-6).
  - Disk encryption: `diskutil info -plist <root>` for the volume holding the root, falling back to `fdesetup isactive`. The plist key names are unverified; the opt-in test must confirm them.
- Every other OS gets a refusing adapter behind one constructor without `cfg` (architecture-rules.md:447-466, ADR-0017 LNX-5). The `Library/Application Support` path hard-coded at `setup.rs:122` moves into `InstanceLocations`.
- Test adapters, debug and `test-support` only:
  - `FakeServiceManager`: records calls.
  - `ChildServiceManager`: spawns the real `agentic-org serve --name N` as a child in a temp base and root.
  - `FakeDiskEncryption`.
- Tests: plist bytes, launchctl argv, collision refusal, idempotent reconcile, refusing adapters (checked with `cargo check --target x86_64-unknown-linux-gnu`).

**B1 · Setup continuation v1, rewritten** (`crates/org-host/src/instance/setup.rs` and `setup/tests.rs`)
- `Input`: `{version, root, listen}` only.
- `Record`: `{version, input, offline, release, setup_operation, instance_id, registry_created, service_registered, initial_link_issued}`.
- Delete `login`, `organization`, `hq_operation`, `provisioned` and `exited`.
- New IDs (instance, operation, link) are UUIDv7. This touches the `ulid_time_ms` checks in `host_secrets.rs:151`, `registry.rs` and `setup.rs:88-94`. Existing provisioning ULIDs stay for FC-142.
- Tests: strict JSON at 64 KiB, unknown fields refused, frozen-input conflict, monotonic progress.

**B2 · One-time link store** (new `crates/org-host/src/links.rs`; `links/contract_tests.rs`; `hostdb/links.rs`)
- Contract `OneTimeLinks`:
  - `issue(kind, subject, now) -> (LinkId, Token, expires_at)`. It cancels the pending link of the same kind and subject in the same transaction.
  - `consume(kind, token, now) -> Result<LinkId, Refused::{Unknown|Used|Cancelled|Expired}>`.
  - `pending(kind, subject)`.
- Token: 32 random bytes, base64url.
- Digest: `SHA-256("agentic-org/link/v1" ‖ instance_id ‖ kind ‖ token)`. Only the digest is stored; the secret goes after `#` (design:424-426).
- Table `one_time_links(link_id, kind, subject, digest UNIQUE, issued_at_ms, expires_at_ms, state pending|used|cancelled, ended_at_ms)`, plus a partial unique index on `(kind, subject) WHERE state='pending'`.
- The engine-independent suite runs against SQLite and an in-memory adapter (ADR-0018). This is the first piece of A3's host-storage contract.
- In the same commit, `HostDb::create` takes `{instance_id, clock_high_ms}` with no account. The schema starts at version 1.
- Tests: used, replaced and expired links are refused; 24-hour expiry with an injected clock; the digest is bound to the instance; the store holds no token bytes. The bytes check is a scan of the db file.

**B3 · Host: registered-instance serve and link issuing**
- Files: `crates/org-host/src/lib.rs`, `service/mod.rs`, `org-host-control/src/{ops,status}.rs`, `apps/agentic-org/src/serve.rs`.
- `serve --name N`: the instance comes from the frozen setup record (executable-migration.md:23-25). The host creates the registry at first start under its own instance lock and host lock, so no unlocked window exists.
- New `host.sock` calls. Admission: hello, the registered instance, peer euid equal to the host's, and the existing `AppPolicy` signing check.
  - `admin.link.initial{setup_operation}`: issues only if no link was ever issued for that operation and no administrator exists. A replay returns `already_issued` with no secret. This keeps the existing rule that a replay never issues a second link (design:152-154).
  - `admin.reset{}`: the host runs `Presence::local(peer)` then `confirm()`. It issues a registration link while no administrator exists. The set-password branch returns `unavailable` until A3.
  - Secrets travel in replies only, through `service::secret_response`.
- `InstanceStatusResult` gains `administrator: none|present`.
- Tests: admission refusals (another uid, missing hello); a presence fake that refuses; a replay gives no secret; the status field.

**B4 · CLI composition**
- Files: `apps/agentic-org/src/{lib.rs, setup.rs, admin.rs, service.rs}`; setup logic in `crates/org-host/src/setup_flow.rs` (new).
- The setup flow, in order:
  1. Platform gate.
  2. `Paths`, then lock and freeze the record.
  3. Reserve the instance and prepare the layout.
  4. Report ENC-3. Modes are read back with `stat`, never assumed.
  5. Reconcile the service and start it.
  6. Probe readiness with hello and `instance.status` within a deadline.
  7. Call `admin.link.initial`, then print the link.
- The wizard prompts with defaults: 7420 for `default`; for a named instance, the first free port from 7421. A port conflict is an actionable error (SRV-4). Isolation mode is recorded as `same-user`.
- Satellite flags are refused as today (`lib.rs:29-37`).
- Real-process tests use the fixture binary in `apps/agentic-org/testing`, composed with `ChildServiceManager` and `FakeDiskEncryption` in temp dirs. They also run the real binary for argument refusals.

**B5 · Docs and checklist.** Section 5 lists them.

## 3. Acceptance criteria and how each is tested

| Criterion | How it is tested |
| --- | --- |
| SRV-2 | Real process: setup twice gives the same ID, root and port, one child serve, and no link on the second run. |
| SRV-3 | Real process: `default` and `lab` have disjoint roots, sockets, labels, ports and secrets namespaces. |
| SRV-4 | Two concurrent setups hit `setup.lock`, one is refused. An aliased root is refused (existing `registry` tests). An occupied port gives an actionable error and the saved address is unchanged. |
| SRV-5 | Interactive and non-interactive setup give an equal frozen record. Non-interactive prints no link and reports `administrator: none`. A grep of log, status JSON and argv finds no token. Satellite input is refused before provisioning. |
| SRV-6 | Running setup again leaves the plist bytes untouched, `kickstart` is not called on a healthy service, and the child PID is unchanged. A setup killed at each step resumes without duplicates (checkpoint hooks). |
| SRV-7 | The registry has 0 accounts and 0 organizations, and no office directory exists. Admin calls from another uid, or with no presence, are refused. |
| SRV-33 | Fake manager: one reconcile and one start; no install prerequisite. |
| SRV-20 | `git grep` finds no `FirstAccount`, `BootstrapParams` or `finish_bootstrap`. |
| SRV-32 | Setup dispatches inside `apps/agentic-org`. |
| SRV-15 | Partial: `service start`, `stop` and `status` only. |
| ENC-3 | Unit test of report rendering for On, Off and Unknown, with modes from `stat`. A real-process test asserts that the report lines appear and match the actual tree modes. |
| "A used or replaced link is refused" (Done when) | Contract suite, plus a real process: setup, `admin reset`, `admin reset`, stop the service, open the registry through `test-support`, then `consume`. Tokens 1 and 2 are refused; token 3 is accepted once. |

**Service registration is tested safely.**
- Default tests never call launchctl. They use only the fake and child-process managers.
- One opt-in run touches launchd: `AGENTIC_ORG_REAL_LAUNCHD=1 cargo test -p org-host --features test-support real_launchd -- --ignored --test-threads=1`.
  - It uses the label prefix `com.ellosoft.agentic-org.test.<uuid>`, a temp agents directory and a temp root.
  - A drop guard calls `bootout` and removes the plist.
- A second opt-in test checks `diskutil` and `fdesetup` parsing against the real machine.
- Presence uses the existing no-prompt fixture pattern (`crates/org-host/tests/recovery_presence.rs`).
- No test opens a real organization root.

**Deferred, and named in FEATURES:**
- SRV-1 and SRV-34 (bundle and offline release staging, ADR-0009). A2 records a development release pin: the absolute paths and digests of the sibling executables.
- SRV-14 (agent-os and harness checks).
- Offering `dedicated-user` (L6).
- The link page (A3).

## 4. What to delete

- `crates/org-host/src/hostdb/first_account.rs`; `FirstAccount` and `HostDbInit.first_account`; `BootstrapRecord`, its `meta` key and `finish_bootstrap` (`hostdb.rs:69-75, 124-153`); the first-account insert and test hooks (`hostdb.rs:218-220, 275-286`).
- The legacy schema migration v1 and v2 (`hostdb.rs:336-347`, `require_legacy_schema`). It breaks "no migration".
- All of `crates/org-host/src/bootstrap.rs` and `bootstrap/{tests,recovery_tests,release_gate_tests}.rs`. Before deleting, port any release-gate or recovery-custody assertions that still apply into the setup-flow tests.
- In `org-host-control/src/ops.rs`: `Bootstrap`, `BootstrapParams`, `BootstrapOutcome`, `BootstrapRefusal`, `BootstrapResult`, `Exit`, and the bootstrap `TlsPair`. Also `journal::bootstrap_fingerprint` and `SetupConnection` in `service/listener.rs`.
- In `setup.rs`: the `login`, `organization`, `hq_operation`, `provisioned` and `exited` fields, and their checks.
- The refusal text and HELP line in `apps/agentic-org/src/lib.rs:4, 27-39`.

## 5. Decisions where the documents are silent

- **D1 · No reverse bootstrap.** It existed to carry the first password and TLS key into the host. A2 removes both. The service host creates its own registry at first start under its locks. Setup gets the initial link over `host.sock` (`admin.link.initial`), bound to the setup operation ID, at most once. Rejected alternative: keep the reverse connection with a spawned bootstrap host. It then has to hand its locks to a launchd-started host, which is the "unlocked restart" the design forbids. Update `standalone-server.md:146-156` and `:279-288`, and add a short ADR at the next free number.
- **D2 · Who owns `admin reset`.** A2 owns the command, presence, and the registration branch for the no-administrator case (plan:112-113). A3 adds the set-new-password branch (plan:145-147). Update both increment texts in `implementation-plan.md`.
- **D3 · Presence also applies to the registration link.** The same gate and dialog apply, with reason "create a new administrator registration link". Setup's initial link needs no presence. Update spec §4a.2 (spec:371-418).
- **D4 · Link details.** Token format, digest binding and table as in B2. The host builds the URL from its own configured origin. Update design §6.
- **D5 · ENC-3 copy and disk-encryption method.** Section 1 copy; Off and Unknown warn, never refuse. Update spec §2 (spec:76-77) and the §4a.2 transcript.
- **D6 · Isolation mode.** A2 records `same-user` and states the reduced protection. The `dedicated-user` offer comes with L6. Update spec §2 step 4.
- **D7 · Linux.** Setup refuses before any change, and all Linux adapters refuse until X1. The SRV-34 line "missing service manager reports foreground instructions" waits for X1. Note this in `implementation-plan.md` X1.
- **D8 · IDs.** New instance, setup operation and link IDs are UUIDv7 (non-negotiable 4). Update FC-142 evidence.
- **D9 · Ports and labels.** Port 7420 for `default`, next free from 7421 for named instances. Label `com.ellosoft.agentic-org.instance.<instance-id>`. Update design §3 table (design:170).

Also update `docs/FEATURES.md` FC-011 and FC-012, `docs/design/executable-migration.md` (entry points), the setup sentence in `docs/START-HERE.md:40`, and `standalone-server.md:625-627`.

## 6. Interactions and proposed commit order

**A1.** Both increments rewrite `hostdb.rs`: `create`, `open`, `connect` and the registry key. Proposal:
1. A1's first commit removes SQLCipher from hostdb only: no `registry` secret, `sqlcipher.rs` deleted. It merges into v1-integration first.
2. A2 does B0 and B1 meanwhile, rebases, then does B2 on plain SQLite.
3. A1 must not touch `first_account` or bootstrap; A2 deletes them. A2 deletes the legacy migration, and A1 should leave it alone.
4. The office secrets key over the rendezvous stays A1's. A2 creates no office.

**F4.** It is not merged: `worktree-f4-sessions` (10613a83) is not an ancestor of 6d9b737a. It changes `lib.rs` `start_prepared`, the supervisor and the `serve` composition. Proposal:
1. F4-CONFORM merges first (order 0 in `.agents/agent-kit/tasks/v1-milestone-1.md`).
2. A2's B3 rebases onto it and adds only the instance-selected `serve --name` entry and the `admin.*` calls.
3. Readiness in A2 is the host plus `instance.status`. It waits for the web process only if F4 reports web status.
4. The debug `serve --home` legacy entry (`serve.rs:88-118`) stays for existing integration tests. Removing it is a follow-up for F4 or A3; record it.

**Order:** F4-CONFORM → A1-hostdb → A2 B0 → B1 → B2 → B3 → B4 → B5 → the rest of A1 → A3.

## 7. Risks and verification

**Risks:**
- The launchd plist key names and `diskutil` keys are unverified on this machine; only the opt-in run proves them.
- The `KeepAlive` and `bootout` semantics must make sure an explicit stop does not restart the service (design:365-366).
- The existing presence bridge needs a foreground console session. Real-process tests must inject a fake, or they block.
- Deleting bootstrap removes heavily tested custody-ordering code. Port the recovery-custody precondition at `hostdb.rs:223-231`.
- Real-process tests spawn `serve`. Socket paths must stay under 103 bytes (design:224), so use short temp bases.
- Lock behaviour has no CI on Linux; only `cargo check` for the Linux target covers it.
- The design calls for a security review of the token and digest before release (design:446-447).

**Commands:**
- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo test -p agentic-org --features test-support --test setup`
- `cargo check --workspace --target x86_64-unknown-linux-gnu`
- `.agents/agent-kit/runtime/kit_hooks.py check`
- Opt-in: `AGENTIC_ORG_REAL_LAUNCHD=1 cargo test -p org-host --features test-support -- --ignored real_launchd real_disk_encryption --test-threads=1`
- Manual on a dev Mac:
  1. `cargo build`, then `target/debug/agentic-org setup --name a2-check --root <tmp>`.
  2. `launchctl print gui/$(id -u)/com.ellosoft.agentic-org.instance.<id>`
  3. `agentic-org admin reset --name a2-check`, then `agentic-org service stop --name a2-check`.
  4. Clean up by hand: run `launchctl bootout`, remove the plist, the setup record and the locator entry. Uninstall is not in A2.

### Critical Files for Implementation
- crates/org-host/src/instance/setup.rs
- crates/org-host/src/hostdb.rs (with hostdb/first_account.rs, hostdb/schema.rs)
- crates/org-host/src/bootstrap.rs (to delete) and crates/org-host-control/src/ops.rs
- crates/org-host/src/lib.rs (start_prepared) and apps/agentic-org/src/{lib.rs,serve.rs}
- crates/org-host/src/recovery/presence.rs (moves to platform/presence)