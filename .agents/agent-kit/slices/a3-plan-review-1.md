# A3 plan review 1 (fresh Opus High) — PLAN_CHANGES_REQUIRED; lead decisions
Findings 1-14 and D-verdicts as returned by the reviewer (see lead transcript); summary and lead rulings:
1 P1 reorder: ACCEPT. First vertical slice = register + sign-in + GET /session on a real `serve --name` instance:
  minimal store (session table + account ops registration needs), host ops link.register / session.sign_in /
  session.validate / session.end, web routes, register + sign-in pages + SIGN-9 empty state. Refresh/rotate/touch,
  password change, hand-over, set-password, change stream after. B2 rename off the critical path (after the slice, in a
  window agreed with A1.6). B1 (lingering close) already running.
2 P2 Origin/Host/CSRF: ACCEPT — state rule (configured Host, browser Origin required on unsafe methods, JSON only, no
  credentialed CORS), test each refusal; check what the web library enforces today first.
3 P2 lockout/throttle: ACCEPT — session.sign_in uses hostdb lockout; 429 rate_limited with retry_after_ms; throttle password
  checks in set_password/hand_over; set-password redemption clears the account's limit (SIGN-32).
4 P2 rotation vs tabs: LEAD DECISION — the UI leader (BroadcastChannel/SharedWorker owner) performs refresh and broadcasts the
  new proof; the host keeps the previous cookie+proof pair valid for a short fixed grace (30 s) after a rotation, single
  rotation in flight per session; test two tabs and an in-flight request across a refresh. Planner may propose an
  alternative with reasons.
5 P2 SIGN coverage: ACCEPT — reconcile; SIGN-11..16 either in or explicitly deferred with a FEATURES row; SIGN-33 and Linux
  SIGN-34 leave A3 (milestone 2); SIGN-24 regression only; add SIGN-7, SIGN-27.
6 P2 data A3 can't create: ACCEPT — host-level fixtures for hand-over/SES-3; define the hand-over chooser source without
  listing all accounts; defer browser parts explicitly.
7 P2 username/password rules: ACCEPT — host authoritative (≥12 chars; NFC, case-folded unique usernames), UI mirrors, error
  codes→copy mapping; passwords in redacting zeroizing types.
8 P2 SIGN-34: ACCEPT — enforcement stays the host's audit-token presence check; any SSH hint only orders the message; test:
  SSH env cleared and still refused.
9 P2 office files: B8a starts only after A1.5 merges (A1 then hands over serve/host/authority.rs, serve/host.rs).
10 P3 B9 position: B9 last in A3, after A1.6; lead updates the A1 plan note.
11-14 P3: fold in (web-api-v1 §2 table; store contract details; ended-push vs validate ordering; idempotent table creation
  for existing dev instances or a clear refusal — never partial).
D1-D9 agreed (D5: add FEATURES/ADR-0018 follow-up row for the rest of ENG-3; D7: record work/messaging deferral; D8: 4096,
same unavailable body for every account, reap before refusing). D10: SIGN-33 + Linux SIGN-34 out of A3; Linux fails closed.

## Lead rulings on revision 2 (R1-R3)
- R1 ACCEPT: refresher = holder of the `org-session:refresh` Web Lock; host accepts any mix of current/previous values of the
  same session for a fixed 30 s after a rotation; rotation only against current values, else 409 conflict with no change;
  the UI reloads the saved session and never shows the sign-in dialog for it. No SharedWorker.
- R2 ACCEPT: usernames 1-64 Unicode chars after NFC, no leading/trailing whitespace, no control chars, unique by NFC+case-fold;
  passwords ≥12 chars, ≤1024 bytes; login/taken only behind a valid registration link.
- R3 ACCEPT: 415 for non-JSON POST, 403 for Sec-Fetch-Site: cross-site, OPTIONS 405 without CORS headers, never
  Access-Control-Allow-*.
- A1 plan note: the web launch fold is A3's last block (B13), after A1.6.

## Lead rulings on B3 (AccountStore)
- Verifier lives in hostdb::journal (adapter module) but the contract uses it: move the PHC Verifier value type to
  accounts/ in B4 (after A1's UUIDv7 merge), so the contract depends on nothing in hostdb.
- One per-login guess budget shared by the web and native paths (socket 'control' row): ACCEPTED — one account, one budget.
- LoginTaken only after revocation; revoke helpers in tests: fine.

## Lead rulings on B4 deviations
- No separate BrowserAuth struct (view() needs account_authority, no contract yet): ACCEPTED for B4; add an AccountAuthority
  read to the AccountStore contract (or its own) before B8 so the host's browser path depends only on contracts (ENG-1).
- Wall time via wall_ms in dispatch, now_ms parameters in the logic: ACCEPTED.
- Ordering test host half only; web-cache half in B5: ACCEPTED.
- B5 review must check: the web process fills client_address from the socket peer address, never from X-Forwarded-For or any header.

## Lead rulings on B5 questions
- GET /instance: add a small host op on the web connection (e.g. `instance.view` → name, status, web status, administrator
  present) — the SIGN-9 empty state and the app shell need it; no secret; tested.
- field/rule/retry_after_ms: add them to the shared error DTO (org_proto gateway error) as optional fields, with vectors read by
  Rust and TS; never hand-built JSON.
- GET /organizations: `{organizations: []}` after validating the session, no host call, until A4 creates organizations;
  documented in web-api-v1 "what the code does today".

## Lead rulings on B5 run 3
- instance.view: the instance name is set on the Service by the registered start (registered.rs, like install_admin — an
  `install_instance_view`/name setter), not via supervisor/. Result: {name, status, web_status, administrator}. TLS state
  is omitted (optional field, absent) until TLS lands; documented.
- Credential bodies (sign-in, register, later set-password) must not linger in non-zeroized buffers: a credential ingress that
  reads the bounded body into a Zeroizing buffer and parses into Password from it. Required before B5 merges.

## Lead ruling (premise correction found in B5): when the web process runs
F4 made the host start the web process only when authentication is present AND an HQ office is Active
(crates/org-host/src/service/mod.rs web_eligible/web_serving/hq_active). That contradicts A3/A4's outcome: the person opens
setup's link and registers BEFORE any organization exists, and A4 creates the organization from the browser
(implementation-plan §A3 outcome, §A4; SIGN-9 empty state). Ruling: the web process is eligible once the instance is
registered and its registry/authentication is present; HQ being Active is NOT required. Routes that need an organization
or office answer their own "not yet" state (organizations: []). Update the F4 real-process test that asserted "no web process
before HQ Active" to the new rule, and record the change in docs/design/office-host-boundary.md and FEATURES FC-013.

## Lead ruling (B6 premise correction): the old mocked welcome list
My earlier ruling assumed the officeApi-driven organization list still existed behind sign-in; it doesn't (539b1a26 replaced
it with the API-backed SIGN-9 page). Ruling (b): rewrite shell-switch-footer and desktop-navigation-focus to the SIGN-9 /
API shape (they test shell behaviour, not the list). open-organization and create-organization tested a mock-only flow
(officeApi rows, Open, office line) that A4 rebuilds against the real API: remove those two specs now and make their
behaviours A4's acceptance (list from GET /organizations, create, open), recorded in FEATURES FC-014/A4 row and the A4
plan. No skipped specs left in the tree.
