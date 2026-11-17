# GW-01: browser gateway sessions on the native host boundary

- Status: ACTIVE (revision 2, architecture READY 2026-11-17)
- Scope: F4 in `.agent-kit/tasks/v1-incremental-delivery.md`; contract `.agent-kit/tasks/v1-f4-gateway-sessions-contract.md`.
  Request: continue toward the complete v1 delivery (user, 2026-11-17).
- Architecture owner: Claude lead (hybrid profile, Opus).
- Source baseline: `e5152b4e` (F3c candidate). Reservoir `b77cbd68` is read-only provenance. Evidence date 2026-11-17.
- Extends `.agent-kit/tasks/v1-native-host-boundary.md`. Its N4 deferral, the private `gateway_binding`
  conversion and browser startup, now get their real consumer here. Does not supersede it.

## Intent and premises

The outcome is a real host-supervised `agentic-org-web` process serving version plus session
sign-in/GET/refresh/sign-out (gateway-api-v1 §2 rows 1-5, §4). It is internal until U1 adds the built UI.
Non-negotiable: §4 cookie/proof/origin rules, host-owned authority, no task-service import, no unsigned
production bypass, preservation of the CH-LIFE-1r2 budget/ACK. Changing these belongs to the owner via the spec.

| ID | Claim (kind) | Source | State | Consequence / recheck |
| --- | --- | --- | --- | --- |
| P-1 | The gateway holds only an in-memory pair→host-session mapping; the host owns accounts/expiry/revocation (requirement) | gateway-api-v1.md §4 | VERIFIED (spec) | Design core |
| P-2 | Host control ops AccountVerify/SessionRenew/Inspect/End/Grant and org_proto gateway session views exist unchanged at HEAD (fact) | closure-inventory-1 §absent; org-proto/src/gateway/session.rs:148 | VERIFIED by read-only inventory (Opus Explore) | If changed: adapt web to HEAD types, never revert host |
| P-3 | HEAD channel structures (ChannelSlot, MAX_GATEWAY_CHANNELS, receipt map) are newer than the reservoir and must be kept (fact) | closure-inventory-1; native-host-boundary N5 | VERIFIED (diff direction) | Taking reservoir bytes would regress CH-LIFE-1r2 |
| P-4 | Missing host closure: GatewayConnection (~40 lines), Service::gateway_ready, gateway.rs supervisor, lib composition, gateway_binding (+8), require_build_trust (+3). GATEWAY_SIGNING_ID exists (server_check.rs:44); ServerCheck::Gateway and gateway_for_build exist but are #[cfg(test)] (server_check.rs:60-61,110-111): ungate, not import (fact) | closure-inventory-1 + r1 review F4, lead-verified | VERIFIED for presence; compile closure UNVERIFIED | Early compile is the first checkpoint |
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
  F4 does not create the gateway transfer datagram; that waits for the U1 office-channel consumer. The CH-LIFE budget
  is untouched. (P-3, P-4)
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
  draws with an HMAC digest), F2 (gateway-api-v1.md:57) and F4 (server_check.rs:44,60,110). All findings are resolved in r2.
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
