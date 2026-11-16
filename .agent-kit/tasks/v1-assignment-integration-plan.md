# Selected assignment implementation plan

**Plan revision 17, 2026-11-16: delivery boundaries superseded by separate acceptance/merge units.** Preserve current bounded repairs through safe handoff. Existing-HQ first-owner registration/admission remains PAUSED on missing authoritative proof. Architecture readiness is separate from code acceptance.

The owner explicitly replaced “finish the entire assignment bundle first” with
[dependency-driven deliveries](v1-incremental-delivery.md). That record and its
[exact source inventory](v1-delivery-source-map.json) govern extraction, order and
per-unit acceptance into `v1-integration`. This branch is a preserved source
reservoir, not a whole-ancestry merge candidate. Sections below retain implementation
history and the assignment contract; they no longer require accepting every
prerequisite and assignment together before usable onboarding can ship.

Parent stable acceptance: [v1-assignment-integration-contract.md](v1-assignment-integration-contract.md).

Checkout: `/Users/vgmello/projects/agentic-org`, branch `codex/v1-assignment-integration`, base `73a39c58d683b00ad8620d05f7d5ae10fdc17bc9`; preserved implementation checkpoint `57b209a9fe0f5bc9a03320ae1b9f30e7916be7a4` is unaccepted and unmerged.

Authority: [active ADR-0006 / TAI-01 revision 5](../../docs/adr/0006-selected-task-acceptance.md), its [independent READY recheck](../logs/v1-assignment-integration/architecture-review/recheck/result.json), the [parent acceptance contract](v1-assignment-integration-contract.md), and [active ADR-0007 / REC-ESC-1 revision 1](../../docs/adr/0007-hq-creation-escrow.md) for the focused HQ escrow prerequisite. [ADR-0008 / GW-SBX-MAC-1 revision 1](../../docs/adr/0008-gateway-macos-sandbox.md) governs the focused macOS gateway sandbox prerequisite; Linux enforcement remains unresolved, without waiving the full contract.

One preserved fullstack author, Sol/Xhigh, owns the complete feature under the lead. The lead preserves overall architecture, shared interfaces, dependency order and final integration. At most two implementation writers may work concurrently in separate worktrees under explicit nonoverlapping scopes; current assignments and the review-repair split are in execution checkpoints7–10 below. No nested delegation, worker commits/push/merge, live-root access or installed CLI/spool changes.

## 1. Materialize reviewed contracts and interfaces

**Premises P1–P13.** Amend owning documents before dependent examples/code: organization-data 03/05/12 for exact snapshots, codec-2 events/public blobs and migration; 16 for protected receipts, journal ordinals and origin mappings. Reconcile draft SQL/schema coverage separately from executable migrations. Preserve 01/02/04/06/08 relationships, identity, numbering and attribution.

Extend the gateway contract/capability revision and shared vectors. Document these **additive implementation interfaces**, which are not currently shipped:

- Native `task.set_assignment`: authenticated office context plus `{change_id, task_id, command}`; `command` has exactly `expected_revision`, `assigned_team_id`, `assignee_id`.
- HTTP `POST /api/v1/organizations/{org}/tasks/{task}/assignment`: existing `{operation_id,scope,params}` envelope; the same complete pair, with decimal-string `expected_revision`.
- Authorized task list/detail, per-task history and assignment-target reads under that task namespace; list filters distinguish accountable owner, assigned team and employee.
- Existing `GET /api/v1/operations/work/{id}` lookup, retaining its fixed instance/org/office query fields through an explicitly versioned selected-contract adapter.
- Task-scoped content-reference/chunk reads and owner-bound adoption staging/commit calls. Chunks carry at most 256 KiB raw data; encoded messages remain within 1 MiB.

Return compact committed revision/event identity, assignment pair and origin-office scope. Unknown/error/conflict outcomes follow existing gateway outcome conventions.

Add `crates/org-host-control` and affected gateway/setup documentation to implementation preflight scope: its current name-only provisioning DTO cannot carry selected bootstrap inputs. This is scope bookkeeping for the reviewed provisioning extension.

## 2. Implement bounded codec and snapshot foundations

**P5/P8/P13.** Add selected event/history modules in `org-work`, typed wire DTOs/raw-envelope decoding in `org-proto`, and an additive source-preserving parser/editor API in `org-docs`.

Implement codec 2 exactly as ADR r4 specifies: decoded duplicate detection recursively, scalar-order keys, deterministic string escaping, lexical number tokens, explicit envelope defaults, strict signed-positive integer headers and top-level-only checksum omission. Keep typed command fingerprints separate. Retain unknown payloads/extensions and immutable schema-1 bytes; unsupported codecs remain staged/unverified.

Enforce `selected-task-resources/1`, including aggregate blob limits, during parsing/emission. Retain recorded profiles during recovery. Enable only the necessary pinned `serde_json/raw_value` feature; do not enable `arbitrary_precision` or alter legacy canonical hashing.

Snapshot editing preserves original source, character-to-byte offsets, trivia, fences/BOM, extensions and body. Locate decoded root keys; account for quoted spans containing trailing comments. Replace only the six reviewed scalars, then reparse and verify all untouched slices. Stage unsupported representations.

Validate exact public Markdown before embedding or storing it in immutable blobs. Never regenerate extensions from `Value` or copy private/unclassified source into public history.

## 3. Build selected persistence and recoverable acceptance

**P3/P4/P9/P12.** Add cohesive selected-task storage/recovery modules inside `org-office`; keep policy/ports in `org-work`. Add the reviewed `org-office → org-docs` dependency. Apps remain composition roots.

Implement executable, versioned migrations for selected directory/task projections, events, adoption gates, protected receipts, journal cursors and origin mappings. Preserve existing legacy tables/data. Store authoritative extension/event JSON losslessly as text or verified content references.

Introduce a dedicated acceptance job on the existing writer. Preparation freezes complete mutations, attribution, result, account binding, directory generation and allocation mapping before journal publication. Filesystem publication never occurs inside rerunnable callbacks.

Implement:

- Durable immutable anchor and transaction envelopes at the ADR’s `journals/work-acceptance/...` paths.
- Independent journal ordinals and unchanged work-origin allocator/incarnation.
- Exact zero/one/multi-event mappings, original feed/client positions and checked exhaustion.
- Durable blobs before manifests; atomic non-overwriting publication and durable parent updates.
- Idempotent SQL/canonical-file projection, protected result materialization and startup replay.
- Verified receipt lookup before fresh revision/target checks; post-publication failures remain recoverable/unresolved.
- Coherent verified task/event/receipt reads before completion.

Capture participating legacy work mutations through the same boundary while preserving their decisions, identifiers, results and event ordering. Receipt-only/no-op transactions allocate no event positions. Preserve message allocation/watermarks.

## 4. Ship protected bootstrap, sessions and browser handoff

**P6/P9.** Implement the missing production composition across `org-host`, `org-office-launch`, `agentic-org`, office and web apps.

Use existing typed `bootstrap`, `account.verify`, `session.renew/end`, `office.open_channel` and `session.grant` contracts as applicable. Implement bounded verification/session state and revocation; retain channel-bound grants, host key custody and separate human/agent identities.

Add a capability-versioned provisioning operation carrying organization name/slug, remote repository/state branch and explicit bootstrap home-team inputs. Preserve the legacy name-only operation. Bind provisioning to authenticated instance-administrator authority; task assignment remains live organization-owner authority.

Make `agentic-org setup` a resumable HQ-only workflow using a verified complete local release/bundle path. Create protected administrator/HQ state, valid selected bootstrap records and recovery state; then reconcile/start launchd or systemd registration and report verified local readiness. Preserve recovery collection/defer ordering, stable key namespaces and resumable operation IDs. Offline setup performs no external probes.

Wire the supervised web child through authenticated listener/control handoff. Implement the existing session HTTP routes with configured-origin checks, cookie/proof handling, secret-specific bounded ingress, no-store responses and sanitized errors. Browser responses never expose host handles, grant MACs or office keys.

### 4a. Complete the missing HQ escrow prerequisite before setup readiness

Revision 2 resolves the original author's missing-format question through ADR-0007 revision 1, independently ARCHITECTURE READY. ADR-0006 P6/P9 and all other premises remain intact. Unaffected transport/UI work continued while this prerequisite was paused.

1. Reassess actual prospective org-pki/worker Cargo edges and exact existing ring/argon2/zeroize versions, then refresh the capability packet before dispatch. Follow ADR-0007 byte format and caller-owned zeroizing Argon2 working buffer; preserve legacy nonempty secret-wrapper contracts with an explicit new code parser at provisioning.
2. Add the cohesive org-pki adapter and strict bounded code/envelope APIs; prove exact-byte vectors, public-identity/signature continuity and failure-before-KDF for framing errors. Worker owns this adapter; host/domain/wire layers never gain private CA-key access.
3. Add the worker-local-only SQLCipher CA record and copied public escrow record with executable versioned migration, snapshot/export exclusions and atomic initial completion. Freeze generated material outside retry callbacks. Lost reply/restart reuses the exact committed key/package; acknowledged existing HQ starts without a pending code. Never regenerate committed identity on incomplete/missing state.
4. Wire host durable pending code/opaque journal handle, protected create handoff and truthful preparation/readiness states. Reconcile protected delivery/ACK durability with credential deletion after crashes; status/read/browser never redisplay code or key. Collect/defer plus protected encrypted-package export retain last-group/typed-yes and off-HQ retention acknowledgement. Use existing native platform confirmation requirements.
5. Prove production worker create/reopen and before/after-commit/reply-loss, real protected export/delivery/ACK recovery with generated temporary credentials. Combine with actual service/setup/adoption/browser acceptance. Primitive probe and architecture READY are not substitutes for these tests.

No enrollment/X.509 service or promotion procedure is claimed by this scoped prerequisite; REC-1 remains open. Public escrow version/profile becomes persistent compatibility state and cannot be removed by executable rollback.

### 4b. Complete the reviewed macOS gateway sandbox prerequisite

ADR-0008 r1 is independently ARCHITECTURE READY. Original author implements the narrow libc-only org-socket adapter and web/host composition; no new project edge or unsafe code in web. Apply the fixed parameterized deny-default/exact-rendezvous/deny-bind policy from the sole main thread after bounded public asset preload and unintended-FD closure, before secrets/readiness/untrusted network input. Verify kernel thread count, main/never-threaded conditions and parent post-accept sandbox/image state. Any failure refuses handoff without fallback. Preserve exact descriptor checks and authorization/session revocation.

Use actual subprocess deny/allow/thread/path tests, then real supervised handoff/HTTP/static UI acceptance. Root probes are bounded feasibility; SDK deprecation remains a recheck trigger and no broad runtime allowance is preapproved. Linux's required Landlock/seccomp adapter remains unimplemented/unreviewed and cannot be declared supported or omitted from any applicable platform acceptance. Stable full feature contract and ADR0006/0007 remain unchanged.

## 5. Implement trusted directory admission and actual adoption

**P7/P9/P11.** Build selected organization/team/employee/project import and a writer-ordered directory-generation barrier. Preserve required owners/home teams, active status and project-owner relationships; no first-membership or profile-flag inference.

Expose a human CLI adoption operation backed by authenticated staging/commit calls. Stream bounded artifacts into protected staging, validate manifests and preserve original sources. Do not expose arbitrary server filesystem paths through HTTP.

Before activation, verify host/HQ identity, writer fencing and retained origin history/high-water; install the anchor, imported state and adoption gates recoverably. Preserve IDs, number mappings, provenance, complete original WorkEvents and numeric tokens. Snapshot-only baselines remain explicit; known gaps cannot be concealed.

Fence legacy commands, cascades, Undo and replication imports from mutating adopted IDs. Unmapped/unsupported records remain intact in staging. Other task mutations on adopted records either use selected acceptance or return explicit unsupported/version outcomes.

## 6. Connect assignment and authenticated transport

**P1–P7/P9/P12.** Implement the selected application service using `plan_assignment`, trusted facts and the acceptance port.

Enforce complete replacement, exact revisions, active same-org desired targets, cross-home-team assignment and equal-pair event creation. Preserve owner, reporter, scope, number, path and unrelated content.

Decode native/HTTP wrappers and commands from raw input before map conversion; reject duplicate wrapper, scope and command keys. Preserve legacy parsers. Use lossless browser revision strings through full `i64` boundaries and versioned selected-ID validation.

Implement real gateway `ApiBackend` routes and capability-scoped `org-client` transport. Authenticate every read, chunk, mutation and lookup; bind content references to authorized task/adoption scope. Publish capabilities only after bootstrap, recovery, directory and service readiness.

## 7. Connect the static Svelte feature

**P10.** Add a task-assignment feature module using the production HTTP adapter and existing operation-client immutable-intent/reconciliation behavior. Integrate the task sheet, queues, history and sign-in/session flow.

Display accountable owner separately from assigned team/employee. Independent clear actions submit the complete pair. Preserve loading, submitting, unresolved, conflict, rejected/error, unknown-history and completed states. Scope responses to account/instance/org/task; reconcile lost replies using the original operation ID.

Use existing accessible components, keyboard/focus handling and status announcements. Keep Tailwind Plus inside the UI app. Follow global Svelte skills and official MCP; run autofixer on changed components/modules until clean. No production mock fallback or Node backend.

## 8. Verify behavior and failure boundaries

Use temporary roots and generated test credentials exclusively.

- Codec/snapshot tests: cross-language vectors, escaped/nested duplicates, Unicode, extreme tokens, budgets, strict headers, untouched bytes, public-blob round trips and retained unsupported history.
- Assignment/auth tests: all sixteen pair transitions, independent clearing, equal-pair acceptance, full-range revisions, stale conflicts, target deactivation races, cross-org denial, revoked/wrong-channel grants and denied agent capabilities.
- Receipt tests: retries after later changes/deactivation/restart, account/fingerprint conflicts, expiry and original result preservation.
- Journal tests: ADR’s success/rejection/multi-event vectors; every publication/projection crash boundary; uncertain installation; replay without domain reruns; exhaustion, conflicting files, copied roots, missing checkpoints and stale epochs.
- Migration/legacy tests: original IDs/results/events, B-/REQ mappings/collisions, incomplete history, tombstones, unsupported fields, adoption fences and unchanged message watermarks.

Implement the platform flush adapter and record actual temporary-filesystem operation/results on macOS, including required full-flush behavior and durable publication failures. Unsupported guarantees stop acceptance. Process-kill/reopen evidence must not be described as physical power-loss proof.

Run browser journeys against the **built static assets and real supervised Rust host/web/office services**, invoking actual protected setup/adoption rather than seeding production state. Cover sign-in, all assignment states, queues/history, conflicts, response loss/reload lookup, large chunked content, unauthorized access, accessibility and screenshots in Chromium/WebKit. Exercise actual service registration in an isolated platform environment; preserve production privilege checks.

## 9. Complete checks, review and rollback evidence

Run pinned Rust formatting, locked workspace Clippy/tests; UI type/tests/build/lint/format checks; affected journeys and CSP/gateway checks; draft-schema checks and explicit `kit_hooks.py check`.

Record source-bound receipts and failed/unsupported outcomes honestly. Follow the bounded local review/repair cycle, this author’s intent validation and separate fresh final review of the exact commit. The lead owns integration.

Before adoption, disable the capability to roll back. After accepted writes, pause mutations and retain compatible reads, journals, receipts, blobs, adoption gates and watermarks. Use replay/forward repair; never restore an older executable/checkpoint over acknowledged changes.

**Revision 2 adds independently reviewed ADR-0007 for the discovered missing escrow format.** The other provisioning extension, route/DTO decomposition, parser API and storage modules retain the previously reviewed mechanisms. A contradiction of P1–P13, additional dependency direction, authorization expansion or changed persistence boundary returns exact evidence to the lead before dependent work proceeds. No code acceptance or execution-check success is claimed by this plan.
## Lead dispatch record

The lead validated this sequence against active ADR-0006 revision 4 and the source-bound architecture READY result. The plan decomposes reviewed authentication/bootstrap, persistence/adoption and UI obligations without introducing an authorization expansion, new durability boundary or new dependency direction. A separate IMPLEMENTATION_PLAN pass is unnecessary under task-routing; return genuine new risks/premise contradictions to the lead before dependent writes. All acceptance criteria remain in the stable parent contract.

One original fullstack writer, `01a1dfb4-1d26-7100-9ea4-72b53756ef23`, observed Sol Xhigh. Lead pauses product-source writes during implementation; lead-owned capability/decision/plan records may be updated independently, without overlapping the author’s product files. No nested delegation; one local writer across UI/backend/database. Reviews use fresh contexts later. Required initial Sol High review and separate final Sol Xhigh review remain mandatory. Task-wide review budget follows routing: default three repair rounds, maximum five, stop repeated findings. Architecture used one pass and one focused recheck.

The capability catalog now explicitly covers org-host-control. Refresh the full feature packet and acknowledge its current sources before writes; the earlier planning packet is historical. If Cargo/basis/guidance changes later, the lead performs focused reassessment before new assignments or review. Use only temporary roots and synthetic credentials for execution; preserve real installed services and all user organization data. Service-registration evidence must use disposable service identities and release roots with cleanup, never existing installations. Report unavailable platform evidence honestly; no physical power-loss proof is claimed by fault injection.

Revision 2 dispatch record (2026-11-15): lead inspected original owning escrow requirements and author missing-format report, preserved ADR-0006 and stable contract, ran the bounded exact-version primitive probe, and obtained fresh independent ARCHITECTURE READY for ADR-0007 r1. The added section decomposes that reviewed mechanism and retains public/private records, atomic retry and ACK/deletion obligations. Original fullstack author remains owner. Before its next product phase it must read/acknowledge this revision, ADR-0007 and a refreshed current capability packet. No release or feature acceptance is implied.

Revision 3 focused dependency dispatch (2026-11-15): ADR-0006 r5 independently READY permits private web host-control consumption of existing socket/launch infrastructure, alongside existing exact serde/HMAC/zeroize direct dependencies. This affects section4 session/handoff composition only; no new storage/custody/authorization boundary, wire secret exposure or office RPC implementation is permitted. Host typed session inspection can expose current authorized public facts and host deadlines to the protected web control connection; browser receives only its public view. Before new dependent phase, original author reads changed r5/plan3 paragraphs and current packet; actual source-bound checks remain required. ADR-0007 and complete acceptance remain unchanged.

Revision 4 focused sandbox dispatch (2026-11-15): original author reported missing OS policy and proposed its boundary. Lead scoped existing org-socket, verified parameterized special-path enforcement, SDK sole-thread checks, inherited restriction and parent kernel observation in disposable children, then obtained fresh independent ARCHITECTURE READY for ADR-0008 r1. No repeated unchanged architecture pass or extra implementation-plan review is required: section4b decomposes that exact reviewed mechanism. Author must read this revision/ADR8 and current packet before dependent writes; no code, Linux or full-feature acceptance implied.

### Revision5: complete-release verification prerequisite and independent library slice

ADR0009r1 is independently ARCHITECTURE READY and active; it fills the original author's missing release format/key-distribution contract without weakening P1–P13 or the stable full assignment contract. Preserve exact candidate/review/feasibility references in its activation record. This is decomposition of that reviewed boundary, so no separate IMPLEMENTATION_PLAN pass is required absent a material new risk/interface.

1. Lead maintains one complete-feature contract/metrics run and integration owner. Original author continues adoption/native transport/static UI. A second bounded fullstack implementation worker may implement ONLY the independent org-release public verifier in a separate worktree after explicit source/API ownership ACK, at Sol/Xhigh for critical publisher trust risk. No third product writer. No new whole-task metric/overlapping task total.
2. org-release owns strict exact manifest/signature/trusted-key/public inventory verification with pure policy/byte interfaces. It has no archive extraction, service registration, custody, network, paid model calls, platform process admission or application imports. Proposed API names/types and failure semantics are recorded with the bounded worker contract and acknowledged by the original fullstack owner. Workspace member/dependency lines are integration glue only, never replace the main Cargo manifests wholesale.
3. Original fullstack owner integrates that verified public library, then implements host-owned bounded directory/tar.gz staging/durable pins/setup/service/recovery composition and web authenticated preload/actual-image binding under ADR0009. If a useful second independent slice remains, lead may rescope with nonoverlapping ownership; it never delegates shared contracts to simultaneous writers.
4. Test generated-key exact valid/tamper/duplicate/unknown/inventory/path/floor/rotation policy in the library. Actual supported container/staging/host/web failure/restart and complete setup/adoption/browser tests remain parent acceptance; helper completion is not shipped assignment or production signing proof. Preserve original author's same-context intent validation and fresh initial/final full-feature review.
5. Reassess/validate catalog for org-release's critical public trust boundary, refresh actual packets/read ACKs in each checkout and confirm exact model/effort before implementation. No runtime trust override in release builds, no real signing key generation/access, no dependency upgrade beyond reviewed exact additions.

### Revision6: owner-approved coordinated parallel execution

The owner explicitly approved evaluating and using parallel work while one lead preserves the overall architecture and direction. The full stable acceptance contract and ADR0006–0009 remain unchanged. This revision changes ownership and sequencing only; no new security/persistence mechanism, public product interface or waiver is introduced, so a repeated architecture/implementation-plan review is unnecessary. Material new risks still return to the lead before affected work. Earlier single-writer dispatch paragraphs above are historical and superseded by this current bounded assignment.

| Lane | Owner and scope | Current status / handoff |
| --- | --- | --- |
| Architecture and complete delivery | Root lead: authoritative decisions, agreed interfaces, capability/routing, cross-layer consistency, verification/review and v1-integration. Original fullstack author retains whole-feature intent and integration validation. | Active throughout. The lead does not become a third product writer. |
| Host and runtime implementation | Preserved original author: Rust setup/release/adoption/directory/transport, actual protected disposable service/fixture driver, production UI fixes unless explicitly transferred. | Active; owner-authenticated native adoption/assignment/exact/stale replay now passes actual worker fixture. Consume imported release library next. |
| Shared release verifier | Isolated library author: only org-release plus test-only local Cargo glue. | Complete/frozen. Six declared source files and precise member/lock glue imported; main focused fmt,14tests and Clippy pass. Retain author context for later validation/repairs. This is no full-feature acceptance. |
| Real browser verification | Next isolated fullstack worker: only UI e2e-selected/** and playwright.selected.config.ts; ignored verification evidence. | Prepare after original author's explicit ACK freezes those prefixes and their private SELECTED_TASK_FIXTURE descriptor contract. Expand/run real Chromium/WebKit assignment journeys, accessibility and screenshots. No production source/API/Rust/fixture-driver/gate/lock edits or mocked backend; report product defects to original author. |
| Independent acceptance review | Fresh read-only reviewers under review-cycle after writers freeze. | Pending. Initial review/repairs, exact original-author validation and separate fresh final exact-commit review remain required. |

Before browser dispatch, root verifies the acknowledged source checkpoint, prepares a detached isolated checkout and bounded contract, routes actual risks and resolves its checkout-specific guidance/preflight. If the runtime driver is not yet available, the browser author can complete assertions and static checks but must report actual execution as pending; no mocked substitute establishes real-browser acceptance. Keep original work moving on the runtime dependency. Only explicitly owned files return from that worktree.

Track successful and failed outcomes in the existing whole-feature run. Do not sum overlapping child durations or claim a measured speedup without a comparison. Report delivered user outcomes and elapsed delivery/repair evidence. The next browser slice is one bounded implementation pass plus same-context repairs; no worker creates another plan/review/delegation tree.

### Revision7: shared release UI execution profile

ADR0009r2 is independently ARCHITECTURE READY after its one valid directory-count finding and same-session focused recheck. Original fullstack integration author owns the dependent implementation: pure org-release exports/enforces 64MiB aggregate UI bytes and4096 loader entries (regular UI files plus unique proper inferred directory prefixes below ui/, excluding root), unchanged whole-release limits; gateway consumes shared constants and independently checks actual bytes/count/inventory before allocation/secrets. Add nested exact-limit/one-over tests and host prepublication refusal. Preserve actual kernel csops/audit-token binding and static asset authentication. This decomposes the reviewed correction with no new dependency direction/resource increase/acceptance waiver; original contract and all final gates remain. Read/ACK active ADR9r2 and refreshed packet before dependent phase. Browser source stays with its isolated author; root retains overall architecture.

Revision7 scoped parallel dispatch,2026-11-16: original fullstack author acknowledged all six main org-release source hashes and froze that prefix for the retained original library author. That helper implements only MAX_UI_BYTES/MAX_UI_ENTRIES, strict manifest enforcement and nested boundary tests in its existing separate worktree; Cargo/consumers are excluded. Root independently verified all starting hashes and current helper capability validation0; helper reads/ACKs/preflights before writing. Main author retains host/web/setup/platform/store and production UI. Browser-owned test/config sources stay frozen between actual journeys. At most two product writers; root imports only the declared library delta and then releases the prefix. No full-feature acceptance implied.

Current ownership checkpoint,2026-11-16: the verifier author returned only `crates/org-release/{README.md,src/lib.rs,src/tests.rs}`; root imported exact hashes and main focused formatting/19tests/Clippy passed. Helper is frozen and main org-release ownership returns to the original fullstack author for integration. That author owns all host/web/setup/platform and production UI files. The preserved browser author is executing unchanged `apps/agentic-org-ui/e2e-selected/**` and `playwright.selected.config.ts` against fresh actual fixture6; it writes no source during this run. Root owns architecture/plan/stablecontract, verification receipts and integration. New AGENTS instructions were actually reread; main packet r6, library packet2 and browser packet2 have refreshed source ACK/preflights. Dependency order remains sharedprofile -> consumeradmission -> normalsetup/service/recovery -> fullchecks/review -> v1-integration; browser exercises the existing actual protecteddebugfixture concurrently and does not establish production signing/setup readiness. At this checkpoint one product writer remains active, with browser verification independent.

Remaining-work parallelism assessment,2026-11-16: keep runtime pin, authenticated asset loading and setup composition with the original author while their typed ports and presecret ordering stabilize; a second consumer/CLI writer now would copy a moving host API or share app composition. The independent verifier delta and real Chromium run are complete and retained; root has run the exact native kernel check blocked by the worker sandbox. Reassess at the next admission/setup checkpoint for an isolated service/legacy/browser slice with stable interfaces. Current concrete scopes and dependencies are recorded in `.agent-kit/logs/v1-assignment-integration/parallel-execution-checkpoint-2.json`. This dependency-bound serial phase preserves the two-writer maximum and does not change any acceptance or review gate.

Checkpoint after setup adapter verification, 2026-11-16: complete unchanged Chromium journey passed1/1; browser and library source workers are frozen with author contexts retained. Original fullstack author continues host setup/recovery/authentication and office legacy/fault work. Root verified the requested disposable launchd adapter1PASS with stable source/binary hashes and cleanup, and completed a separate bounded Linux pinned-Tokio feasibility probe; neither establishes production signing/platform setup readiness. New concrete legacy mapping triggers only a focused TAI-P11 architecture review while authentication work proceeds. No second ready source slice exists until that review or actual additional browser controls are ready; reassess then rather than duplicating investigation. Current precise file ownership/dependencies and evidence are in `.agent-kit/logs/v1-assignment-integration/parallel-execution-checkpoint-3.json`. New delegate communication guidance requires main packet8 actual read ACK/preflight; completed helpers have prepared packet3 for their next real assignment. One architecture/plan/acceptance/integration owner, two-writer maximum, separate worktrees and every full production/test/review gate remain unchanged.

### Revision8: conservative first legacy adoption profile

[ADR-0010 / LEGACY-ADOPT-1 r1](../../docs/adr/0010-legacy-task-adoption.md) is independently ARCHITECTURE READY after one focused pass. It resolves the concrete TAI-P11 source mapping without changing ADR6 authorization, codec, persistence, number or public-private boundaries. Original fullstack author first updates owning05/12/related mapping documentation, then implements the existing staged-adoption route's strict explicit metadata/source comparison for version1 legacy snapshot plus exactly one matching Created event. Use baseline1 at import time for newly supplied selected facts; preserve original raw source event/actor/time and reporter/creation facts, IDs/numeric tokens/origins/watermarks. Missing, private, colliding, longer or inconsistent histories stay intact/staged. Existing same-writer frozen acceptance installs gate/state and preserves exact account-bound retry. Cover the profile's full negative/legacy fence/reopen checks. No universal reconstruction or backdated owner facts. Main current packet9 includes the new decision and delegate guidance; actual read ACK/preflight required before dependent code.

Parallelism after review: keep the bounded importer with the original author, who has already inspected its private writer/adoption source and is finishing auth/setup composition. A second writer would need a large moving source basis and duplicate that context for a small first profile. Root coordinates review/targeted runtime checks; completed browser/library author contexts remain retained. Reassess if a new sizeable stable independent slice appears. Fullproduction/auth/recovery/browser/legacy/check/review gates remain unchanged.

### Current browser authentication split, 2026-11-16

The original driver author acknowledged the exact private fixture interface in `author/browser-auth-driver-ack-1.json` (agreed contract SHA `55a1b9d77d3ccd2218639cefc204b1faf9ad42869f1dc546f8b177b6eddf2226`). The preserved browser author now implements only the four existing selected-browser test/config files in its separate checkout. The main author owns all Rust driver/legacy/product work. Root coordinates architecture and verification without becoming a third writer. Browser static implementation runs independently; actual fresh-fixture runtime waits for the coherent Rust driver checkpoint. Exact ownership, dependencies, acceptance and source-bound receipts are recorded in `../logs/v1-assignment-integration/parallel-execution-checkpoint-3.json`. Full production and independent review gates remain unchanged.

## Revision 9: reviewed protected recovery delivery prerequisite

Active [ADR-0011 REC-DEL-1 r1](../../docs/adr/0011-protected-recovery-delivery.md) was independently reviewed READY with no findings on exact candidate SHA `0632ef4695f6c9311d18169c114fb673495f440baa44eb044fb141712cf97f88`. The original main author materialized the exact lead activation, preserving two writers. Same architecture owner/unchanged full acceptance contract. Implementation order: update owning native/setup protocol docs; strict versioned public-only one-shot worker descriptor/DTO; private macOS ARC/blocks OS-presence adapter and bound connection ACK permission; protected versioned delivery/audit/ack_pending deletion migration/reopen; shared CLI collect/defer/create-new export/TTY/off-HQ acknowledgement and setup integration; targeted actual/fault/negative checks, then complete feature checks/reviews. Dependent recovery work requires current packet10 actual new-source ACK/preflight. No actual OS prompt or real user/private-key/service mutation is authorized.

The original main author exclusively owns these product paths; browser helper concurrently owns only its four test/config files. Root coordinates, reviews and runs exact disposable native/runtime checks without a third product writer. The repaired actual native request4 passed21.875s with all618 captured Rust/Cargo sources unchanged. Immutable executable copies decouple browser runtime from subsequent main Rust changes. Exact guard maintenance is in `release-verification-design/source-inventory-maintenance-assessment-1.json`: eliminate generated Rust includes, retain ADR9 strict build-time registry validation, allow only exact reviewed helper edges and fixed native-source/whole-script pins. Run guard tests after the coherent build-source checkpoint, then full current checks. Architecture readiness and generated debug/native passes do not establish physical authentication, production roots/signing/normal service/Linux/WebKit or final feature acceptance.

### Current execution checkpoint: bounded browser admission repair

At the 2026-11-16 04:20 UTC checkpoint, root remains the sole architecture, plan, acceptance and integration owner. The original main author implements ADR11 native recovery/controller/CLI/setup and precise build/native-source guards in the main worktree; its materialized, already reviewed cc1.11.1 edge was reassessed and packet12 prepared. The retained browser author independently owns only the three selected-browser spec/support/README files in its separate worktree, correcting an additional late availability assumption into strict known/proven-missing admission-refusal checks. Configuration and all original foreign privacy, two-session revocation, accessibility and CSP assertions remain required. No product authority/fencing change is authorized. Root coordinates and verifies without becoming a third product writer. Exact paths, dependencies, handoff receipts and all unchanged production/check/review gates are in `../logs/v1-assignment-integration/parallel-execution-checkpoint-4.json`. Actual browser execution waits for a new explicit coherent driver checkpoint and immutable fixture; no stale request reuse. The next usable delivery remains the complete accepted task-assignment vertical, not this scoped repair.

### Current execution checkpoint: stability handoff and restart evidence

At 2026-11-16 04:51 UTC, the same browser author completed only `e2e-selected/support.ts` in its separate worktree. Root verified the 37-artifact receipt, exact delta, actual ordered nine-source ACK/current preflight and all excluded hashes. The helper is frozen; ownership returns explicitly to the original main author for exact import. The precondition waits for two identical complete geometry samples across separate default-budget polls, then runs every original containment inequality unchanged. No status/tolerance/animation/timeout waiver. Main remains the sole current source writer; root owns the unchanged architecture/acceptance/integration contract and coordinates verification. Exact ownership/dependencies are in `../logs/v1-assignment-integration/parallel-execution-checkpoint-5.json`. Assignment ten-point and recovery eight-point actual process-kill/reopen matrices now pass at unit-fixture scope, with targeted evidence in `implementation/lead-process-crash-evidence-assessment-1.json`. Current full suites, production/platform evidence and all independent review gates remain required before the next usable complete assignment delivery.

### Current execution checkpoint: full checks, browser closure and authority prerequisite

At 2026-11-16 05:41 UTC, root remains the sole architecture, plan, shared-interface, acceptance and integration owner. Current source checkpoint3 passed the complete Rust workspace (3183 passed/0 failed/11 ordinary ignored), required UI checks (1939 tests), explicit kit3, actual built-gateway Chromium CSP and native7. The preserved browser author completed one full selected-Chromium journey (57.8s), including all assignment transitions, genuine two-session revocation, strict terminal admission refusals, accessibility and CSP; root verified all24 artifacts/10 masked screenshots and protected fixture cleanup. These source-bound generated-debug-fixture checks do not replace authentic production/platform evidence or independent feature reviews.

Current split/dependencies and exact ownership are in `../logs/v1-assignment-integration/parallel-execution-checkpoint-6.json`. Browser and library product source scopes are frozen with author contexts retained. The original main author performs only a bounded no-prompt SDK lifecycle probe against exact existing native bytes; root coordinates evidence and one fresh read-only ARCHITECTURE suitability review of ADR6's unimplemented existing-HQ first-owner prerequisite. No source writer is currently active in those lanes; any product repair requires explicit ownership and current packet/preflight. At most two writers in separate worktrees remains the ceiling.

The existing-instance admission prerequisite is incomplete: registered path/key custody, signed software, a host administrator or legacy profile flags do not establish organization ownership. Genuine protected owner state before writer reservation is reachable only within the exact interrupted fresh-provisioning reservation; it does not authorize an arbitrary old HQ. Dependent first-owner admission implementation remains paused pending authoritative proof and an agreed interface; unaffected new-HQ/recovery verification proceeds. Requirements, original histories and all production-readiness/review gates remain unchanged. The next usable delivery remains the accepted complete task-assignment vertical, followed by the recorded incremental roadmap.

Focused existing-owner suitability review completed ARCHITECTURE INCOMPLETE on checkpoint1; see `../logs/v1-assignment-integration/existing-owner-architecture-review/root-disposition.json`. It independently retained ADR6 requirements and confirmed the missing first-owner producer. This is an incomplete planned prerequisite, not a disproved requirement or permission to infer organization ownership. No dependent first-owner interface is approved.

At 05:53 UTC the exact-byte no-prompt SDK probe completed32scenarios across ordinaryARC/ASAN builds with both fail-closed controls and no source repair. Root verified34command/log records and targeted interception/lifetime boundaries (`implementation/lead-sdk-lifecycle-probe-assessment-1.json`). The original author now exclusively owns two new permanent regression files, `crates/org-host/tests/recovery_presence.rs` and `crates/org-host/tests/fixtures/recovery_presence.m`, under the narrow current packet/ownership contract in checkpoint6. Production native/build/Cargo/guard/security interfaces remain outside that bounded assignment. Root continues architecture and evidence work, with no second product writer. Current full checks remain historical checkpoint3 until changed-source regression and final coordinated verification complete.

## Execution checkpoint 7: structural commit gate, 2026-11-16

The first ordinary local WIP commit was rejected by the installed Repowise staged health hook: 61 high/critical structural errors across 36 files. No commit was created. Full checkpoint4 checks and browser11 remain historical passing evidence for the exact pre-refactor sources; they do not prove later refactors. Preserve the original index/patch backup and all author contexts. The complete acceptance contract and existing-HQ authority pause remain unchanged.

Root remains architecture/shared-interface/acceptance/integration owner. This is behavior-preserving decomposition within reviewed APIs, validation order, durable ordering and security boundaries, not a new product mechanism. No additional architecture pass is needed unless implementation discovers a material interface/policy/storage change. Return such changes before edits. Do not disable the hook, suppress findings, lower thresholds, weaken assertions or replace coherent code with cosmetic counting tricks.

Concrete split (exact files and source hashes are in ignored structural-gate-refactor-1/*-findings.json):

- Preserved fullstack author, Sol Xhigh, current main worktree, base73a39: the 34 non-browser/non-org-release files in author-findings.json. Own private cohesive decomposition of the flagged functions; no new public interfaces. Preserve production native ownership/lifecycle behavior and exact source guards; return any required guard/pin scope before modifying it. Run targeted affected tests/Clippy and source diff checks, report changed hashes and remaining findings.
- Preserved browser author, Sol Xhigh, existing browser-worker detached73a39 worktree: only e2e-selected/support.ts. Preserve its exports, all54 assertions, fixture validation, privacy, error/geometry semantics and time budgets; split the fixture/monitor responsibilities into meaningful private helpers. No spec/config/README/status/timing changes. Root owns integration import.
- Preserved release-library author is pending, not concurrent: after browser writer finishes, assign org-release/src/lib.rs (and existing tests only if a genuine regression needs coverage) in its separate release-worker worktree. Preserve ADR9r2 strict verification/error ordering/public API. This lane depends on explicit root handoff and current packet; no writes now.

Maximum two concurrent writers: fullstack author plus browser, then fullstack author plus library. Root does not become a third writer. No active model reviews during source changes. Existing packets must be refreshed for changed plan sources; every delegate reads current AGENTS and delegate-only caveman before responding and records actual preflight. Receipts are concise and source-bound; detailed findings/logs stay on disk.

Integration order: browser import, release-library import, fullstack author handoff; then inspect meaningful diffs, rerun affected full checks and mandatory staged health gate, create a normal unaccepted local checkpoint, and run fresh initial SolHigh review, original-author intent validation and separate final SolXhigh exact-commit review. No merge/acceptance until complete production readiness, legacy/platform evidence and existing authority prerequisites pass. Next usable delivery remains the assignment vertical, not structural cleanup.

## Execution checkpoint 8: coordinated native source and exact pins

The original fullstack author finished33 non-native structural refactors and stopped its turn with native source unchanged. Root verified all34 owned current hashes,48 actual check-log hashes, scoped repairs and unchanged contract; source-invariants retain432 assertion token streams/public signatures (static evidence, not semantic equivalence). Root imported the explicitly frozen browser-support398f7090 and release-librarya5c986c7 changes after targeted patch/receipt inspection. Those original author contexts remain retained. All helper writers are frozen. Full4/native7/browser11 are historical pre-refactor evidence; current full integration checks/reviews remain required.

Root now explicitly hands the SAME fullstack author ownership of exactly3 writable files: crates/org-host/src/recovery/presence.m, crates/org-host/build.rs and crates/org-office/src/store/tests/opener_build_pins.rs. The other33 refactored files stay frozen. Worktree/base/branch unchanged; SolXhigh, no nested delegation. Root remains architecture, shared-interface, acceptance and integration owner; no second source writer is needed for this short coupled change.

Current packet author-native-pin-packet-r2.json covers the existing34 plus the2 pin paths; its10 required guidance bodies are identical to the prior actually read packet. Revalidate retained actual readings/hashes, read this updated plan and scoped handoff, and run fresh actual preflight before writes. Exact pin baselines and root mechanism inspection are in native-pin-ownership-decision-1.json; no source/API/authentication premise changed. Private cohesive extraction only within local_token preserves all validation/call/error/shortcircuit order and existing C ABI. Callback state/ARC/LAContext/dialog/cancellation mechanisms stay unchanged.

Update only the complete BRIDGE native byte literal in build.rs, the matching complete host build-script literal in opener_build_pins.rs, and its exact NATIVE_SHA256. Preserve whole-source equality, full build-script tokens, source confinement, flags/frameworks/libbsm/build-trust validation and Cargo inputs. No wildcard/helper/include/config/rule/threshold/assertion exception. Return any further required scope or behavior change before dependent edits.

Run the ordinary recovery_presence integration test (both native compiles/all32 isolated ARC+ASAN scenario processes), relevant existing storage/source-pin guards, scoped host/office Clippy and formatting/diff checks. No real OS authentication prompt, private key, live service or organization root. Concise source-bound3-file receipt and actual logs; update compact canonical author handoff pointer without overwriting preserved history. Then freeze all source for root whole-feature checks, actual native/browser verification, normal staged structural gate and unaccepted WIP commit before fresh SolHigh review, original-author validation and separate SolXhigh final exact-commit review.

This is implementation decomposition under existing reviewed contracts; no new architecture mechanism or public interface warrants another plan-review pass. A discovered semantic/security/storage change returnsroot. Existing-HQ firstowner admission remains paused and authentic production/platform evidence remains open; no merge or assignment/v1 completion is claimed.


### Execution checkpoint9: review repairs and efficient verification

The lead re-read current AGENTS.md and keeps the overall architecture, plan,
acceptance contract, shared interfaces and integration. No contract, trust,
storage or recovery policy changes in this repair checkpoint. The initial
independent review of commit57b209a9 remains INCOMPLETE/BLOCKED; its three
confirmed source findings have bounded same-session repairs, without implying
whole-feature approval. The original fullstack, browser and library author
contexts/worktrees and parked work remain preserved.

| Lane | Exact ownership and dependency | Current handoff / next check |
| --- | --- | --- |
| Repair author, existing session01a1e35e-8c1b-74d0-be39-12fa950c1d87, SolHigh | Same main checkout/branch, base73a39c58. R1 canonical adoption privacy and R2/R3 session/channel lifetime repairs; latest structural turn owns only apps/agentic-org-web/src/sessions/control.rs and control/tests.rs. | Terminal private ChannelCache extraction;11session tests and scoped Clippy/fmt pass. Existing source/assertions and staged repairs preserved. Root actual structural gate and unrestricted complete checks required. |
| Root parallel verification, no source writes | Frozen host/office and dependency inputs; no dependency on changing web sources. Build actual office and native host test; execute generated disposable protected setup and worker restart. | Useful parallel check reached a new test-only nonblocking-reader WouldBlock failure, previously hidden by reviewer sandbox bind denial. Actual failure retained; no production failure or passing restart inferred. |
| Next bounded same-session repair | After web writer terminal, transfer only crates/org-host/src/bootstrap/tests.rs. Reuse existing socket readiness API with one absolute deadline across response bytes; preserve every lifecycle/authority assertion. | Root owns unrestricted actual rerun after terminal handoff, then source freeze and coordinated full workspace/kit checks. No concurrent writer in this checkout. |
| Preserved browser author, read-only execution next | Original isolated browser worktree; four unchanged E2E/config/support sources. Depends on final frozen current native/web/office binaries and static UI, with immutable input manifest. | Run existing full assertion journey against new source-bound runtime after repairs; concise receipt and targeted root evidence verification. No duplicate routine investigation or new source ownership. |

No second ready source slice justifies another writer while these channel-lifecycle
repairs settle. Root's independent host/office execution is useful parallelism
without a source conflict. The ceiling remains two concurrent source writers,
including the lead, in separate worktrees with agreed nonoverlapping interfaces.
Root coordinates/verifies while delegates write. Ownership expansion or material
public/security/storage change returns to root before dependent edits.

Each continuation resolves and reads current guidance, records its actual
preflight, returns changed-path/check/log/source-hash receipts and retains failures.
Root inspects the changed boundary and verifies receipts rather than repeating
routine worker discovery. All complete production requirements remain: actual
platform/setup/adoption/browser evidence, full required tests, fresh complete
initial review, original author intent validation and separate fresh exact-commit
final review before merge--no-ff/push to v1-integration. Existing-HQ first-owner,
authentic publisher/physical authentication and unsupported-platform prerequisites
remain open; generated debug fixtures and component checks do not satisfy them.

Next usable delivery remains the complete assignment vertical in the parent
contract. Repair checkpoints are not accepted features or v1 completion.


### Execution checkpoint10: canonical and HTTP task-ID admission

Current preservation commit2a23aff7 is unaccepted. Full6 (3192Rust tests),
native9 and unchanged Chromium13 passed against its source, with all mandatory
checks and temporary-runtime cleanup recorded. A fresh independent review still
returned INCOMPLETE: it found two additional task-ID compatibility defects and
did not establish complete290file/output coverage. Both actual review runs and
their unsuccessful metrics remain preserved; no independent approval or merge.

Root keeps architecture/plan/interfaces/acceptance/integration. Resume only the
second reviewer's actual session01a1e3b2-3b57-72a3-8bf9-df094bff9514, SolHigh,
for the bounded repair in second-review-repairs-1/root-assignment.json.
It owns selected_tasks/directory.rs, service/tests.rs and new
service/tests/adoption_paths.rs under crates/org-office/src, plus TESTS ONLY
in apps/agentic-org-web/src/path.rs if useful. All other source authors remain
frozen. Root coordinates and verifies; no second ready source slice benefits
while admission/publication invariants settle. Separate worktrees and max2
sourcewriters remain mandatory if useful concurrent implementation becomes ready.

Validate the complete generated task filename and HTTP representability before
immutable acceptance.253–255byte bare IDs must not become unmaterializable
.md filenames after journaling. Backslash-bearing IDs must not be adopted and
listed while the existing gateway rejects their detail paths. Preserve original
opaque IDs/source bytes in protected staging for unsupported representations;
never rewrite identities or relax the global URI decoder. Positive252byte,
UTF8, percent-looking and other supported identities remain supported. This
implements the current ADR6/10 staging contract, with no new trust/storage/wire
mechanism. Escalate an actual policy/interface expansion before dependent edits.

Require failing-before/passing-after real admission tests. Preserve the existing
durable protected Staged receipt on first refusal (one receipt-only journal
advance); exact retry must not advance again. Require no public/task/adoption
projection or work-origin mutation, exact staging/retry retention,
positive valid publication and reopened readiness. Re-resolve/read current skill
packet and preflight before edits. Root verifies concise source/check/log receipts,
runs early structural and coordinated current fullchecks/runtime as affected,
then required fresh complete review, original author validation and separate
exactcommit final review. Existing production/platform/firstowner requirements
and next complete usable assignment delivery remain unchanged.


### Execution checkpoint11: snapshot privacy and physical channel lifecycle

Root retains the one architecture, plan, stable acceptance contract, shared
interfaces, dependency order and integration owner. Preservation HEAD c3ada005 is
unaccepted. Third actual initial SolXhigh review session
01a1e3e7-cacd-7153-a114-deab8f51ed46 found R1/R2/R3 and returned INCOMPLETE;
its absolute finding paths failed report validation. Preserve the original raw
report, actual execution and BLOCKED result; no code acceptance is inferred.
Full7/native10/Chromium14 and original intent validation are historical evidence
once any repair changes their inputs. All production gates remain required.

| Lane | Worktree / ownership | Dependencies and checks |
| --- | --- | --- |
| Same third reviewer, explicit fix, SolXhigh | Main checkout at c3ada005; only org-office selected_tasks/adoption.rs, adoption/legacy_import.rs, service/tests.rs and new service/tests/adoption_snapshot_privacy.rs | R1 uses existing provenance policy on actual current and historical Markdown, inline and referenced. No new public DTO or parser normalization. Real stage/commit red/green, source retention, first protected Staged receipt and exact retry/reopen, public unknown-extension positive controls; scoped selected tests/fmt/Clippy. |
| Fresh independent architecture reviewer, read-only SolXhigh | Main checkout; candidate ADR0012 channel capacity/lifecycle, host/worker/launch source only | R2 shared physical budget8/gateway7 and graceful ordinary refusal; R3 true socket closure releases occupancy. Review authenticated close/refusal event ordering, pending FD ownership, lock ordering and failclosed malformed-transfer behavior before dependent code. No source writes. |
| Original fullstack author, pending | Separate new detached c3ada005 worktree only after architecture READY and explicit ownership handoff | Candidate org-office-launch v3 protocol/tests, host supervisor/channels and service/authentication/tests, office transfer/channels/tests and startup event wiring. Exact files/interfaces must be enumerated in the approved handoff before dispatch. No org-office privacy files. |

There is currently one source writer. Privacy repair and read-only channel design
review are useful independent parallel work. If channel architecture is READY
while privacy is active, a second writer may start only in the separate worktree
with disjoint exact files and agreed event/state interfaces. Root coordinates and
verifies without becoming a third writer. Otherwise serial implementation is
preferable to speculative cross-process interfaces. Existing browser/library/main
contexts, all worktrees, parked stash and installed CLI/spool remain preserved.

Root integrates terminal receipts, targeted source inspection and early structural
checks, then coherent complete workspace/kit/app and affected native/browser
verification. Preserve independent initial review, original-author validation and
separate fresh final exact-commit review. No merge until the complete stable
acceptance contract passes. Next usable delivery remains the full assignment
vertical; these repair checkpoints do not complete assignment or v1.


### Execution checkpoint12: approved parallel channel implementation

ADR0012 CH-LIFE-1revision2 is ACTIVE after independent SolXhigh architecture
READY at exact archived proposal06073c57959c1a1dae08f4a63f0d5178fe222fc4d6417275a2289e04829063aa.
Root owns the single architecture/plan/interfaces/acceptance/integration. The
source-bound assignment contract remains unchanged. R1 privacy author is still
finishing terminal targeted checks in main; its four files are disjoint from
this channel slice. Preserve its actual session, work and every failed attempt.

Original fullstack author01a1dfb4-1d26-7100-9ea4-72b53756ef23 SolXhigh now owns
CH-LIFE-1 implementation in separate detachedc3ada005 channel-worker worktree.
Exact enumerated paths/interfaces/checks are in channel-implementation-1/root-assignment.json.
No nested delegation or worker commits/branches/push/merge. This material protocol
correction returns to original architecture/fullstack author after root decision,
rather than broadening the privacy reviewer's bounded R1 fix. Preserve third
reviewer context for finding/repair verification and original authors for intent.
Maximum two writers: privacy author main plus fullstack channel-worker. Root
freezes source writes while they are active and verifies/coordinates only.

The shared interface is the reviewed8total/7gateway profile/readiness marker,
authenticated ACK/refusal/close lifecycle, per-ID physical liveness witness and
closed-aware Session/native/gateway ownership receipts, retaining16pending,
512combined listener and64cross-office session limits. No new host-control API,
trust authority, dependency, SQL schema, native pin, global error waiver or source
normalization. Worker returns genuine extra ownership or mechanism requirements
before editing them. Existing writer/recovery/revocation/failed-rights invariants
remain. Targeted failing-before/passing-after tests and concise evidence receipt
precede root integration; root verifies current hashes and affected boundaries,
runs early structural gate then coherent complete checks and actual native/browser.
Independent initial, original intent and separate fresh final exact-commit review
remain mandatory. All production/platform/existing-owner prerequisites stay open.

Next usable delivery remains the full assignment vertical; repair and architecture
READY are not accepted assignment or v1 completion. Canonical plan15 is authored
in this worktree and will be integrated by root after both handoffs; main plan14
remains preserved during the privacy writer's active turn.


### Execution checkpoint13: owner defers release signing to the final phase

The owner's explicit answer supersedes earlier per-feature signing-input gating:
release signing needs setup and is to be skipped now, handled last after all other
implementation. See v1-release-signing-deferral.md for the exact scoped phase
amendment. Signing/verifier production code and failclosed checks are preserved;
no release bypass, generated production keys or fictitious production proof.
Missing authentic signing inputs must not stop functional implementation or local
reviewed v1-integration delivery. Complete functional tests, original intent and
fresh initial/final exact-commit reviews remain required. Final signed release,
normal production setup/service and signing-dependent E2E remain deferred release
acceptance. Other security, durability, auth and platform requirements remain.

Active channel author continues unchanged CH-LIFE1r2 source interface in separate
worktree; its frozen plan15/ADR2 packet remains valid for that unchanged scoped
implementation. Root owns phase16 planning/metadata in main; no concurrent edits to
worker-owned16sourcefiles. Refresh subsequent handoffs/preflights against phase16;
communicate this scheduling amendment on the next preserved-author turn. R1 scoped
repair is terminal and root verified; early staged health passed0fail/2nonblocking
cohesive test-fixture warnings, without any policy/assertion/threshold change.
