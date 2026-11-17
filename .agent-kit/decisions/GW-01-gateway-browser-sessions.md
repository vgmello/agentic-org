# GW-01: browser gateway sessions on the native host boundary

- Status: PARTLY SUPERSEDED (revision 3, architecture READY 2026-11-17 after focused recheck; r2 D1 clause superseded)
- Superseded in part (2026-11-17, owner architecture review, origin `af79d274`): [ADR-0013](../../docs/adr/0013-stored-browser-sessions.md)
  replaces premise P-1 (in-memory pair to host-session mapping) and the session lifetime rules: browser sessions become stored host
  records that survive web and host restarts, with a total and per-account cap instead of the per-connection cap (FC-159, Track A).
  Still in force: host supervision and admission of the web process, the sandbox-verified handoff, the release/bundle guards and the
  browser cookie/proof contract (ADR-0013 decision 2).
- Merged scope (2026-11-17, slice F4-CONFORM): only the architecture-stable part. The host supervises the web process
  (debug/test start only), and the web process serves only `GET /api/v1/version`; every other `/api/v1` path, including
  the four `/api/v1/session` routes, is a JSON 404. No browser sign-in until FC-159 stored sessions (ADR-0013). The
  connection-bound session layer (web `sessions`, `ops::GATEWAY_SESSIONS`, the web-connection session cap) was removed
  before integration and is preserved on the local branch `f4-full-reference`, so web fix `85f9adf8` is not merged.
  F4-added "gateway" names were renamed to "web" (wire method `web.serving`); pre-existing gateway names are held pending
  owner questions in `.agent-kit/logs/v1-architecture-conformance/owner-questions.md` (main checkout). Still open from
  final review 13: B1/B2 start-eligibility tests (an Active HQ without authentication; a non-Active HQ).
- Scope: F4 in `.agent-kit/tasks/v1-incremental-delivery.md`; contract `.agent-kit/tasks/v1-f4-gateway-sessions-contract.md`.
  Request: continue toward the complete v1 delivery (user, 2026-11-17).
- Architecture owner: Claude lead (hybrid profile, Opus).
- Source baseline: `e5152b4e` (F3c candidate). Reservoir `b77cbd68` is read-only provenance. Evidence date 2026-11-17.
- Extends `.agent-kit/tasks/v1-native-host-boundary.md`. Its N4 deferral, the private `gateway_binding`
  conversion and browser startup, now get their real consumer here. Does not supersede it.

## Intent and premises

The outcome is a real host-supervised `agentic-org-web` process serving version plus session
sign-in/GET/refresh/sign-out (web-api-v1 §2 rows 1-5, §4). It is internal until U1 adds the built UI.
Non-negotiable: §4 cookie/proof/origin rules, host-owned authority, no task-service import, no unsigned
production bypass, preservation of the CH-LIFE-1r2 budget/ACK. Changing these belongs to the owner via the spec.

| ID | Claim (kind) | Source | State | Consequence / recheck |
| --- | --- | --- | --- | --- |
| P-1 | The gateway holds only an in-memory pair→host-session mapping; the host owns accounts/expiry/revocation (requirement) | web-api-v1.md §4 | VERIFIED (spec) | Design core |
| P-2 | Host control ops AccountVerify/SessionRenew/Inspect/End/Grant and org_proto gateway session views exist unchanged at HEAD (fact) | closure-inventory-1 §absent; org-proto/src/gateway/session.rs:148 | VERIFIED by read-only inventory (Opus Explore) | If changed: adapt web to HEAD types, never revert host |
| P-3 | HEAD channel structures (ChannelSlot, MAX_GATEWAY_CHANNELS, receipt map) are newer than the reservoir and must be kept (fact) | closure-inventory-1; native-host-boundary N5 | VERIFIED (diff direction) | Taking reservoir bytes would regress CH-LIFE-1r2 |
| P-4 | Missing host closure: GatewayConnection (~40 lines), Service::gateway_ready, gateway.rs supervisor, lib composition, gateway_binding (+8), require_build_trust (+3). WEB_SIGNING_ID exists (server_check.rs:44); ServerCheck::Gateway and gateway_for_build exist but are #[cfg(test)] (server_check.rs:60-61,110-111): ungate, not import (fact) | closure-inventory-1 + r1 review F4, lead-verified | VERIFIED for presence; compile closure UNVERIFIED | Early compile is the first checkpoint |
| P-5 | Reservoir web sessions are coupled to selected-task types, and that coupling is separable for sign-in/refresh/sign-out (assumption) | sessions.rs:12-21, http.rs:24,35, control.rs:434-595 | UNVERIFIED | If inseparable: pause and return to the lead; never import selected tasks |
| P-6 | v1 browser gateway start is macOS + loopback HTTP, matching the reservoir gate gateway.rs:67 (assumption) | reservoir; product code-signing is macOS Developer ID (code_signing.rs) | UNVERIFIED as product scope | Documented limit; recheck at U1/release |
| P-7 | Web needs hmac/sha2/zeroize (or equivalents already in the lockfile) (assumption) | closure-inventory-1 P-7 probe: hmac 0.12.1, zeroize 1.9.1, rustix 1.1.6 already in Cargo.lock | VERIFIED (lockfile); compile UNVERIFIED | No new third-party crate; each added internal crate edge is justified; org-client excluded unless proven needed |
| P-8 | The UI e2e fake (cookie value = proof) contradicts §4 (fact) | apps/agentic-org-ui e2e/fakes/session-server.ts:55 | VERIFIED by inventory | Fix the fake and adapters to the spec |

## Decision and alternatives

- D1 Composition: the host spawns the gateway through the existing spawn/cloexec/ServerCheck path, with
  `ServerCheck::Gateway`/`gateway_for_build` ungated from cfg(test) (debug→Skip, no team→Refuse, Keychain-forbidding
  admit kept, server_check.rs:261,277). It admits the gateway with `GatewayFirstLine` carrying `GatewayRelease` from
  `RuntimeRelease::gateway_binding`, and supervises it with `GatewayConnection`. `gateway_ready` requires HQ Active,
  authentication present AND an admitted, serving gateway, not persisted configuration (native-boundary condition 5).
  r3: F4 DOES create the transfer datagram pair, because the reviewed handoff wire requires exactly
  [Transfer, Listener] (org-host-control/src/gateway.rs:8-12,205-222) and HEAD `Gateway{transfer,listen}` (listener.rs:97-100) is
  what makes a connection a gateway connection. The web side receives and holds the transfer descriptor but sends no
  OfficeOpenChannel and opens no office channel in F4. Any OfficeOpenChannel an admitted gateway could send already goes
  through HEAD authorization (service/mod.rs:218-262: session authorize plus office linkage) and the unchanged CH-LIFE-1r2
  budget/receipt structures. (P-3, P-4)
- D2 Sessions: web `sessions` keeps an in-memory pair→handle map (idle 30 min / absolute 12 h, MAX_SESSIONS 1024)
  and calls host ops on its admitted control connection. The cookie and the proof are two separate 256-bit
  secure-random draws. HMAC-SHA256, keyed by a per-process random key, stores only a digest of the proof, which is
  compared in constant time. The proof is never derived from the cookie. This is the sole use of `hmac` (P-7).
  Office grant/open/cache and `Route::Selected` are removed because F4 has no consumer for them. CORRECTION (r1 F2):
  U1 workspace entry needs canonical `org.snapshot`, which is a worker read over a `ChannelKind::Gateway` office
  channel (listener.rs:579-586, 783-794), not a host op. **U1 prerequisite:** a task-free office-channel open on
  the existing CH-LIFE gateway budget, owned by U1. (P-1, P-2, P-5)
- D2a Limits: sign-in body bound per the existing web body limits/BODY_READ_TIMEOUT. If host `account.verify` has
  no attempt throttling at HEAD, record a sign-in throttling deferral to B1-06 in FEATURES (no silent omission).
- D3 The minimal `GatewayRelease` contract and validating wire are owned by F4, as its first consumer. The
  unaccepted ace8b29d is preserved and not imported.
- D4 Platform: keep the reservoir macOS+loopback gate for v1 development/U1, and document it. Release builds
  without a team id refuse; debug skips the signature check only. (P-6)
- D5 Ownership: one fullstack Opus High writer owns host+web+UI-adapter end to end in slot-1. No parallel UI
  writer: the UI part is small and shares the wire, so a split costs more than it saves.
- Rejected: (a) whole reservoir import, which regresses CH-LIFE and pulls in tasks; (b) a fake gateway or
  in-process web for proof, which violates early production-path proof; (c) a task-free OfficeScope type now,
  which is design without a consumer.
- Limits: a gateway restart loses sessions (by spec). Physical authentication, REC-1 and authentic signing
  remain separate later gates.
- Falsification checks: an early all-target compile of host+web; a real-process test (exact full name recorded, run
  with `--ignored --exact` on macOS, evidence of exactly 1 passed; a zero-test run is a failure) where the host spawns
  the web binary and completes sign-in→GET→refresh→sign-out, and control EOF ends the sessions, in a temp directory;
  refusal tests per contract, plus: a valid cookie with another session's proof is refused (r1 F1); `gateway_ready`
  stays false when gateway start is refused (F7); a release-build no-team gateway check refuses (F4); a unit test of
  the cookie-attribute builder including Secure for HTTPS, which the loopback gate makes unreachable end to end (F6);
  `cargo tree -p agentic-org-web -e normal` contains no org-host/org-office/org-work/org-client (F5).
- Channel-lifecycle tests in F4 are narrowed to what F4 exercises: control-connection admission, EOF and closure.
  Gateway channel capacity/ACK stays covered by the existing HEAD tests (listener.rs:861, 1962) until U1 opens office
  channels (r1 F3).
- Web dependency edges (r1 F5): each new edge must name the F4 symbol that uses it, and edges without one are dropped.
  org-office-launch is expected to be droppable. Pins must match Cargo.lock under --locked.

## Architecture review

- r1: fresh Opus High code-reviewer (claude-opus-5-5, effort high requested via the harness), PLAN REVIEW
  ARCHITECTURE, base e5152b4e: CHANGES_REQUIRED, F1-F8 (P1: proof derivation, U1 office-channel claim; P2: channel-budget
  consumer, stale P-4, dependency edges, zero-test risk/Secure; P3: gateway_ready condition, sign-in limits). The first
  reviewer attempt hit its 16-turn limit with no report. The lead verified F1 (reservoir sessions.rs:97-200, independent
  draws with an HMAC digest), F2 (web-api-v1.md:57) and F4 (server_check.rs:44,60,110). All findings are resolved in r2.
  Still UNVERIFIED and checked at the writer's CHECKPOINT C: P-5 separability, HEAD control-EOF ending all sessions, and
  whether `session.renew` permits an atomic pair swap.
- r2: fresh Opus High focused recheck (claude-opus-5-5): READY. F1-F8 RESOLVED; no new blocking issues. Conditions applied by the lead:
  - C1: U1 prerequisite added to the U1 row of v1-incremental-delivery.md.
  - C2: D2a added to contract Amendment 1.
  - C3: ungating also covers the `ServerCheck::Gateway` admit arm (server_check.rs:260) and `gateway_without_keychain` (:276).
    Production macOS keeps the Keychain refusal and non-macOS refuses; no weakened stub.
  Suggested extra test: `gateway_ready` turns false after the gateway exits or hits EOF (adopted).
- Lead evidence (r2, read-only), reducing the UNVERIFIED list:
  - Host sessions are connection-bound: `Session.connection: Weak<AtomicBool>` (authentication.rs:84), authorize refuses on a
    connection mismatch (authentication.rs:457), and a reaper thread runs every 100 ms (authentication.rs:60-62).
    So EOF ending sessions is a HEAD host property; F4 still proves it end to end.
  - Host `SessionRenew` only authorizes/touches the handle and returns Empty (service/mod.rs:166-170). Browser-pair rotation
    is therefore a gateway-local in-memory swap after a successful renew; the host handle is unchanged. Atomicity is the
    gateway map's responsibility and is tested in web.

## Challenges and replacements
- 2026-11-17, base f8a370ed, writer 2 (F4 continuation, Opus High): D1 "F4 creates no transfer datagram" contradicts the
  reviewed wire and HEAD structures (evidence above; lead verified gateway.rs:6-12, listener.rs:96-100). Classification: an
  architecture-text error in r2, not an implementation defect. No source was written.
  - Retained: P-1..P-8, D2-D5 and all falsification checks.
  - Disproved: only the r2 D1 no-datagram clause.
  - Rejected option B (listener-only wire + optional transfer): it changes a public reviewed wire and HEAD listener
    structures, which U1 would then restore. That is a larger blast radius for no security gain.
  - Option A adds no new authorization path. It is the first production reach of the existing gateway-kind path
    (gateway-kind channel delivery listener.rs:671; Socket::Control sign-in lockout bucket authentication.rs:335-339), still
    bounded by HEAD authorization and CH-LIFE (16 pending/512 total, listener.rs:653-665).
  - Added checks: web decodes exactly the two descriptors; the F4 web code sends no OfficeOpenChannel (grep/test); the
    reservoir `gateway_ready` (stored state only) and its `authenticate` not_authorized→Unavailable hunk are NOT taken.
  - r3 focused recheck (fresh Opus High, claude-opus-5-5): READY. Conditions adopted:
    - C-r3-1: tests that only the ServerCheck-admitted GatewayConnection sets `gateway` (host.sock and setup connections stay
      Local, listener.rs:102-103), that the host shuts down the write side of the gateway's transfer end before handoff
      (gateway.rs:209), and that the handoff sends exactly GATEWAY_HANDOFF_FDS in order.
    - C-r3-2: D2a is amended. HEAD already throttles with per-socket `login_lockout` (authentication.rs:340-351) under Socket::Control,
      so this is recorded as existing rather than a B1-06 deferral, with one lockout test through the gateway.
  - CHECKPOINT B resumes against r3.
- 2026-11-17, Sol review run 6e6241e4 round 1 (BLOCKED before the fix phase: the runner rejected absolute finding paths, a known kit limitation):
  - R1 P2 (a regression from the lead's reap fix 00c93089): synchronous host.end inside reap on the async routing path can block an HTTP
    worker for up to 5 s per session. Decision: reap only removes mappings, and host cleanup is queued to ONE background thread via a
    bounded channel (try_send; when full, drop and the host's own expiry covers it).
  - R2 P2: Running was published before web HTTP initialization (process.rs:125-180), contradicting D1 / native-boundary condition 5.
    **Revision 4 (additive wire):** after HTTP serving is established, the web sends one `gateway.serving` notification on the control
    connection. The host keeps Starting until it arrives within a bounded deadline (10 s); otherwise Failed and the child is stopped. A
    duplicate or out-of-order notification is refused. Tests: Running only after serving; a missing notification leads to Failed after the
    deadline, with readiness false; real-process test unchanged (it still waits for gateway_ready).
  - Status: ACTIVE revision 4 (lead decision, narrow additive change inside the r3 reviewed design; the fresh final review covers it).
  - Final-review B1 (2026-11-17): host global cap stays 1024 with LOCAL_SESSION_RESERVE 32 (one Local connection's cap); the gateway connection budget is `org_host_control::ops::GATEWAY_SESSIONS` = 992, and the web map MAX_SESSIONS (D2's 1024) is now that same constant, 992.
