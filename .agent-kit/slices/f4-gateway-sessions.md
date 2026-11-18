# Slice: f4-gateway-sessions (single end-to-end writer)

- Contract: `.agent-kit/tasks/v1-f4-gateway-sessions-contract.md`. Decision: `.agent-kit/decisions/GW-01-gateway-browser-sessions.md`
  (implement only against an architecture READY revision recorded there).
- Worktree: `/Users/vgmello/projects/agentic-org-claude-worktrees/slot-1`, branch `worktree-f4-sessions`, base `e5152b4e`.
  Reservoir provenance `b77cbd68` (read with `git show b77cbd68:<path>`; never checkout/merge/cherry-pick it whole).
- Model/effort: fullstack-engineer, Opus High (`.agent-kit/logs/v1-f4-sessions/routing-1.json`).
- Skill packet: `.agent-kit/logs/skills/f4-gateway-sessions-writer/packet.json`. Preflight ack is written to `preflight.json` in the same folder.
- Independent delivery: follow `docs/architecture-rules.md#independent-delivery-and-dependency-boundaries` and
  `docs/independent-delivery.md`. Outcome: an internal foundation; usable delivery is U1.

## Owned paths (expected; report material deviations before writing outside them)
- crates/org-host-control/src/gateway.rs, src/gateway/release_tests.rs (new), src/tests.rs (`release: None` hunks), src/lib.rs (doc)
- crates/org-host/src/{gateway.rs (new), gateway/tests.rs (new), lib.rs, server_check.rs, release.rs, release/runtime.rs,
  service/listener.rs (GatewayConnection only), service/mod.rs (gateway_ready only), hostdb/accounts.rs (test fixtures only)}
- apps/agentic-org-web/{Cargo.toml, src/main.rs, src/service.rs, src/sessions.rs + sessions/{control,http,ingress,tests,control/tests}.rs, src/process.rs, src/release.rs}
- apps/agentic-org-ui/src/lib/platform/auth/*, src/lib/platform/http/client.ts, e2e/fakes/session-server.ts and the specs that assert cookie=proof
- docs/FEATURES.md (relevant rows: evidence only, nothing marked Done)
Never touch: task/selected code, org-client selected, installed ~/.local/bin/org-agent, user org roots, files outside this worktree.

## Ordered steps and checkpoints
1. Preflight: read the packet sources, write the preflight ack, run `kit_skills.py preflight`.
2. Contract layer: `GatewayRelease` + validating wire in org-host-control, with tests. CHECKPOINT A: `cargo test -p org-host-control`.
3. Host composition: server_check/release/runtime hunks, listener `GatewayConnection`, `gateway_ready`, `gateway.rs`
   supervisor, lib.rs startup. Keep the HEAD CH-LIFE-1r2 structures. CHECKPOINT B: `cargo clippy -p org-host --all-targets -D warnings`
   and `cargo test -p org-host` stay green.
4. Web sessions: port reservoir sessions/http/control/process/release with the selected-task coupling removed
   (no office grant/open/cache, no Route::Selected, no org-client unless justified to the lead). Session map, cookie/proof per §4,
   Origin/Host via the existing ingress. CHECKPOINT C: `cargo test -p agentic-org-web`.
   **Return to the lead at C** with a short receipt before step 5. This is the early checkpoint for premise P-5.
5. Production-path proof (plus the GW-01 r2 falsification tests): an ignored-by-default native fixture (follow the F3c pattern of `--ignored --exact` plus a real built binary)
   where the real host spawns the real `agentic-org-web` and completes sign-in → GET session → refresh → sign-out, and EOF ends the sessions.
   It uses temporary directories only. It must report 1 passed; zero tests is a failure.
6. Refusal tests (contract §9) and the UI adapter/fake correction with UI tests.
7. Full checks: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
   `python3 .agent-kit/runtime/kit_hooks.py check`. Commit on `worktree-f4-sessions` with a scoped message (no overlay paths).

## Stop and return to the lead (do not improvise)
- A premise (P-1..P-8) fails, task-service types or a new third-party crate become necessary, a HEAD native structure would need
  changing beyond the listed hunks, or a check fails twice for the same cause.

## Receipt
Changed paths, checkpoint commands with results and log paths, deviations, open questions, and proposed decision/memory updates
(do not edit DECISIONS.md or memory yourself).
