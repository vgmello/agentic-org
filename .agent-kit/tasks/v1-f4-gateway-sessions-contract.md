# v1 F4 — gateway browser sessions (stable contract)

Registered 2026-11-17 by the Claude lead (hybrid profile). Request: continue toward the complete v1
delivery; F4 is the next item in `.agent-kit/tasks/v1-incremental-delivery.md` after F3c, and a
prerequisite of U1 (first usable onboarding/sign-in/create/reopen).

## Base and dependency
- Branch `worktree-f4-sessions`, base `e5152b4e` (F3c candidate, codex/v1-protected-host-creation).
- F3c is not yet accepted. F4 rebases onto the accepted F3c merge in `v1-integration` before its own
  review. Any F3c review fix that touches F4-owned files is reconciled at rebase, keeping F3c semantics.
- F4 merges independently after F3c and never ahead of it.

## Outcome
Internal foundation until U1. A real `agentic-org-web` gateway process is launched and supervised by the
host composed in `agentic-org serve`. It is admitted through the accepted native host boundary
(`v1-native-host-boundary.md`, CH-LIFE-1r2 channel budget/ACK). It serves, per
`docs/design/gateway-api-v1.md` §2 rows 1-5 and §4:
- `GET /api/v1/version`
- `POST /api/v1/session/sign-in`, `GET /api/v1/session`, `POST /api/v1/session/refresh`, `POST /api/v1/session/sign-out`

The host owns accounts, password verification, expiry and revocation. The gateway keeps only the in-memory
cookie/proof → host session-handle mapping on its admitted control connection. It has no DB or DB key.

## Acceptance criteria
1. Cookie and proof are independent 256-bit secure-random values. The cookie name is instance-specific; it is
   HttpOnly, SameSite=Strict, Path=/api/v1, has no Domain, and is Secure on HTTPS. The proof is 64 lowercase
   hex digits in `X-Org-Session-Proof`. Cookie-only and proof-only requests are refused, and nothing is ever
   recovered from the cookie alone.
2. Host/Origin are checked against configuration. There is no wildcard credentialed CORS and no secret in
   URL/query/path.
3. Unknown login and wrong password give the identical `not_authorized` response shape. Passwords are
   accepted only by sign-in and are held in redacting/zeroizing types, never logged.
4. Refresh rotates both browser values atomically through host `session.renew`. Sign-out ends the host
   session and clears the pair.
5. Logout, expiry, revocation and control EOF all invalidate the pair; EOF ends every host session on that
   connection. After a gateway restart, re-authentication is required. An unknown pair gets 401
   `not_authorized` with `session_reason: session_ended`, unless trusted evidence supports `hq_restarted`.
6. The gateway channel uses the existing CH-LIFE-1r2 capacity/ACK/closure accounting. HEAD's newer native
   structures are kept; reservoir bytes are never taken for files HEAD supersedes.
7. No selected-task/task-service code, office grant/open/cache, or task capability. Session routing is
   extracted from the selected-task dispatch.
8. Release builds without a signing team refuse gateway start. There is no unsigned production bypass. Debug
   skip is for development builds only.
9. Tests cover: capacity/ACK/closure, logout/revocation/expiry/EOF, cookie/origin/CSRF refusals, foreign
   account, endpoint failure and gateway restart. Early production-path proof: the real host spawns and
   admits the real web gateway over the actual handoff and completes sign-in → session → refresh → sign-out,
   using a temporary directory only.
10. The UI platform auth adapters and the e2e session fake are corrected to the cookie/proof wire above
    (the current fake sets cookie value = proof). There are no task-assignment UI changes.
11. Checks: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
    `cargo test --workspace`, `python3 .agent-kit/runtime/kit_hooks.py check` (UI check/test/build), and
    relevant browser checks for the UI adapter.
12. Review: Sol review/fix cycle, original-author intent validation and a separate fresh final review on the
    exact commit. Then `merge --no-ff` into `v1-integration` and push only that branch.
13. `docs/FEATURES.md` rows are updated with evidence. F4 alone does not complete any user-facing row.

## Sources
- Spec: `docs/design/gateway-api-v1.md` §2, §4. Plan rows F4/U1 in `v1-incremental-delivery.md`.
- Preserved reservoir `b77cbd68` (read-only provenance, not a grant).
- Lead inventory: `.agent-kit/logs/v1-f4-sessions/closure-inventory-1.md` in the delivery checkout.

## Amendment 1 (2026-11-17, from GW-01 r1 architecture review, before any implementation)
- AC6/AC9 channel tests are narrowed to what F4 exercises: gateway control-connection admission, EOF and closure.
  Gateway office-channel capacity/ACK stays covered by the existing HEAD tests until U1 adds the task-free
  office-channel open, which is now a recorded U1 prerequisite.
- Added tests: a cookie paired with another session's proof is refused; `gateway_ready` stays false when the gateway
  is refused; a release build with no signing team refuses; a cookie-attribute unit test including Secure; a web
  dependency-tree exclusion check; and the real-process test must show exactly 1 executed pass on macOS.
- Sign-in limits (GW-01 D2a): the request body is bounded by the existing web body limit and BODY_READ_TIMEOUT, with
  MAX_SESSIONS 1024. If host `account.verify` has no attempt throttling, FEATURES records an explicit deferral to B1-06.
- Production builds keep the Keychain-entitlement refusal: the `ServerCheck::Gateway` admit arm and `gateway_without_keychain`
  are ungated for production on macOS, and non-macOS refuses (GW-01 r2 C3).

## Amendment 2 (2026-11-17, GW-01 r3 challenge; before host implementation)
- F4 creates the gateway transfer datagram pair required by the reviewed [Transfer, Listener] handoff. Web holds it and sends
  no OfficeOpenChannel in F4.
- Added tests: only the admitted GatewayConnection is gateway-kind; the write side of the transfer end is shut down before
  handoff; handoff descriptors are exact and in order; web decodes exactly two descriptors; one sign-in lockout through the
  gateway (HEAD per-socket login_lockout). This replaces the conditional B1-06 throttle deferral.

## Amendment 3 (2026-11-17, from Sol review run cfde0832 R1; scope correction recorded by the contract owner)
- The Outcome sentence "composed by `agentic-org serve`" is corrected. F4 composes the gateway into the HOST (`Host` supervisor plus
  `startup_gateway`) and proves it through the test-support/bootstrap instance path (real host spawns the real web binary: the
  real-process test). Production `agentic-org serve` → `Host::start_instance` composition is owned by U1 (ON-01 D2, U1-A3; already
  implemented on worktree-u1-onboarding 7423b855/12dfe5db) because it requires the U1 setup/registry/launcher closure. F4 makes no
  claim that `agentic-org serve` starts a gateway; FEATURES FC-013 already states production launcher = U1.
