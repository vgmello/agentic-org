# A3 plan: the web process, registration and sign-in

Status: PLAN revision 2 (lead review of revision 2 pending). Author: A2 owner (Opus High).
Base: `origin/v1-integration` d179cbf3. Worktree: `agentic-org-claude-worktrees/slot-1`.
Untracked, lead-owned location. Review: `.agents/agent-kit/slices/a3-plan-review-1.md`.

## Revision 2 changelog (review 1 finding → change)

| # | Finding / ruling | Change in this revision |
| --- | --- | --- |
| 1 P1 | Reorder into an early vertical slice | New order: B1 (running) → B2 wire fix → B3 minimal store → B4 host slice ops → B5 web slice → B6 UI slice. B6 ends the first usable slice (register, sign in, `GET /session`, SIGN-9, sign out) on a real `serve --name` instance. Refresh/touch (B7), password/link/hand-over (B8) follow. The rename moves off the critical path to B9, in a window agreed with A1.6. |
| 2 P2 | Origin/Host/CSRF | B5 states the ingress rule (exact configured Host; Origin required on unsafe methods; `application/json` only; no CORS; `Sec-Fetch-Site` cross-site refused). It starts from what `ingress.rs:40-48` and `service.rs:242-271` already enforce, and tests each refusal. |
| 3 P2 | Lockout/throttle | B4 `session.sign_in` reuses the `account.verify` lockout core (`authentication.rs:320-390`, `hostdb/lockout.rs`) with `Socket::Control`. Refusals return 429 `rate_limited` + `retry_after_ms` (new `State::retry_after_ms`). B8 throttles the password checks in `account.set_password` and `hand_over` on the same key. Set-password redemption records `Outcome::Reset` (SIGN-32). |
| 4 P2 | Rotation vs tabs | B7 adopts the lead decision with one refinement (§Rotation rule). The refresher is the tab holding the existing `org-session:refresh` Web Lock (`session-controller.ts:223-289`), which writes the shared store and broadcasts `rotated`. The host keeps the previous cookie and proof digests valid for 30 s. **Refinement:** any mix of current and previous values *of the same session* is accepted in the grace window, not only the exact old pair. Rotation is a compare-and-swap on the current pair, so only one is in flight. |
| 5 P2 | SIGN coverage | §SIGN map lists every criterion as in, regression-only or deferred, with the FEATURES row. SIGN-7 and SIGN-27 added to B6. SIGN-24 is regression only. SIGN-33 and Linux SIGN-34 go to milestone 2 (FC-164). SIGN-11/12/13/16 are deferred to FC-015. SIGN-14/15 are in B6. |
| 6 P2 | Data A3 cannot create | SES-3 (removed access), SIGN-39 and SIGN-9 non-admin get host-level fixtures (`hostdb/accounts.rs:146` `change_authority_fixture`). Hand-over chooser source: `instance.administrator.candidates` returns accounts that share an organization with the administrator, never all accounts (SIGN-28). The browser parts are deferred to FC-153 by name. |
| 7 P2 | Username/password rules | §Credential rules: the host is authoritative and the UI mirrors it. Error codes map to SIGN-21 copy. Passwords travel in `SecretText`/`Password` zeroizing, redacting types end to end. |
| 8 P2 | SIGN-34 | B12 restated. The host presence check (`recovery/presence.rs:63-72`, `admin.rs:205`) is the only enforcement. The SSH env hint only orders the message. Test: SSH env cleared + no console presence → still refused, DB unchanged. |
| 9 P2 | Office files | B10 (was B8a) starts only after A1.5 merges and A1 hands over `serve/host/authority.rs`, `serve/host.rs`. |
| 10 P3 | B9 position | Web launch fold is now B13, last, after A1.6. The lead updates the A1 plan note. |
| 11 P3 | web-api-v1 §2 table | B5/B7/B8 each update their rows. `account.verify` → `session.sign_in`, `session.renew` → `session.rotate`, the candidates route is added, and the §4 CSRF rule text is added. |
| 12 P3 | Store contract details | B3 gives full signatures, error type, digest construction, time base, bound behavior and the return values used for pushes. |
| 13 P3 | Ended push vs validate ordering | B4/B5 ordering rule: the host writes replies and events on one ordered stream. The web applies both in one reader task. Cache inserts happen in that task, so an `ended` that follows a validate reply always wins. |
| 14 P3 | Existing dev instances | B3: tables are created with `IF NOT EXISTS` in one transaction at open. A present table whose columns differ refuses the open with a clear error, never a partial create. |
| D1-D10 | Agreed | D5 adds an ADR-0018 follow-up row for the rest of ENG-3. D7 records the work/messaging feed deferral. D8 is 4096, with the same `unavailable` body and expired records reaped before refusing. D10 moves SIGN-33 and Linux SIGN-34 out, and Linux fails closed. |

## Outcome

The person who ran setup opens the printed `/register#<secret>` link, chooses a username and a
password, and is signed in as the instance administrator. With no organization, the UI shows
SIGN-9's "No organizations yet" page with Create organization (A4 builds the create itself).
Sessions are durable host records (ADR-0013 as amended 2026-11-17: 30-day sliding idle, no
absolute lifetime, background refresh rotates cookie+proof keeping the session ID, step-up only
for password change and administrator hand-over, no per-account cap, restarts never sign out).
Requests for a person reach the office with a principal and a host-issued grant. The change
stream shows host and organization changes without a reload.

Done when (implementation plan §A3): registration works once and only once; SES-1..SES-8 pass
(SES-3 removed-access at host level, browser part in FC-153); every SIGN criterion marked "in" in
§SIGN map passes against a real instance, in a browser where it is a browser criterion.

## Code today (verified at d179cbf3)

- Host sessions are in memory with `ABSOLUTE` 12 h / `IDLE` 30 min / `MAX_SESSIONS` 1024 /
  `MAX_CONNECTION_SESSIONS` 32 (`crates/org-host/src/service/authentication.rs:27-31`). They end on
  control EOF (`:267-285`), and the grant lifetime comes from `ABSOLUTE` (`:588`).
- `account.verify` already applies the lockout: an ingress cap of 4096 bytes, `LoginKey` before Argon2, and
  `Socket::Control` for the gateway channel. Per-login and socket-wide (`""`) keys are checked
  before and recorded after, with a dummy verifier for unknown logins (`authentication.rs:320-390`). `State`
  exposes only `is_locked()` (`hostdb/lockout.rs:28-45`). The backoff starts after 6 failures, at 1 s, doubling,
  ≤ 900 s (`:122-133`). `Outcome::Reset` clears a key (`:21-26`).
- Login normalization: `normalize_login` = NFC → default case fold → NFC (`accounts/login.rs:5`).
  `LoginKey` allows 1..256 normalized bytes (`accounts/identity.rs:9-21`). No password length rule exists in code.
- Secrets: `org_proto::gateway::SecretText` (zeroizing Drop, no Clone; `crates/org-proto/src/gateway/session.rs:18`)
  and `org_host_control::Password` (`crates/org-host-control/src/lib.rs:75`).
- Web ingress: the Host must be exactly the configured authority (`apps/agentic-org-web/src/ingress.rs`,
  `accepts_target_and_host`). On `/api`, Origin must be a single exact match, and an absent Origin is allowed only on
  GET/HEAD (`ingress.rs:40-48`). The refusal is 403 `not_authorized` before routing (`service.rs:264-271`). No
  Content-Type check exists. Static responses never carry `access-control-allow-origin` (`static_ui.rs:371`).
- Control ops: `account.verify` returns a raw handle (`crates/org-host-control/src/ops.rs:40-58`).
  `session.inspect` carries `absolute_expires_at_ms` (`:70-84`). `session.grant` lasts 5 min (`:106-128`).
  `subscribe` takes `Empty` (`:808-815`). `LinkedOrg.member_id` (`:48-52`). `account.set_password` (`:697-711`).
- Browser wire: `SessionView`/`SessionEstablished` carry `absolute_expires_at` and
  `OrganizationLink.member_id` (`crates/org-proto/src/gateway/session.rs:153-183`). The UI reader
  requires both and accepts `hq_restarted` (`apps/agentic-org-ui/src/lib/platform/http/wire.ts:159,303-306`).
- UI session controller: one Web Lock `org-session:refresh` serializes sign-in, refresh and sign-out across
  tabs. The stored record holds `proof` + `rotated_at_ms`. A refresh is skipped if another tab rotated after the
  trigger, and a `BroadcastChannel` announces changes (`apps/agentic-org-ui/src/lib/platform/auth/session-controller.ts:223-300`,
  `session-record.ts:9,28`).
- Error codes `Conflict`, `RateLimited`, `NotAuthorized`, `Unavailable` exist (`crates/org-proto/src/gateway/outcome.rs:136-166`).
- Web process: `VersionApi` only (`apps/agentic-org-web/src/api.rs:15-60`). The test
  `the_web_process_sends_no_office_channel_request` is at `:174`, `HostControl` at `src/host.rs:78-300`, and the
  rendezvous lines at `crates/org-host-control/src/gateway.rs`.
- Early-error RST: `service.rs:292-296` returns the refusal before the body is read (B1 fixes this).
- Office host channel refuses `subscribe` (`apps/agentic-org-office/src/serve/host/operations.json:66`).
  4 tests parked: `apps/agentic-org-office/tests/launch.rs:800,813`, `tests/launch/inbox_hint.rs:4,15`.
- Admin reset: `reset_with(local, confirm)` refuses `!local` first (`crates/org-host/src/admin.rs:200-207`).
  `local` = audit-token console presence on macOS, `false` elsewhere (`crates/org-host/src/recovery/presence.rs:63-72`).
  The set-new-password branch answers `unavailable` (`admin.rs:217`). The link path is `/register#` (`:238-242`).
- Host DB: one schema, `SCHEMA_VERSION = 1`, no migration (`crates/org-host/src/hostdb.rs:30-31,261`).
  Link store contract `OneTimeLinks` (`crates/org-host/src/links.rs:230`) with `links/memory.rs` +
  `links/contract_tests.rs` is the pattern for ENG-3.

## Cross-cutting rules (owned by the block named)

### Ingress, Origin and CSRF rule (B5)

Applies to every `/api/v1` request, before routing and before reading the body. Every refusal is a
JSON error and goes through B1's bounded drain.

1. `Host` is a single header equal to the configured authority (exists; keep).
2. Safe methods (GET/HEAD): an `Origin` header, if present, is a single exact match. If absent, the proof binds
   the origin on protected routes (web-api §4). Exists; keep.
3. Unsafe methods (POST, and any other non-GET/HEAD): `Origin` is required and must be a single exact match.
   Otherwise the answer is 403 `not_authorized` "Request origin is not authorized." This exists and gets explicit tests per route class (public link/sign-in
   routes, protected routes).
4. New: if `Sec-Fetch-Site` is present and is not `same-origin` or `none`, the request gets 403 `not_authorized`
   (defence in depth; browsers that omit it rely on rule 3).
5. New: every POST requires `Content-Type: application/json` (an optional `charset=utf-8` and nothing else).
   Otherwise the answer is 415 `invalid_request` "Content-Type must be application/json.". This blocks form/`text/plain` CSRF
   even on public routes, which have no proof.
6. New: `OPTIONS` on `/api/v1` gets 405 `invalid_request` with `Allow: GET, HEAD, POST` and no `Access-Control-*`
   header. No response from the web process ever carries `Access-Control-Allow-*`.
7. Protected routes also need cookie + `X-Org-Session-Proof` (B5 session layer). A cookie without a proof
   is 401 and is never recovered.

Tests (`apps/agentic-org-web/src/ingress.rs` unit + one real-TCP test per row through `run_gateway`/`run_web`):
wrong Host; two Hosts; POST without Origin; POST with `Origin: null`; foreign Origin; two Origins;
`Sec-Fetch-Site: cross-site`; POST `text/plain`, `application/x-www-form-urlencoded`, multipart, missing
Content-Type; `OPTIONS` with `Access-Control-Request-Method`; GET with foreign Origin. Each test asserts the
status, the code, no `Access-Control-*` header, and that the host saw no call (host test counter).

### Credential rules (host authoritative; B4, mirrored in B6)

- Username (resolves spec A81; B4 records it in `docs/specs/server-installation-and-access-ux.md` A81):
  - Host check in `crates/org-host/src/accounts/identity.rs`: a new `Username::new(display)` → `LoginKey`.
  - Display text after NFC is 1–64 Unicode scalar values, with no leading or trailing whitespace.
  - No control (Cc/Cf), line or paragraph separators.
  - Uniqueness is on the existing `LoginKey` (NFC → case fold → NFC), enforced by a UNIQUE index.
  - Sign-in does not apply the format rule, so an invalid name is just an unknown login (SIGN-4 sameness). It
    keeps the existing ingress caps.
- Password: at least 12 Unicode scalar values after NFC and at most 1024 bytes. It is checked only when set
  (register, set-password link, change). Sign-in keeps the 4096-byte ingress cap only.
- Types: browser JSON → `SecretText` (decoded straight into the struct, never via `Value`) → web→host
  param `Password` → Argon2. `Debug` redacts. No password in any log, error message or journal entry.
  Test: a `tracing` capture over the register/sign-in/change flows contains no password bytes.
- Error codes (HTTP 400 `invalid_request` with `{field, rule}`; new optional fields on the error body in
  `org-proto` outcome, vector added). The UI copy table lives in `lib/platform/auth/credential-copy.ts`, mirrored
  by a vitest table test:

| `field` / `rule` | UI copy (SIGN-21 / §4a) |
| --- | --- |
| `login` / `required` (UI only, SIGN-3) | "Enter your username." |
| `password` / `required` (UI only) | "Enter your password." |
| `login` / `format` | "Use 1 to 64 characters, without spaces at the start or end." |
| `login` / `taken` | "That username is taken. Choose another." (only behind a valid registration link) |
| `password` / `too_short` | SIGN-21 rule copy: "Use at least 12 characters." |
| `password` / `mismatch` (UI only) | "The passwords don't match. Enter them again." (clears both) |
| 401 `not_authorized` on sign-in | "That username and password don't match. Try again." (SIGN-4) |
| 429 `rate_limited` | SIGN-6 copy with `retry_after_ms`, or "Wait a few minutes, then try again." |

### Rotation and multi-tab rule (B7; lead decision with one refinement)

- Refresher: the tab that acquires the existing Web Lock `org-session:refresh` (the "leader for this
  rotation"). Other tabs that wanted to refresh then read `rotated_at_ms` and skip (exists, `session-controller.ts:258`).
  The refresher writes the shared record and posts `{kind:'rotated'}` on the `BroadcastChannel`. Followers
  re-read the record and use the new proof from their next request. No SharedWorker is needed. Web Locks are
  already the coordination primitive, and a tab dying mid-rotation releases the lock.
- Host record keeps `current {cookie_digest, proof_digest}` and `previous {cookie_digest, proof_digest,
  valid_until_ms}`, with grace fixed at **30 s** (const, test-injectable clock).
- Validation accepts cookie ∈ {current, previous-in-grace} **and** proof ∈ {current, previous-in-grace}
  for the same session ID. Refinement over "old pair only": the browser's cookie jar switches to the new cookie
  as soon as the refresh response lands, before followers receive the broadcast. A follower request in that
  window carries new cookie + old proof. Accepting mixes within one session grants nothing new, because both values
  were issued to this browser for this session.
- Rotation is a compare-and-swap: `rotate(id, presented, new)` succeeds only if `presented` matches the
  **current** values. A refresh presented with previous values, or losing a concurrent race, gets **409 `conflict`**
  with no state change. The UI handles 409 by re-reading the record (adopting a newer `rotated_at_ms`) and never
  opens the sign-in dialog for it. So only one rotation is in flight per session.
- Grace ends early when the record is deleted (sign-out/expiry/password/access removal). A new rotation inside
  the grace window replaces `previous` with the values just retired.
- The web cache maps both cookie digests to the session until `valid_until_ms`.
- Tests:
  - Host store contract: previous pair OK at +29 s and 401 at +31 s; mixed pair OK in grace; two concurrent
    rotates → exactly one wins, the other `Conflict`; rotate with previous → `Conflict`; end during grace → both 401.
  - Web real process: an in-flight request held across a refresh succeeds.
  - Playwright: two pages in one context, refresh in A, then B's next request succeeds without a dialog and
    uses the new proof; kill A mid-rotation (close page) and B can refresh.

### Ordering of ended pushes and validation (B4 host, B5 web)

The host deletes the record, then writes `session.ended` on the same control connection. Replies and events share
one ordered writer per connection. The web's host reader task applies replies and events in arrival order and
inserts cache entries itself; the requester only awaits the inserted result. A validate reply written before the
delete is therefore always followed by the `ended` that removes it. A validate after the delete fails. Test: a
host-side hook delays delete+push between a validate and its reply; afterwards the cache does not hold the session and the
next request is 401.

## SIGN map (reconciled)

| Criterion | A3 status | Block / deferral row |
| --- | --- | --- |
| SIGN-1, 2, 3, 4, 7, 8, 9 (admin), 10, 18, 20, 21, 22, 27, 28, 29 | In | B6 (host parts B4, web B5) |
| SIGN-5 | In, host-level (no person records before A4): unknown login → identical body | B4 |
| SIGN-6 | In | B4 (host 429) + B6 |
| SIGN-9 non-admin copy | Component test with fixture view; real account in FC-153 | B6 / FC-153 |
| SIGN-14, 15 | In: dialog closes across tabs; both Not now bars (editable flag from a page draft registry API) | B6 |
| SIGN-11, 12, 13, 16 | Deferred (draft retention, "Not saved", focus restore, live resume; editable pages arrive with A4+) | FC-015 |
| SIGN-17 | Withdrawn; `hq_restarted` removed | B2 |
| SIGN-19 | In (uses the B6 draft registry; true only when a page registers unsaved text) | B6 |
| SIGN-23 | A4 (organization create) | FC-018 |
| SIGN-24 | Regression only (existing setup tests stay green) | FC-012 |
| SIGN-25, 35, 36, 43 | Deferred (invitations, remove access) | FC-153 |
| SIGN-26 | In | B8 |
| SIGN-30 | In for the macOS rows of §4a.2; Linux rows in FC-164 | B8/B12 |
| SIGN-31 | In | B6 |
| SIGN-32 | In (Mac) | B8 |
| SIGN-33, SIGN-34 Linux part | Milestone 2; Linux reset fails closed (`presence::local` = false) | FC-164 |
| SIGN-34 Mac part | In | B12 |
| SIGN-37 | Set-password link redemption in (via admin reset); "New sign-in link" on a person in FC-153 | B8 / FC-153 |
| SIGN-38 | Registration + set-password links in; approval links with SIGN-40 | B4/B8; new row (terminal approval) |
| SIGN-39 | Host op + host fixture test in; browser chooser shows "No other account can take over yet." until a second account exists | B8 / FC-153 |
| SIGN-40 | Deferred: terminal approval link | New FEATURES row added by B6's docs commit |
| SIGN-41, 42 | Withdrawn; regression check that no `account` command exists | B12 |

## Blocks (ordered; each independently mergeable)

Every block: Rust → `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`; UI → `npm run check/test/build`; always `python3 .agents/agent-kit/runtime/kit_hooks.py check`.
Tests use temp roots only (`ORG_TEST_*` composition), never a user's organization workspace.

### B1 · Bounded lingering close (foundation; in progress, first)
- Files: `apps/agentic-org-web/src/transport/server.rs`, `transport/server/tests*`, `src/service.rs`.
- Change: after an early error reply whose body is unread, drain up to 64 KiB (body limit) for ≤ 250 ms,
  then close. Otherwise half-close write and read-discard with the same bounds. Per-connection task only.
- Tests: real TCP loopback, split head/body to an unknown `/api/v1` path, 2000 iterations, 0 resets; 10 MiB
  declared body bounded by time; slow client cut at the bound.
- Accept: 0/2000 resets (baseline ~1.35%); transport/write_stall tests pass.

### B2 · Session wire fix, proto + control + UI together (foundation; 1 run)
- Files: `crates/org-proto/src/gateway/session.rs` (drop `absolute_expires_at`; `member_id`→`employee_id`),
  `crates/org-proto/test-vectors/v2/*session*`, `crates/org-host-control/src/ops.rs:48-52,70-84`,
  `crates/org-host/src/service/authentication.rs` (stop computing absolute), UI `wire.ts:159,303-306`
  (drop `hq_restarted`; `employee_id`), `lib/platform/auth/session-*.ts` + tests. `gateway_status`→`web_status` (D2).
- Wire (`SessionView`): `{instance_id, account:{id,login}, admin, organizations:[{org_id, employee_id,
  office_ids}], server_time_ms, idle_expires_at}`; `SessionEstablished` = view + `proof`. `session_reason` ∈ {`session_ended`}.
- Tests: vectors round-trip; UI check/test; readers ignore unknown fields (D3: no shim, no strict reject).
- Accept: proto, control and UI agree; FC-159 note (absolute lifetime removed).

### B3 · Minimal host store behind contracts (foundation; 1-2 runs)
- Files (new): `crates/org-host/src/sessions.rs` (consumer-owned trait `BrowserSessions`, records, shared
  rules as provided methods), `sessions/memory.rs`, `sessions/contract_tests.rs`, `hostdb/sessions.rs` (SQLite),
  `hostdb/schema.rs` (tables). Accounts: `AccountStore` contract over `hostdb/accounts.rs`, `administrator.rs`,
  `lockout.rs`, limited to what the slice needs (register admin from link, credentials lookup, lockout), with memory + contract suite.
- Table `browser_sessions(id TEXT PK uuidv7, account_id TEXT NOT NULL REFERENCES accounts ON DELETE CASCADE,
  created_ms, last_activity_ms, cookie_digest BLOB NOT NULL UNIQUE, proof_digest BLOB NOT NULL,
  prev_cookie_digest BLOB UNIQUE, prev_proof_digest BLOB, prev_valid_until_ms INTEGER, scheme, host, port, instance_id)`.
  The prev columns are created here and used from B7.
- Digests: `SHA-256("agentic-org/session-cookie/v1" ‖ value)` and `…/session-proof/v1` (domain separated,
  computed in the web process; the host never sees raw values). Comparison is constant time on lookup by digest.
- Time: host wall ms from `hostdb/clock.rs` (`now`/`advance_clock`, monotone), injectable in tests.
- Contract (all `Result<_, StoreError{Unavailable}>`; storage errors are never a "valid" answer):
  - `create(record, now) -> Result<Created | Full>`: host-wide bound **4096** (D8). Reap expired records in the
    same transaction before refusing. `Full` maps to the same `unavailable` body for every account (SES-6).
  - `find(cookie_d, proof_d, origin, now) -> Option<SessionRecord>`: idle ≤ 30 d from stored `last_activity_ms`.
    Origin binding must match. Grace matching (B7) lives behind the same call.
  - `touch(&[(id, ms)])`: monotone max, unknown IDs ignored.
  - `rotate(id, presented, new, now) -> Rotated | Conflict` (B7; one transaction).
  - `end(id) -> Option<Ended{id, account_id}>`, `end_account(account, except: Option<id>) -> Vec<Ended>`
    (returned IDs drive pushes).
  - `reap(now) -> Vec<Ended>`.
- Existing dev instances (P3 14): at open, in one IMMEDIATE transaction, `CREATE TABLE/INDEX IF NOT EXISTS`.
  Then compare `PRAGMA table_info` to the expected columns. A mismatch rolls back and refuses to open with
  "host database from an earlier development build; run `agentic-org setup` on a new instance". The schema is never
  half-created. `SCHEMA_VERSION` stays 1 (D4).
- Tests: one suite against memory and SQLite (temp dirs). SES-4: scan a DB copy for any raw 32-byte value
  used in the test, and show that a copied DB yields no usable cookie. Idempotent open twice. A pre-created mismatching
  table → refusal and no other table created.
- Accept: no SQL/driver type outside `hostdb/` (ENG-1 grep); both adapters pass; nothing wired.

### B4 · Host slice ops: register, sign in, validate, end (foundation; 2 runs)
- Files: `crates/org-host-control/src/ops.rs` (+ `tests.rs` method table), `crates/org-host/src/service/authentication.rs`
  (split: native connection-bound sessions stay, SES-8; browser sessions use B3; the lockout core is extracted to
  `fn check_password(socket, key, password) -> Result<AccountId, Refusal{Denied|Limited{retry_after_ms}}>`),
  `service/mod.rs` dispatch (web-kind connection only), `hostdb/lockout.rs` (`State::retry_after_ms()`:
  max of the per-login and socket-wide `until - now`), `accounts/identity.rs` (`Username`, password rule), `links.rs`.
- Wire (web→host; params carry digests, never raw cookie/proof; `origin={scheme,host,port}`; passwords `Password`):
  - `link.register {link, login, password, cookie_digest, proof_digest, origin}` → `SessionRecord`. The setup
    link's subject is the administrator. Exactly one admin (DB constraint). Invalid/used/expired link → `not_found`.
    Rule failures → `invalid_request{field,rule}`.
  - `session.sign_in {login, password, cookie_digest, proof_digest, origin}` → `SessionRecord`. A locked key gives
    `rate_limited{retry_after_ms}` (no Argon2 work). Unknown login and wrong password give one identical `not_authorized`.
  - `session.validate {cookie_digest, proof_digest, origin}` → `SessionRecord` (SES-5: one call per unknown cookie).
  - `session.end {session_id}` → `{}`; deletes, then pushes `session.ended {session_id}`.
  - `SessionRecord = {session_id, account:{id,login}, admin, orgs:[{org_id, employee_id, office_ids}],
    server_time_ms, idle_expires_at_ms}`.
  - Host→web events: `session.ended {session_id}`, `session.changed {session_id, record}` (ADR-0013 §5).
- Remove for browser sessions: `ABSOLUTE`, per-connection cap, ending in `disconnect` (`:267`). `MAX_SESSIONS` becomes 4096 host-wide.
- Tests (unit on memory store + injected clock; real host in `crates/org-host/src/web_process_tests.rs` style):
  - Link redeemed twice → second `not_found`.
  - SIGN-4/5: byte-identical body for unknown vs wrong password.
  - Lockout: the 7th failure → `rate_limited` with `retry_after_ms` > 0, the same for an unknown login; the socket-wide key works.
  - SES-6: bound full → `unavailable` (same body for every account; expired records reaped first).
  - SES-7: idle 30 d via injected clock.
  - SES-8: native session still ends on EOF.
  - SES-3 host part: end, and `change_authority_fixture` revocation ends the account's sessions + pushes.
  - The ordering test from §Ordering.
- Accept: SES-4/6/7/8 host-side; FC-014/FC-159 rows updated with evidence.

### B5 · Web slice: ingress rule, session layer, slice routes (usable over HTTP; 2 runs)
- Files: `apps/agentic-org-web/src/ingress.rs` (§Ingress rule), `src/session/{mod.rs,cache.rs,cookie.rs}` (new),
  `src/api.rs` (real backend replaces `VersionApi`), `src/host.rs` (new calls; one reader task applying
  replies and `session.ended/changed` in order; connection generation), `src/process.rs`.
  Docs: `docs/design/web-api-v1.md` §2 rows (sign-in → host `session.sign_in`), §4 (ingress rule text),
  "What the code does today".
- Routes: `POST /session/sign-in`, `POST /links/register`, `GET /session`, `POST /session/sign-out`,
  `GET /instance`, `GET /organizations` (host `organization.list`; empty list now).
- Cache: key = cookie digest. An entry is valid only for the control-connection generation that validated it. An unknown
  cookie gets one `session.validate`. `session.ended` drops the entry. Cookie per web-api §4 (instance-named,
  HttpOnly, SameSite=Strict, Path=/api/v1, `Secure` over HTTPS). The proof is 64 lowercase hex.
- Tests:
  - Ingress tests from §Ingress.
  - Real process (temp homes): register → cookie+proof.
  - SES-1: kill web, next request 200 with no sign-in. SES-2: restart host on the same home.
  - SES-3: sign-out → 401 with a warm cache.
  - SES-5: host test counter shows one validate per cookie, and a reconnect revalidates once.
  - Cookie without proof → 401.
- Accept: SES-1, 2, 3 (sign-out), 5 on a real instance. Retire `the_web_process_sends_no_office_channel_request`.

### B6 · UI slice: register, sign in, empty state, sign out, dialog (first usable slice; 2 runs)
- Files: `apps/agentic-org-ui/src/routes/(onboarding)/{register,sign-in}/` (new), `routes/(onboarding)/welcome`
  (SIGN-9), `src/lib/platform/auth/*` (+ `credential-copy.ts`, a draft registry `hasUnsavedText()` API), the
  shell account menu (Signed in as, Administrator, Sign out; Change password lands in B8), the "Sign in again" dialog
  and Not now bars. The fragment secret is read from `location.hash`, sent in the JSON body, and the hash is cleared via
  `history.replaceState`.
  Docs: FEATURES FC-014/FC-015/FC-155 rows. New rows: SIGN-40 terminal approval, ENG-3 remainder (ADR-0018,
  D5: journal, recovery, provisioning, TLS stores). FC-015 lists SIGN-11/12/13/16 deferred. FC-013 records D7
  (work/messaging feeds with U1/C).
- Tests: vitest units (copy table, validation mirrors host rules, registry). Playwright against a real
  `serve --name` instance under a temp root: SIGN-1, 2, 3, 4, 6 (lockout via 7 failures), 7 (stop the host after load),
  8, 9 (admin), 10, 14, 15, 18, 19, 20, 21, 22 (link from setup output), 27 (axe + focus order), 28, 29, 31.
  Screenshots kept as UI evidence.
- Accept: **first usable slice**: a person registers from setup's link and signs in and out in a browser on a
  real instance. Lead may start A4 here (it needs only a signed-in admin + B5's host path).

### B7 · Refresh, rotation grace and activity (2 runs)
- Files: `crates/org-host/src/sessions*` + `hostdb/sessions.rs` (rotate/grace), `ops.rs` (`session.rotate`,
  `session.touch`), `authentication.rs`, web `src/session/activity.rs` (flush ≤ 5 min, D9), `api.rs`
  (`POST /session/refresh`), UI `session-controller.ts` (409 handling, broadcast), docs web-api §2 refresh row
  (`session.renew` → `session.rotate`).
- Wire: `session.rotate {session_id, presented:{cookie_digest,proof_digest}, new:{…}}` → `SessionRecord` | `conflict`;
  `session.touch {sessions:[{session_id, last_activity_ms}]}` → `{}`. HTTP refresh: `{}` → view + proof; 409 `conflict`.
- Tests: §Rotation tests; SES-7 with activity flush across a web restart (stored activity ≤ 5 min stale).
- Accept: SES-7 complete; refresh keeps the session ID; multi-tab Playwright passes.

### B8 · Password change, set-password link, hand-over (2 runs)
- Files: `ops.rs` (`link.set_password`, `account.set_password` web variant `{session_id, current, new}`,
  `instance.administrator.candidates {session_id}`, `instance.administrator.hand_over {session_id, account_id, password}`),
  `crates/org-host/src/admin.rs:217` (set-new-password branch: `LinkKind::SetPassword`, URL `/set-password#<secret>`),
  `links.rs` kinds, web routes `POST /links/set-password`, `POST /session/password`, `GET /instance/administrator/candidates`,
  `POST /instance/administrator`, UI `routes/(onboarding)/set-password/`, Change password dialog, Settings › Instance ›
  Administrator. Docs: web-api §2 (candidates row).
- Rules:
  - Current-password checks in change/hand-over go through `check_password` (same lockout keys, 429).
  - Set-password redemption sets the password, ends every session of the account (pushes) and records
    `Outcome::Reset` for the account's login key (SIGN-32). It leaves the socket-wide key alone.
  - Change password ends every other session.
  - Hand-over is one transaction, so there are never two administrators or none.
  - Candidates are accounts that share at least one organization with the administrator, shown by their person name in that
    organization. They are never all accounts (SIGN-28).
- Tests: host fixtures for a second account (SIGN-39, SES-3 change-password). Admin reset on an existing admin →
  set-password link; old password refused; sessions ended; a rate-limited login is allowed again (SIGN-32 host).
  Playwright SIGN-26, 32 (link part), 37 (redemption), 38. Chooser empty state.
- Accept: SIGN-26, 32, 37 (redemption), 38, 39 (host) pass; FC-014 and FC-153 notes updated.

### B9 · Gateway→web rename in host/web/control (mechanical; window agreed with A1.6)
- Scope unchanged from revision 1's B2: Rust identifiers, the `org_proto::gateway` → `web` module, `GatewayErrorCode`, vector
  file names (D1). Docs: FEATURES FC-013/014/018, `office-host-boundary.md:394-402`, `system-architecture.html:837`.
- Not here: `crates/org-office-launch/**` (A1.6), JSON wire strings (B2 did those).
- Timing: right after A1.6 merges or before it starts, never interleaved. One commit; behavior unchanged.
- Accept: `rg -i gateway` over the three trees lists only A1.6-owned imports; full Rust checks.

### B10 · Office grant-bound subscribe (after A1.5 merges and A1 hands over the files)
- Files: `apps/agentic-org-office/src/serve/host/{operations.json,authority.rs}`, `serve/host.rs`, office tests.
- Wire: `subscribe {grant, feeds:[organization], cursor?}` → `ready`; `subscription.renew {grant}`. An expired-unrenewed
  or revoked grant ends with `session_ended`. Re-enable `tests/launch.rs:800,813`, `tests/launch/inbox_hint.rs:4,15`.
- Accept: office tests green.

### B11 · Web channel pool and `/events` (after B6 and B10)
- Files: `apps/agentic-org-web/src/office/{pool.rs,grants.rs}`, `src/events.rs`; `session.grant {session_id, office_id,
  channel_id}` (TTL fixed 5 min).
- ≤ 7 channels per office. `POST /events` SSE + `/events/poll`. Feeds `instance_status` (subscribe-then-list)
  and `organization` (+ `GET /organizations/{org}/snapshot`). Work/messaging deferred (D7).
- Tests: change by another caller arrives; sign-out ends the stream with `session_ended`; 8th channel refused; renewal
  across > 5 min with an injected short TTL.
- Accept: implementation plan "change stream"; FC-013.

### B12 · SIGN-34 (Mac) message order; enforcement stays at the host (1 run; any time after B8)
- Enforcement: unchanged. `admin.rs` `reset_with` refuses unless `presence::local` (audit-token console presence)
  is true. That is the only gate, and the CLI never decides access.
- Hint: `apps/agentic-org/src/admin.rs` checks `SSH_CONNECTION`/`SSH_TTY` with a pure function. If set, it prints the
  "needs you at this Mac" message **first** and exits per §4a.2 without calling the host. If unset, it calls the host, which
  refuses a non-console peer with the same message. No `org-platform` adapter: the env read is portable and grants
  nothing. Linux keeps failing closed (SIGN-33/34 Linux → FC-164).
- Tests: with SSH env → message first, exit status, host DB unchanged. **SSH env cleared and presence false**
  (test composition) → still refused, same message, DB unchanged. `reset_with(local=false)` unit test exists, keep.
  No `account` command (SIGN-41/42 regression). SIGN-30 macOS rows.

### B13 · Web launch folded into the one launch contract (last; after A1.6)
- Files: `crates/org-office-launch/src/web.rs` (types moved from `org-host-control/src/gateway.rs` / `web.rs` after B9),
  `crates/org-host/src/web.rs:394-560`, `supervisor/child.rs` (shared rendezvous + admission helper),
  `apps/agentic-org-web/src/process.rs:42-120`. Deletes the separate rendezvous types.
- Tests: `web_process_tests`; web exits on host loss (kill -9 host); a v3-era line is refused.
- Accept: "separate web rendezvous lines are deleted"; ADR-0008 confinement before input.

## Dependencies and merge order

1. B1 (running) → B2 → B3 → B4 → B5 → B6 serial: the critical path to the first usable slice. B2 and B3 touch
   disjoint files and may use two writers (separate slots) if the lead wants.
2. After B6: B7 and B8 serial (both touch `authentication.rs`/`sessions*`). A4 may start after B6.
3. B9 in the A1.6 window (needs `crates/org-host/src/service/*` quiet).
4. B10 only after A1.5 merges and A1 hands over `serve/host/authority.rs`, `serve/host.rs`. It is office files only and may be a
   second writer alongside B7/B8. B11 after B6 + B10. The A1 EINVAL fix (`supervisor/channels.rs:275,331,344`) is wanted
   before B11's real-process runs.
5. B12 any time after B8. B13 last, strictly after A1.6 (the lead updates `.agents/agent-kit/slices/a1-launch-no-sqlcipher-plan.md:181-186`).

## Decisions (D1-D10 agreed in review 1; recorded)

D1 rename depth yes (B9). D2 wire renames in B2. D3 no compat shim. D4 tables in place, version 1, with the P3-14
refusal. D5 ENG-3 for A3 = sessions + accounts/lockout/administrator + links; FEATURES/ADR-0018 follow-up row
(B6 docs). D6 digest-only ops; `account.verify` stays native-only. D7 `instance_status` + `organization` feeds;
work/messaging deferral recorded in FC-013. D8 4096, same `unavailable` body, reap before refusing. D9 5 min.
D10 SIGN-33 + Linux SIGN-34 out of A3; Linux fails closed.

New for the lead (revision 2):
- R1 Rotation grace accepts mixed current/previous values of one session (reason in §Rotation). It keeps Web Locks
  rather than adding a SharedWorker. Refresh conflicts are 409, never a sign-in dialog.
- R2 Username rule 1–64 scalar values, no edge whitespace or control chars (closes A81). Password 12..1024 bytes.
  `login/taken` is revealed only behind a valid registration link.
- R3 POST requires `application/json` (415 otherwise) and `Sec-Fetch-Site` cross-site is refused: new refusals
  beyond today's ingress.

## Risks

- Browser sessions currently ride control-connection lifetime in grants and channel binding
  (`authentication.rs:514-600`). B4 keeps channel binding per connection; only the session outlives it.
- SSE on HTTP/1.1 uses up the per-origin pool. B11 ships `/events/poll` too, with one stream per origin via the leader.
- Real-process flakiness (EINVAL, port reuse, `apps/agentic-org/tests/support/mod.rs:592`): B1 first; reuse the A2
  spawn/socket lock.
- Suspended-child orphans on host SIGKILL affect B5/B13 kill tests until the org-supervise fix lands.
- Playwright against a real instance needs Keychain-free host secrets: use `ORG_TEST_*` and temp roots.
- B3 refusing a mismatched dev DB forces a new instance for anyone on a pre-B3 dev build with a hand-made table.
  That is acceptable because v1 is unshipped, and it is stated in the error text.

## Revision 3 — lead rulings on the second plan review (binding; they override the body where they differ)
1. (P2) Lockout on the web channel: `session.sign_in` and every browser password check do NOT use the socket-wide `""` key.
   Keys: per-login (existing schedule) AND per-client-address, where the web process supplies the peer IP (v4 or v6 /64) as
   an opaque field; the per-address key uses the same schedule capped at 60 s. The native CLI path keeps its current keys.
   Required test: a flood of failures on login X from address A leaves a correct sign-in for login Y from address B
   working with no delay, and from address A delayed by at most 60 s. Admin reset / set-password redemption clears the
   per-login key only (SIGN-32).
2. (P3) ADR-0013 amendment in B7's file list: the 30 s grace, the mixed current/previous rule, 409 on previous values.
3. (P3) Lost refresh response: the web process keeps the raw new cookie+proof of a rotation it performed in memory for the
   grace window and answers one retry presented with the previous pair by returning the same new values (idempotent
   refresh); no second rotation. The web cache maps both cookie digests and accepts both proof digests during grace;
   `session.ended` drops both entries. Tests in B5/B7.
4. (P3) B4/B5 docs lists add the web-api-v1 status table rows (415, 405) and the error-body fields `{field, rule}`.
5. (P3) R2 corrected: password at least 12 Unicode scalar values after NFC and at most 1024 bytes (no byte minimum).
Plan status: ACCEPTED by the lead after two independent reviews; implementation proceeds block by block, each block with
its own fresh review.
