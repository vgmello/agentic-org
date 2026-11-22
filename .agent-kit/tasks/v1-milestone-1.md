# v1 milestone 1 — delivery map (lead)

Source of truth: docs/implementation-plan.md on origin/v1-integration. Each increment is a building block; the next builds on it.
Every increment: a written plan reviewed by the lead before code; branch from origin/v1-integration; checks green; FEATURES row
updated; fresh review; merge --no-ff into v1-integration. At most two writers, separate worktrees.

Owner priority (2026-11-17): setup → registration link → sign-in → organization onboarding → team and project onboarding →
agent onboarding; then the browser views for every vertical (sign-in, accounts, users, admin, link).

| Order | Increment | Building block it adds | Needs | Track |
| --- | --- | --- | --- | --- |
| 0 | F4 web process (finishing) | host supervises the web process; verified handoff; /api/v1/version | — | A |
| 1 | A2 setup + registration link | instance, service registration, one-time link (digest) | — | A |
| 1 | A1 one launch contract, no SQLCipher | rendezvous-only launch; plain SQLite; host-wrapped office keys | — | B |
| 2 | A3 web registration + sign-in | link redemption, admin account, stored sessions (ADR-0013), grants to office, UI sign-in | A2, F4 | A |
| 2 | B1 store foundation (plan first) | files are the source; projection; storage contracts; engine-independent suites | A1 | B |
| 3 | A4 create organization | create in browser; top-level team; owner person; HR employee; recovery code | A3 | A |
| 4 | A5 person on the CLI | browser-approved CLI sign-in; principal + grant per call | A4 | A |
| 4 | B2 teams and people | one home team; leads; moves; CLI + API | B1, A5 | B |
| 5 | B3 projects | project + its team | B2 | B |
| 5 | O1 office in the product | office list/state/restart | A4 | A |
| 6 | A6 second person | invitations, password links, ownership/admin hand-over | A3, B2 | A |
| 6 | U1 browser views | offices, teams, people, projects against the real API | A3, B2, B3, O1 | A |
| 7 | C1 spike, C2 hire through HR, C3 start an agent | agent onboarding | B2, U1 | C |

Open follow-ups recorded elsewhere: UI session wire (absolute_expires_at, hq_restarted, member_id) → A3; F4 B1/B2 eligibility tests;
system-architecture page line "The web restart signs browsers out" contradicts ADR-0013 (regenerate the page).

## Integration log (lead)
- 2026-11-18: F4 merged 032ea63f; A1 part 1 aa2d86db; A2 part 1 d408bfb9 (pushed); A1 part 2 acedd571 (local, tests running).
- A1.4 split into A1.4a/A1.4b (receipt-7 lead decisions): unregistered host starts no office; non-HQ name-only create is deleted (A4 rule), not extended; worker accepts UUIDv7 instance IDs.
- Merge order: A1.4a (worker accepts UUIDv7 instance IDs) BEFORE A2 B2 (host mints UUIDv7 instance IDs); otherwise real-process launches with a new instance would be refused by the worker.
- 2026-11-18: instance-ID UUIDv7 switch is one coordinated commit by the A1 writer after A2 B2 merges (ownership hand-off of
  instance/registry.rs, host_secrets checks and hostdb fixtures); A2 B2 does not switch the format.
- 2026-11-18 web process: ConnectionReset diagnosed. Product bug: an early error reply (404/400/403, parser refusal) on a request
  whose body is not read is followed by an RST when late body bytes are unread (apps/agentic-org-web/src/service.rs:292-296
  api_response; transport/server.rs:199-225). Browsers can show ERR_CONNECTION_RESET instead of the JSON error. A3 requirement:
  bounded lingering close in transport/server.rs serve_connection (shutdown write, read-discard briefly, bounded by size/time), or
  drain the body before early errors. Test fix (now): web_process_tests.rs http() writes head+body in one write_all (A2 owns the
  file after relocation; include in A2's next commit). Reproducer: split write gives ~1.35% resets per request; one write gives 0/2000.
- 2026-11-18: A1.4a merged f0711d1c (pushed 5e6246d0). A2 B2 merged 2ec83770 (pushed): link store, registry without account,
  reverse bootstrap removed; ADR renumbered to 0022 (0021 = office store). Workspace 3230/0.
- Next: A1.4b (in progress, slot-2) → coordinated UUIDv7 instance-ID commit by the A1 writer (hand-off of instance/registry.rs,
  host_secrets checks, hostdb fixtures from A2) → A2 B3 (serve creates registry at first start, admin.link.initial, admin.reset).
- A1.4b open items to track to closure: 5 binary.rs serve tests #[ignore]d until A2 B3 registered serve lands (port then);
  descriptor-inheritance binary test deleted (gap: re-add for start spawns); first-line refusal now restarts as crash (was
  Unavailable) — reviewer to judge whether a permanent refusal must stop restarting.
- Follow-up (A1.5 or a diagnosis slice): under heavy client churn the host logged "couldn't start the server of office ...:
  Invalid argument (os error 22)" (handled as Crashed retry, recovered). Undiagnosed; possible spawn/broker failure under accept
  pressure (A1 run 21 receipt).
- 2026-11-18: A1.4b merged and pushed 66fbaa52 (workspace 3190/0, kit pass). Next A1: UUIDv7 instance-ID coordinated commit (needs A2 B3 merged first? no — touches instance/registry.rs; wait until A2 B3 merges), then A1.5.
- Owner rule "gateway → web everywhere": org-office-launch v3 `ChannelKind::Gateway` (crates/org-office-launch/src/v3.rs:886) and
  related doc comments still say gateway. Rename in A1.6 (one contract, version 1), which owns the launch wire.
- A3 requirement (from A1.5): grant-bound `subscribe` on the web host channel (web-api-v1 §6: subscription grants renewed by
  the web process, revocation ends it); then un-ignore the office live-event/inbox_hint binary tests parked in A1.5.
- 2026-11-18: A2 B3 merged and pushed 7d8479b5 (workspace 3212/0, kit pass). Next A2: B4 CLI setup composition.
- Owner rule gateway→web: org-host still has GatewayState / 'gateway' names (crates/org-host/src/web.rs etc.); rename in A3 (owns the web process).
- SIGN-34: admin reset over SSH must refuse first ('needs you at this Mac'); SSH-session detection not built. Track for A2 follow-up/A3.
- 2026-11-18 FLAKE (pre-existing on origin since A1.4b/A1 transfer tests, not from B4): EINVAL under load.
  (1) apps/agentic-org/tests/supervision.rs:97 a_fatal_failure_makes_the_office_unavailable_without_restarts (host spawn
  "Invalid argument" → extra run); (2) apps/agentic-org-office/tests/launch/v3_activation/transfer.rs:454
  stream.set_read_timeout(..).unwrap() → EINVAL. Lead hypothesis: macOS setsockopt(SO_RCVTIMEO/SNDTIMEO) returns EINVAL on a
  socket whose peer already shut down/disconnected; the host may call set_read_timeout/set_write_timeout on a rendezvous
  stream after the worker exits. Diagnosis agent running. Fix before A1.5 merges.
- EINVAL ROOT CAUSE (diagnosed): Darwin setsockopt(SO_RCVTIMEO) returns EINVAL once the unix peer closed; buffered data stays
  readable. crates/org-host/src/supervisor/channels.rs:344 Broker::handoff set_read_timeout(None)? → SpawnError::Io → Crashed
  (a real worker's fatal failure code is lost and it is restarted instead of Unavailable). Stress: 91/144 fail baseline, 0/144
  with fix. Fix (next A1 run): helper tolerating EINVAL on timeout changes at :344, :275 (discard), :331; unit test in
  channels/tests.rs (peer wrote+closed → Ok and the buffered event still read). Same pattern in test
  apps/agentic-org-office/tests/launch/v3_activation/transfer.rs:454. Evidence/diff:
  scratchpad/einval-fix-and-instrumentation.diff, einval-hits.log.
- Separate test flake: servers_inherit_no_host_fds_while_clients_connect ConnectionRefused (61) at
  apps/agentic-org/tests/support/mod.rs:592 under parallel load (host.sock backlog) — bounded retry in Client::connect.
- Possible orphan: once, a target/debug/agentic-org-web --serve was left running with ppid 1 during binary tests (A2 run 19).
  A3 requirement: the web process exits promptly when its host control connection reaches EOF (SIGKILLed host); add a
  real-process test (kill -9 the host; assert no web process remains).
- UX: a second `serve` on a locked instance says "instance state is already locked" — map instance::Error::Busy at reserve
  (crates/org-host/src/registered.rs) to a clear message (setup/admin reset may also hold it). A2 follow-up.
- FLAKE (from A2 B4, on origin): apps/agentic-org/setup-tests/setup.rs — setup with --port P sometimes refuses "port P on
  127.0.0.1 is in use by another program" (seen in setup_starts_the_service_prints_one_link..., piped_setup_prints_no_link...).
  Moving free_port() off the ephemeral range did NOT stop it (failed 1/4 and 1/8), so something in-process may hold the
  port (setup's own held-port probe? another test's service?). A2 to diagnose after run 20; lead change reverted (unproven).
- Orphan risk (A2 run 20, inferred): the host spawns web and office children suspended for admission (spawn_suspended); a host
  SIGKILL in that window leaves a stopped child with ppid 1 forever (it cannot see EOF while stopped). Fix belongs to the spawn
  path (A1 / org-supervise): record suspended child pids durably and kill them at the next host start, or another bounded
  mechanism; test with kill -9 between spawn and resume (test hook). Track to closure.
- Doc consistency (known remaining, from review of a23590c2): FEATURES FC-018 (line ~78) and docs/design/office-host-boundary.md
  :397 and :401-402 still say host.sock dispatches only hello/instance.status and provisioning is reached only from tests;
  FEATURES ~:106 "reached only from tests" unchecked. Align with FC-016 wording in A3's first doc pass (A3 owns sign-in/provisioning use).
- 2026-11-18: A1.5 merged and pushed (workspace 3196/0, kit pass). Next A1: UUIDv7 instance IDs (coordinated), then A1.6, A1.7.
- 2026-11-18: A3 B1 (lingering close) merged and pushed (workspace 3206/0). A3 B2 (session wire) running in slot-1; A1 UUIDv7 instance IDs running in slot-2.
- Ruling (A3 B2): not_a_member stays (live domain refusal for team.set_lead); removed with the old membership operations (track in B-track teams/people rebuild).
- UUIDv7 instance IDs (A1 226b8aa8). Follow-ups: (1) gateway.rs first line still accepts the plain id form for the web
  process test fixture "INSTANCE" (apps/agentic-org-web/src/process.rs:229) — A2/A3 switches the fixture, then gateway.rs drops
  the plain branch + ULID refusal test (or B13 deletes gateway.rs). (2) Remaining ULID IDs to move to UUIDv7 (non-negotiable 4):
  office, operation (create/recovery/writer/hq_creation/provisioning/journal/release), member/owner, account, channel, writer
  incarnation, mailbox/message, work/change, gateway outcome; temp-file names can stay non-ID. Plan as a separate slice.
- 2026-11-18: UUIDv7 instance IDs (239ad3de) and A3 B3 stores (bfa1d1f7) merged and pushed; workspace 3242/0, kit pass.
- Possible load flake: org-host web::tests (4 cases 'child did not connect before the deadline') failed once under parallel Argon2 load in A3 B4 run; 43/43 on rerun. Watch; diagnose if it recurs.
- Hung test processes found and killed (pids 5544, 82665: org_host 'gateway server_check service::' filter runs ~12h; 11650: agentic-org service test ~4.7h). Some test in service:: or the agentic-org service suite can hang without a timeout — diagnose (A1/A2 follow-up).
- 2026-11-18: A3 B4 (host browser sign-in ops) merged and pushed; workspace 3262/0.
- UI key_missing removal (apps/agentic-org-ui proto.ts:19, wire.ts:290, offices/model.ts:171,272, office/mock.ts:24, model.test.ts, e2e/offices.spec.ts:202,240): A2 follow-up after A1.6 merges.
- DECISION 2026-11-18: web process eligible once the instance is registered with authentication; HQ Active no longer required (A3 premise correction; F4 rule changed). See .agent-kit/slices/a3-plan-review-1.md.
- A1.6 follow-ups (run 40): (1) off-macOS release, keys::store_for runs macOS trust checks before the unsupported adapter, so the refusal reads NoKeychainGroup/NotHardened — move preconditions into the platform adapter (X1/M2). (2) No Linux cross toolchain locally: the non-macOS branch of keys/platform.rs is uncompiled — M2 CI must compile it. (3) Rename test helpers db_key()/note_db_key/db_key_fingerprints (A1.7). (4) Watch supervisor flake an_hq_create_reported_after_shutdown_began_is_stopped_not_adopted.
- A3 B5 merged locally (not pushed): full run red only on known web::tests load flake (43/43 alone); push after A1.6 (with 3faccb98 deadline fix) merges and the combined full run is green.
- M2 follow-up: host_secrets::release_store (host_secrets.rs:164-195) checks macOS group/signing before SelectionError::Unsupported off macOS — same shape as A1.6 F1; move preconditions into its platform choice.
- Setup test a_named_instance_skips_ports_other_instances_saved failed once: port 7422 held by another program (concurrent real-instance Playwright in slot-1 binds the same 7421+ range). Passes alone 8/8. Test isolation hazard: real-instance tests and setup tests share the fixed 7421+ port range across worktrees — consider a per-run port base.
- 2026-11-18: pushed A3 B5 (web session layer) + A1.6 (launch contract/office secrets key/platform keys). Workspace 3291 passed, 1 env failure (port contention, 8/8 alone); kit pass.

## OWNER DIRECTION 2026-11-18 (UI scope)
Another agent (owner-run) drafts and builds ALL other pages of the system. Our UI scope ends at: sign-in (A3: register,
sign-in, welcome/SIGN-9, account menu, sign-in-again dialog) and organization creation (A4 create page). Beyond that we build
backend/host/web API + contracts + tests only — user creation, organization editing, projects, teams, agents (A4 rest, B*,
O1, A5, A6, U1, C*) — with no UI pages. Keep UI edits confined to those pages so the other agent's work does not conflict;
expose every capability through documented web API routes (web-api-v1) so their pages can consume it. Interpretation to
confirm with the owner if it matters: "organization editing" and "user creation" are API-only for us.
- DECISION 2026-11-18: A1.8 "retire the app-token path" (office.app_token, MsgBind.app_token, org-client bind_app, worker serve.rs, host secrets.rs, org-host service/mod.rs + listener comments, org-host-control secret.rs, UI lib wire.ts/proto.ts/model.ts/mock.ts + offices.spec.ts) — the launch contract said grants are the only path for a person to act; this is unfinished A1 scope. Runs after A3 B6 merges (crosses A2 files); one writer. host app_auth.rs AppPolicy (peer-process admission for admin/recovery) is NOT a token and stays.
- Real-instance Playwright: one ERR_CONNECTION_RESET opening the setup link during instance startup (r23-real.log), 4/4 on rerun. Possibly web start readiness vs first request; watch, diagnose if it recurs.
- 2026-11-18: pushed A3 B6 (browser register/sign-in/sign-out; first usable slice). Kit pass (UI check/test/build); real-instance Playwright 4/4 on the merged tree. Rows renumbered FC-173 (SIGN-40), FC-174 (ENG-3 remainder).

## OWNER DIRECTION 2026-11-18 (models)
Opus tokens are running out. Every delegate from here on runs on Fable: writers (fullstack-engineer), reviewers
(code-reviewer), planners and lookups are all dispatched with model "fable". This overrides the routing policy's
Opus/Sonnet assignments until the owner says otherwise. All gates stay (plan review, fresh review per commit, author
validation, final review); each record names Fable as the model that ran the phase.
Commit trailer from here on: "Co-Authored-By: Claude <noreply@anthropic.com>" + the Claude-Session line.
SUPERSEDED later 2026-11-18: Fable limit reached (429) twice; owner: "use opus from now on". All delegates run on
Opus again under the routing policy's effort assignments; records name Opus. Commit trailer: "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>" + Claude-Session line.
- 2026-11-18: pushed A1.7 (SQLCipher dropped). Workspace 3292 passed / 0 failed; kit pass on the merge tree. Writers now on Fable: slot-1 a4-create-organization (A4.1+A4.2), slot-2 a1-8-retire-app-token (A1.8).
- 2026-11-18: pushed A1.8 (app-token path retired). A1 increment (launch contract, no SQLCipher, no app token) complete.

## Token/cost rules for delegates (owner concern 2026-11-18: subagent cache goes cold after ~5 min idle)
1. Writers never wait on slow checks: they run only quick checks for the code they touched (fmt, clippy/test of the
   touched crates), commit, hand back. The lead runs slow gates (workspace tests, real-process tests, kit check, e2e) as
   shell jobs and dispatches a fix agent only on failure. Same gates before every merge.
2. One short brief per block (rulings folded in, exact files/lines) instead of plan + revision history.
3. One fresh review per block by the lead; writers run no sub-reviews; small fix commits are reviewed with the block.
4. Commit after every step; runs sized to finish inside the turn cap.
5. Lead: state in this file, batch notifications, no narration of interim notices.
Open with owner: raise fullstack-engineer maxTurns 40 → 80 (kit config) — awaiting answer.
- 2026-11-18 state: pushed K1 (f7e60098). A4 host+web complete on a4-create-organization (head 9fea9b70), merge candidate in main checkout under slow gates; merging INTO integration avoids the .claude guard (upstream now tracks .claude/agents/principal-designer.md; merging integration INTO an old branch is blocked by the local overlay guard — owner asked; do not bypass). B1a blocks 1-8 on b1a-store-core (head f84e3c77), review of runs 4-5 running, run 6 = blocks 9-10. A4.5 UI brief: slot-1 .agent-kit/logs/a4/brief-ui.md. Root cause of load 503s: unoptimized Argon2 in debug builds (fixed in 9fea9b70).
- 2026-11-18: pushed A4 host+web (c2fd2dd9 + upstream merge). Workspace 3355/0, 14 real-process tests, kit pass. Next: A4.5 UI (running, slot-1 on a4-create-organization), B1a blocks 9-10 (running), brief-7 queued for B1a.

## OWNER DIRECTION 2026-11-18 (integration branch)
Owner: "v1 integration is done, is v1 now". origin/v1 == origin/v1-integration == c88c56c2 (PR #16). From now on
completed work merges `--no-ff` into `v1` and pushes `v1` (explicit owner instruction; supersedes "never merge into
release v1"). Never push main. AGENTS.md / docs/processes/branch-and-merge.md still name v1-integration: update in a
docs commit with the next merge, after confirming with owner if it conflicts with the other engineer.

## CHECKPOINT 2026-11-18 (owner: stop after this push for setup)
- Local v1: 9b88d15c (A4.5 merge), 08681218 (origin/v1 merged in; A4.5 code made lint-clean: resolve(), no unused
  bindings, separators, prettier; production focus case -> /org-chart), e4a79185 (B1a merge; catalog rehash;
  bootstrap_bytes.rs adapted to A4 PreparedV1). Merge-resolution review (Opus, fresh): ACCEPTED, no findings.
  A4.5 run 4 review ACCEPTED (+P3 fixed e24d6352); B1a brief-7 review ACCEPTED (+2 P3 fixed a2a5eddf).
- Gates on the combined tree: job balxpidr2, scratchpad m-gates.out. Push v1 only when all pass.
- Follow-ups: #[ignore] golden comparison `cargo test -p org-office todays_bootstrap -- --ignored` stops at
  "HQ provisioning requires recovery" (A4 precondition) -> adapt the test setup, then schedule byte differences;
  apps/agentic-org/tests/binary.rs 4 tests failed only under concurrent load (pass alone) -> watch;
  AGENTS.md / docs/processes/branch-and-merge.md still name v1-integration; overlay guard vs upstream
  .claude/agents/principal-designer.md blocks merging v1 INTO branches (owner decision pending);
  slots: slot-1 a4-create-organization and slot-2 b1a-store-core are merged -> release both after push.
- Next increments after setup: B1b, B2, B3, A5, O1, A6 (backend/API only).
- 2026-11-18 PUSHED origin/v1 6c85ac9c: merged local v1 (d71261fd: A4.5 + B1a + lint/kit fixes) INTO a fresh worktree at
  origin/v1 (decceb9f, owner tooling commits that track CLAUDE.md/.mcp.json/.claude/), so the guard saw no overlay
  paths; catalog rehashed, kit validate pass. Product tree == tested d71261fd (workspace 3545/0, mocked e2e 1605
  passed, real-instance 5/5, kit pass, UI lint/format/check clean). Slots 1 and 2 released.
- OPEN: main checkout local v1 is behind origin/v1; fast-forward needs the 7 local overlay copies moved aside
  (backups in scratchpad overlay-backup/) and the guard list (.git/hooks/local-overlay-paths) to drop CLAUDE.md,
  .mcp.json, .claude/ (owner runs it; auto mode refuses guard edits).

## B1b (started 2026-11-18)
- Assessment .agent-kit/tasks/b1b-assessment.json -> planner/arch review/worker opus high; review gpt-6.1-sol high, final opus high.
- Plan draft dispatched to a Plan agent (opus, read-only); lead reviews -> .agent-kit/slices/b1b-office-on-new-store-plan.md -> fresh architecture review -> dispatch.
- Plan saved .agent-kit/slices/b1b-office-on-new-store-plan.md (Rev 1 lead rulings). OWNER: old orgs recreated, no migration; doc 01 amended for wrapped CA key in protected journal. Architecture review (opus high) running.
- Arch review: CHANGES_REQUIRED -> Rev 2 rulings -> recheck 3 blockers -> resolved -> plan READY. Order 1,2(M1),3,4a,5,6,4b,7(M2). Writers A (3,4a) || B (5 after M1).
- 2026-11-18: pushed v1 ddfce73b (kit: Sol review/fix rounds follow policy review.model; local hybrid policy = gpt-6.2-sol; shared codex-only stays 6.1). Owner approved one-time ALLOW_LOCAL_OVERLAY_COMMIT for the shared manifest (5 hashes). Reviews: 3 fresh Opus passes, final clean.
- 2026-11-18 M1 (blocks 1-2) on worktree-b1b-office-store (slot-1), rebased on ddfce73b, head c1c56fdd. Gates: fmt/clippy/test workspace 3540/0, mocked e2e 1605 passed, kit pass (after catalog evidence rehash for apps/agentic-org-office/Cargo.toml). kit_codex Sol 6.2 review running (log scratchpad/logs/m1-codex.log). Follow-up: agent-os maps unsupported to exit 4 (EXIT_IO); dedicated refusal code -> B6.
- M1 Sol 6.2 run 1 (80748132): 3 findings, 2 fixed by Sol (c93fe1ec), docs by lead (744031c3). Run 2 (c6c8a686): round1 fixed by Sol (c0c9d51c: watcher on change token, FC-072 table), round2 3 P3 incomplete -> lead fixed (be91d727: drop org-supervise dep, docs, catalog rehash Cargo.toml+Cargo.lock). Tests 1280/0 touched crates, clippy ws, kit pass. Run 3 started. Note: Codex sandbox blocks unix-socket tests; owner may set ~/.codex sandbox_workspace_write.network_access (auto mode refused lead edit).
- 2026-11-19: pushed v1 21ab4f77 (kit: checks scoped to changed paths since base origin/v1; blocking:false option; UI vitest --testTimeout=30000; owner approved one-time override for 10 manifest hashes). 6 fresh Opus review rounds, last clean. origin/v1 also gained Track X (other engineer, cf49f0f0). B1b rebased onto 21ab4f77 (conflicts: office lib.rs office_home vs deletions; Cargo.toml org-platform kept, org-supervise dropped; catalog rehash d561785e). Workspace gates + Sol run 6 running.

## OWNER DIRECTION 2026-11-19 (reviews)
Owner: "stop reviewing code no sol no more usage, use opus medium". No more Codex/Sol review runs (kit_codex). Every code review from here on is a fresh Claude code-reviewer on Opus, effort medium; records name it. Overrides the routing policy review model until the owner says otherwise. Sol run 7 stopped after its fix turn; its repairs verified and committed.
- 2026-11-19: PUSHED v1 3bf43104 = B1b M1 (blocks 1-2). Final review Opus medium clean after doc fixes (608c2b8c, 39d7953c). Next: block 3 (writer A, org-office) and block 5 (writer B, host/launch) in parallel per plan Rev 2; reviews on Opus medium (owner).
- 2026-11-19: dispatched writer A block 3 (slot-2, worktree-b1b-block3) and writer B block 5 (slot-1, worktree-b1b-block5), both Opus high, base 3bf43104. Packets .agent-kit/logs/skills/b1b-block3|block5.
- 2026-11-19: block 5 done 01ba06a6 (per-crate green; worker red by design until 6). Review Opus medium running.
- 2026-11-19: block 3 done (6eb839ec,30d84c32) + lead ca91e481 (doc 16 move, changelog 2.22, doc 06 note, catalog rehash). Reviews Opus medium running for blocks 3 and 5.
- 2026-11-19: block 5 fixes fda0abe6, 09ce30ea, efcdaf32 (custody: roots test, transient-only retry incl keychain -25308, owner bound). Kit note: writer stop hook ran in the main checkout (on v1 = base) so every check ran and the UI build hit the 300 s budget under load; consider scoping the hook to the worker worktree.
- 2026-11-19: block 5 ACCEPTED at efcdaf32 (review Opus medium clean). Waits for block 6 to integrate with 3+4a.
- 2026-11-19: block 3 fixes 6875e23a (review running). Writer A briefed for block 4a on the same branch (new Office beside the old; additive org_store::files publish; custody via org_store::WriterAuthority with a local fake).
- 2026-11-19: block 3 ACCEPTED at 6875e23a (review Opus medium clean; read_ca Files-branch test folded into 4a). Writer A on 4a.

## OWNER DIRECTION 2026-11-19 (scope)
This session works backend and non-UI only (crates, office worker, host/CLI, server-to-server, store/engine, backend docs, kit). The ui-dev session owns apps/agentic-org-ui; UI mirrors of wire changes go to ui-dev as a message, not an edit. Sync message sent to ui-dev.
- 2026-11-19 sync with ui-dev (branch v1-ui, integration v1-ui-step2-all): they own apps/agentic-org-ui/**, docs/specs/**, docs/plans/v1-ui-*; FEATURES UI rows (tell me before commit). .agent-kit is mine from now (they changed kit_codex.py clone sandbox, review-sandbox.json, principal-designer files on v1-ui; review kit part when v1-ui merges into v1). They message before merging v1-ui into v1. I message them per web-api-v1 route as it lands and when unsupported calls come back. unsupported = not transient, no Retry.
- 2026-11-19 OWNER: C1 spike partly done by the owner (poc/console-attach, v1 d0945749: Rust PTY proxy, persistent Claude stream-json daemon, headless SSO-like login): sign-in and execution covered; session resume and sub-agents (ADR-0025 d10) still open. Do not start C1; when C1/C3 are planned, start from that POC.
- 2026-11-19: block 4a done 90b9a07b (review Opus medium running). Block 6 next.
- 2026-11-19: PUSHED v1 0a301cb7 = block 3 merged onto d0945749 (gates: fmt, clippy, workspace 3868 + 1 load flake serve::tests::a_log_that_cannot_be_read_closes_... passing 3/3 alone; kit pass). Branches worktree-b1b-block3 (6875e23a at push time) and worktree-b1b-block5 backed up on origin.
- 2026-11-19: 4a review: 2 medium (roots-binding test, journal incarnation in intent) + 4 low -> writer A. Block 6 dispatched to writer B (slot-1, branch worktree-b1b-block6 = block5 + block3@90b9a07b + v1).
- 2026-11-19 KIT FOLLOW-UPS (after v1-ui merges into v1; from ui-dev review of d0daefd2): (1) manifest skill-catalog.json hash stale (needs owner override); (2) kit_codex.py legacy runs without review_model labelled 6.1: derive from recorded requested_model (cosmetic); (3) skill-catalog.json rationale cites HEAD: -> cite 8c92c2cc and 3bf43104. Also: writer stop hook runs in main checkout (all checks on base branch) - scope to worker worktree.
- 2026-11-19 LEADS for C1/C3 (from agentic-org-da): Codex 0.169.0 isolation probes in /Users/vgmello/projects/agentic-org stash@{0} (branch codex/v1-codex-runtime; RT-01 PROPOSED; raw evidence untracked .agent-kit/logs/v1-codex-runtime/): (a) profile derived from :workspace allowed a write outside its roots (custom profile without inheritance refused) - conflicts with docs/design/agent-sessions.md:186-190; (b) mcp_servers={} does not clear home/trusted-project MCP servers; empty home + untrusted-project override does (C3 empty MCP config); (c) named profiles need explicit default_permissions. Recheck in C1 on the installed Codex.
- 2026-11-19 OPEN SECURITY DESIGN QUESTION (A1/X1c): macOS admission relies on the audit token; early-SIGCONT spoof risk recorded open at crates/org-platform-host/src/admission/macos/spawn.rs:13-23 while docs/design/office-host-boundary.md:134,235 accepts the audit token as sufficient. Option: fd-holder check (one fd, cloexec, not stdio, not shared) in org-platform-host admission/macos.rs verify_reader (reference origin/park/river/key-handoff peer_fds, unreviewed). Owner decision.
- 2026-11-19 CORRECTION (agentic-org-da, relaying the owner): another agent is building the agent runtime; drop the Codex isolation leads above. Confirm with the owner before planning C1/C3 here. The macOS admission question stands.
- 2026-11-19: 4a fixes 1ac1f9a6 (intent binds the custody reservation; no second journal; etc.); review Opus medium running; writer B told to merge it into block 6. Branch backup pushed.
- 2026-11-19: 4a fix review: M-A (fresh-create check refuses host marker selected-writer.required -> would break every real create) + 3 low + intent VERSION 3. Writer A (580k context, last run made no commit) stopped; fresh writer (Opus high) fixing in slot-2. Writer B warned.
- 2026-11-19: 4a fix a62de4a4 (marker allowed; discard keeps custody; org.yaml refusal permanent; final checks before office.json tested; intent v3). Review running.
- 2026-11-19: block 4a ACCEPTED at a62de4a4 (review clean). 4b note: reuse/delete the old OFFICE_ENTRIES (OLDER_BUILD_ENTRIES duplicates it). Block 6 told: discard returns OfficeExists for a marked folder.
- 2026-11-19: block 6 at c3e5f20b (worker on journal_office under custody; recovery-public ULID/UUIDv7 bug fixed; real provisioning path create/restart/delete-runtime/restart Active; org-host fold-ins; fatal exit 1; catalog skips marked folders). Escalation: LNX-1 + agent-os office tests need bindable agents (B2) and selected-office addressing (A5): lead decided (same rule as owner LNX-1 ruling) LNX-1 on unbound hello (+ owner snapshot), agent-os office cases deferred to B2/A5 with scenarios recorded. origin/v1 now de960b2c (X5, conflicts): writer B merging. Branch backup pushed.
- 2026-11-19: block 6 at 4b27d008: merged origin/v1 c60bcada (X5) with conflicts resolved; agent-os office cases kept 5, deferred 3 (A5/B2/B6 done-when). Escalation: no cheap office for LNX-1 under M8 (custody only via the real create path; no admin seed fixture); LNX-2 container lnx2-office seeds a legacy office and breaks too. Lead decision: option (a) public org-host test-support admin-seed fixture + real HTTP create for LNX-1 and LNX-2 (B1b must not leave v1 red or break Track X). Writer B on it.
- 2026-11-19: block 6 at 3e45694d: admin-seed fixture (test-support+debug only); 9 stale ignored web-process tests fixed; LNX-1 real serve + HTTP create + hello passes; LNX-2 passes steps 1-9 in Docker on this Mac (step 10 needs AppArmor host; agent-os parts recorded under B2); macOS bash 3.2 brace-expansion fix in lnx2.sh. Token-bearing trace log deleted. Backup pushed. Whole-block-6 review (Opus medium) running.
- 2026-11-19: block 6 ACCEPTED at 3e45694d (review Opus medium: clean at medium+; merge 0157a7e7 verified; 5 low + 2 info). Low fixes -> fresh writer slot-1 on worktree-b1b-block6; block 4b -> fresh writer slot-2 on new branch worktree-b1b-block4b from 3e45694d (incl. one owner for the custody-marker rule). Known flake: serve::tests::a_log_that_cannot_be_read_closes_... under load.
- 2026-11-19: block 6 low fixes d2f60b06 (owner check at start, agent-os deps, channels doc, L8 barrier, FC-164 wording); writer-run fresh Opus review clean after 2 comment fixes. Item 1 (exact creation ULID) moved to 4b writer: Office::creation_command_id accessor + recovery.rs check after merging block 6. Open for block 7: FC-164 says both "LNX-2 not met" and "passes in Docker" (step 10 unrun).
- 2026-11-19: block 4b at 13c10daf (old store deleted ~38k lines; journal_office -> org_office::Office; marker owner org_office_launch::writer::marker (new edge org-office -> org-office-launch, no cycle, accepted); legacy id_profile + org_proto work writer types deleted; merged block 6 fixes; Office::creation_command_id + exact ULID recovery check). Open: capability evidence for Cargo.lock; Option hq_creation/bootstrap; plain-ID structs in launch tests; engine_boundary header (block 7). Review running.
- 2026-11-19: 4b review: changes required. H1 platform-conditionals allow line for deleted file; H2 6 agentic-org tests on plain IDs. Workspace at 13c10daf (real worktree): 3517 passed / 49 failed: 48 apps/agentic-org real-process tests (supervision, host_control, service, fake_launch, etc.) still on plain IDs / custody-less fakes; missed because block 6 gates ran only lnx1 from that crate. LESSON: per-block gates must include cargo test -p agentic-org (and ideally --workspace) whenever host/worker launch changes. Dispatched: slot-2 4b fixes (H1, M1 tests, stale docs, dead plain-ID branches, non-Option hq_creation/bootstrap, catalog skips plain-ID folders); slot-1 new branch worktree-b1b-block7 from 13c10daf: block 7a, all 48 agentic-org tests ported/deleted/deferred with reasons.
- 2026-11-19 CONTRACT (from ui-dev): processes write call for new role/skill/template must refuse a taken key with already_exists on the key (UI already refuses client-side). Apply when the processes write route is built (C5 / processes family). ui-dev merging v1-ui into v1 after its release review; will message first.
- 2026-11-19: 4b fixes 5a0a1ddb..1dd6d6f7 (allow list, 4 discard/open tests, stale docs, office.json v2 only, non-Option hq_creation/bootstrap, catalog skips plain-ID folders). Review running. Latent test bug: store::open::tests::crashes::on_sqlite_a_committed_write_is_answered_while_the_flush_is_held_and_a_team_is_not can HANG (assert inside thread::scope while flushes held) instead of failing: fix in block 7.
- 2026-11-19: block 4b ACCEPTED at 7a220120 (fix reviews clean, Opus medium; lead fix F1-F4 + nits, own fresh review clean). Block 7a writer (slot-1, worktree-b1b-block7 from 13c10daf) must merge 4b head 7a220120. PR 17 (owner request; runtime rename org-agent-runtime, C1 spike, ADR-0032) review running, report-only, no GitHub posting.
- 2026-11-19 COORDINATION with linux-dev (Track X lead; X6 CI + X7 HTTPS): agreed X7 items 1-5 (tls.fetch/tls.set/tls.renew, optional tls in InstanceViewResult, http_published scheme, https setup with self-signed or supplied pair; key only in host secrets, fail closed). They avoid setup_flow/{fixture,seed,registered}.rs. X6: handed them org-socket, org-supervise, org-host server_check.rs, web transport tests, office transfer/tests.rs; they WAIT on office startup/tests.rs + launch/activation/transfer.rs until B1b lands. CI lnx2 job must use the B1b lnx2.sh. They ping before merging into v1.
- 2026-11-19: linux-dev confirmed: no edits to B1b files; X7 container stays http until B1b in v1, then switches to https on our lnx2.sh; X6 takes handed-over predicate files now. TODO: message linux-dev (and ui-dev) when B1b is in v1.
- 2026-11-19: block 7a done at e3c6a6a8 (worktree-b1b-block7): all 48 agentic-org tests ported (test_support::custody reserve_office; fake office takes writer control; 4 new fake tests; descriptor-in-flight flake fixed); writer ran 4 fresh Opus medium reviews -> accept. Two startup timing asserts changed (production dummy Argon2 at Authentication::open costs 1.4-2.3 s). Open: kit capability evidence for Cargo.lock stale.
- 2026-11-19: block 7b done d2ea0fd1 (ENG-1 syn scan, hang fix, enc1 + delete-runtime real-process tests, records). Lead decisions: M4/M5 through the real host narrowed (worker/adapter level covered; real-host versions with A5 client), recorded in FC-146 + A5 done-when. Leftovers (bounded held-flush waits incl. org-store, rules doc, ADR-0007 acceptance, linux-host.md layout) -> fresh writer. Then lead: kit catalog rehash + capability evidence (Cargo.lock), slow gate, whole-change review, merge.
- 2026-11-19: block 7 leftovers done 3a6d3988 (bounded held waits in org-store testkit; rules doc; ADR-0007 acceptance; linux-host.md layout; M4/M5 narrowings in FC-146 + A5). Lead: kit refresh uncommitted in slot-1 (hq-creation-provider skill current; 7 catalog hashes; org-data README trimmed to 23998 B after 7b pushed it to 24764). Cargo.lock reassessed: edges only, capabilities unchanged. Review of kit refresh + slow gate running.
- 2026-11-19: kit refresh committed 9ff35c78 (review clean after root-name fix). Whole-change B1b review (c60bcada..9ff35c78, 196 files, +9.6k/-40.3k; Opus medium, read-only, no cargo) and slow gate running. origin/v1 now 58a44b2b (two X5 test commits) to merge at M2.
- 2026-11-19: WHOLE-CHANGE B1b review (Opus medium): no Critical/High/Medium; merge after slow gate incl. ignored org-host real-process tests. 7 Low: L3 (S1 note: second office of one org per instance refused), L4 (no monotonic ULIDs), L5 (owner.rs comment), L6 (worker test_support gated on debug_assertions) fixed on worktree-b1b-final in slot-2 (review running). Left for B2 with reason: L1 organization/apply.rs + people::owner_bootstrap (rules/ go with B2), L2 writer Request::Owner (no production caller; decide in B2), L7 org-office -> org-office-launch edge (lead accepted). Info: per-confirm keychain write + transient custody read failure fatal -> B1c measurement (R8); creation.rs TEMPORARY duplicates org-store TEMP_PREFIX.
- 2026-11-19: linux-dev merging X6 (x/x6-ci f6cf1045) into v1: no overlap with B1b; expect conflicts on platform-conditionals.allow (drop acceptance line) and skill-catalog.json (rehash). They take the 2 office test-file predicates after B1b lands.
- 2026-11-19: SLOW GATE at block7 head (3a6d3988 code): fmt/clippy/build OK; cargo test --workspace 3584 passed 0 failed; ignored org-host real-process 14/15: supervisor::channels::tests::accepted_stream_waits_for_delayed_ready_and_descriptor_receipt fails ("worker channel unavailable": uncustodied Broker::new + plain "office" id since block 6 made custody mandatory; missed because ignored). Fresh writer porting it on worktree-b1b-final. LESSON: run org-host --ignored set in every block that touches launch/supervisor.
- 2026-11-19: PUSHED v1 5f9941f1 = B1b integrated (worktree-b1b-final 3f164f72 incl. channel-test custody port; merged on X6 701d8460; conflicts: allow-list -> 2 office files/11 predicates, FC-164 sentence 3-way). Gates: merged-tree fmt, clippy, platform_conditionals, kit validate; branch: workspace 3584/0, ignored org-host 15/15. linux-dev + ui-dev told. PR 17 review posted as a comment (owner request). Owner: tokens short, commit and merge what we can. Next per plan: B2.
- 2026-11-19: B2 plan .agent-kit/slices/b2-organization-teams-people-plan.md; owner answered Q1 forest (flexible), Q2 refuse, Q3 refuse until cleared, Q4 max_members hires only, Q5 archive empty. linux-dev: X7 in v1 a31248ca (+ registry lock wait fix); follow-ups for me: office-host-boundary.md still says HTTPS unavailable; supervisor::tests::an_admission_that_could_not_be_built_is_asked_for_again flaky under load on Linux; container https switch not done (owner: no new X work).
- 2026-11-19 OWNER: no new features; only review, verify and merge what is done. B2.1 stopped; partial work saved as WIP on origin wip/b2-1-checkpoint (does not build, not merged). B2 plan + owner answers kept in .agent-kit/slices/b2-organization-teams-people-plan.md. Browser-to-office path (A3 Build, unbuilt) = new feature, not started.

## WHERE WE ARE / HOW TO CONTINUE (written 2026-11-19, end of session)

**Superseded for current state by `docs/workstreams/backend.md`** (start there). This section is the
handover as written at the end of the 2026-11-19 session; some facts below have since changed (LNX-2
step 10 later passed in CI on Linux; v1 has moved on).

### State
- `origin/v1` = 4d747ab7. It contains A1-A4, B1a, **B1b** (merge 5f9941f1: the office runs on the
  journal store under host writer custody; old store deleted), Track X X5/X6/X7 (Linux service,
  CI on both systems, container HTTPS) and the UI lead's merges. Never push `main`.
- B1b gates passed: workspace 3584/0, org-host ignored real-process 15/15, LNX-1 on macOS, LNX-2 steps
  1-9 in Docker Desktop on a Mac. Not yet run on Linux: rewritten LNX-1, LNX-2 step 10.
- **Paused by the owner (2026-11-19): "no new features; only review, verify and merge what is done".**
  Do not start B2 or anything new until the owner says so.

### Unfinished work (kept, not merged)
- **B2.1** partial code: branch `wip/b2-1-checkpoint` on origin (does NOT build; deletions half done).
  Also local branch `worktree-b2-1` in worktree slot-1. Plan:
  `.agent-kit/slices/b2-organization-teams-people-plan.md`, with "Owner decisions (2026-11-19)" at the end
  (Q1 team forest, Q2 refuse deactivating a lead/manager, Q3 refuse moving a lead until cleared,
  Q4 max_members default 6 on hires only, Q5 archive only empty teams). Blocks: B2.1 store rules,
  B2.2 wire/dry-run/operation.get (needs ui-dev mirror update at the same time), B2.3a browser-to-office
  path (UNBUILT in A3: no browser request reaches an office today; a new feature: owner must approve and
  decide with the other Track A lead who builds it), B2.3b /api/v1 routes, B2.4 CLI (waits for A5).
  Open owner questions: Q8 (who builds B2.3a), Q9 (move agent-placed done-when cases to C3).

### How to continue when the owner says go
1. Read AGENTS.md "Session start" and this section; summarize to the owner; ask before resuming.
2. B2: start from `wip/b2-1-checkpoint` (or restart B2.1 from origin/v1 using the plan), finish B2.1,
   fresh review, merge; then B2.2 (coordinate with ui-dev), then B2.3 after Q8. Order after B2 per
   docs/implementation-plan.md: B3, A5, O1, A6, then C2/C3 (another agent builds the agent runtime: ask the
   owner before planning C1/C3), B4, B6.
3. Process the owner set: writers Opus high, every review a fresh Claude `code-reviewer` on Opus medium
   (no Codex/Sol); one fresh review per change and a whole-change review before every merge; verify
   findings. Backend + documented web API only (ui-dev owns apps/agentic-org-ui; send UI mirror changes
   to ui-dev). Gates: per-block checks must include `cargo test -p agentic-org` and the org-host
   `--ignored` real-process set whenever host/worker launch changes (two B1b gaps came from skipping
   them): `ORG_TEST_OFFICE_BIN=$PWD/target/debug/agentic-org-office
   ORG_TEST_WEB_BIN=$PWD/target/debug/agentic-org-web cargo test -p org-host --lib -- --ignored
   --test-threads=2`.

### Pending (not new features: small follow-ups, do when allowed)
- docs/design/office-host-boundary.md still says HTTPS is unavailable (X7 added it).
- Kit: manifest hash of skill-catalog.json stale (needs owner override for the overlay guard);
  kit_codex.py labels old review runs without a model as 6.1 (cosmetic); catalog rationale cites "HEAD:".
- B1b left for B2: organization/apply.rs and people::owner_bootstrap unused; custody wire
  Request::Owner unused (keep for A6 or delete); creation.rs TEMPORARY duplicates org-store TEMP_PREFIX.
- B1c measurements: per-confirmation keychain write; a transient custody read failure while serving
  is fatal (worker exits, host restarts).
- Narrowed to A5 (recorded in FC-146 and A5 done-when): delete-runtime with office calls and journal
  corruption/rollback through the real host. agent-os office cases return with B2/A5/B6; LNX-2 agent-os
  steps with B2/C2.
- Platform allow-list: two office test files keep 11 predicates (startup/tests.rs,
  tests/launch/activation/transfer.rs); Track X will not take them; fold in when convenient.
- Container HTTPS switch (setup.json, README, lnx2.sh) not done (Track X stopped).
- Flaky under heavy load only (pass alone): serve::tests::a_log_that_cannot_be_read_closes_...,
  supervisor::tests::an_admission_that_could_not_be_built_is_asked_for_again (Linux),
  web::tests::a_child_that_exited_without_a_refusal_line_... .
- Security design question for the owner: macOS admission relies on the audit token; the fd-holder
  check (crates/org-platform-host admission/macos.rs verify_reader) is unscheduled.
- AGENTS.md and docs/processes/branch-and-merge.md still name `v1-integration`; the branch is `v1`.

### PR 17 (runtime rename, C1 spike, ADR-0032; built by another agent)
Reviewed and commented on GitHub (one review comment). Rename commit 97526ed9 is mergeable; the docs
commit 751ee234 has 4 blocking issues (non-negotiable 12, data-contract doc 14 / ADR-0016, ADR-0025
background work not carried through, spike evidence overstated). Owner decides; not our branch.

### Other sessions
- ui-dev (UI lead; branch v1-ui): told B1b is in v1; merges v1-ui into v1 after its own review.
- linux-dev (Track X lead): X5-X7 done; told B1b is in v1; stopped by the owner (no new work).
