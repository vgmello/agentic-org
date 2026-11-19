# A4 · Create the organization — plan (draft r1, 2026-11-18)

Base: v1-integration (A3 B6 merged locally at d024abd1; A1.7 pending on slot-2). Source is authoritative.
Owner UI scope (2026-11-18): our UI ends at sign-in and the organization CREATE page plus the welcome list/open.
Everything else (org editing, user creation, projects, teams, agents) is backend + documented web API; another agent
builds those pages.

## Summary
- One host op `organization.create` replaces `organization.provision.v1` and both name-only declarations. It runs only
  on the admitted web connection, carries the selected payload, is authorized by the browser pair digests and requires
  the administrator flag.
- Creation is asynchronous at the host: the web host link is one request in flight with a 5 s deadline
  (apps/agentic-org-web/src/host.rs:33, :395-455); a 30 s create would kill it. Admit within budget → `accepted`;
  a host driver completes; the same call with the same operation_id polls and re-drives.
- The office bootstrap gains the creator's person record, lead of the top-level team named after the organization;
  records attributed to that person.
- Web: `POST /api/v1/organizations` (create + poll with the same operation_id) and a real `GET /api/v1/organizations`.
- UI: create page, welcome list/open, `/` routing only.
- Recovery code stays in host custody, collected on the HQ machine (ADR-0011); A4 proves one delivery through the real host.

## Premises the code contradicts (record in docs as A4 lands)
1. No host handler exists for name-only `organization.create` (calls.rs:270; host-control ops.rs:128 declarations
   only); service dispatches hello, instance.status, organization.provision.v1 (service/mod.rs:614).
2. Provisioning is NOT test-only: production `serve` dispatches `organization.provision.v1` on host.sock to any native
   session from account.verify (terminal password). A4 deletes that path.
3. `agentic-org recovery release|status` signs in with a terminal password (human/recovery.rs:382-390) and takes
   --org-id; RCV-14 wants --org '<name>'. Browser-approved CLI sign-in is A5 (FC-173).
4. FC-029 "no account-to-person link" is wrong: account_links/org_owners exist (hostdb/schema.rs:10-14, 47-49);
   establish_organization_owner writes both.
5. Bootstrap writes no creator person; team lead null; attribution created_by_kind owner/id null; HR has is_owner.
6. Org/office/repository/team/HR ids are UUIDv7; owner_member, operation ids and account ids are still ULID.
7. Data root: code writes <instance>/durable/state/<org>/ and durable/authority/<office>/; contract paths are B1b's.
8. Completion currently waits for Active (offices.rs:318, 357); the design says completion is the durable identity.
9. Duplicate name/slug surfaces as `unavailable` (constraint error); OFC-17 needs a named duplicate refusal.
10. FC-018's A4 list disagrees with the spec in places (copy, Close allowed, all four fields); spec wins.
11. Web process before any organization: already true (lead decision 2026-11-18).
12. Organization view lacks slug and offices.

## Decisions (lead, from the draft's recommendations)
- Q1 Recovery: code generated at admission into host custody; browser sees only recovery: pending|delivered|acknowledged;
  collected on the HQ machine with `agentic-org recovery release`. Real-process proof: browser create → native release
  (test presence) → acknowledge → second release refused (RCV-10). Add `--org <name>` (RCV-14). Terminal password stays
  until A5; record in FC-016.
- Q2 Host auth: params carry auth {cookie_digest, proof_digest, origin} like session.validate; validate_browser →
  account_authority; require admin && !revoked; recheck admin in the reservation transaction.
- Q3 Asynchronous admit/drive/poll as above.
- Q4 Completed = worker reported created and adopted, whatever its later status.
- Q5 Creator employee id IS owner_member; keep its current ULID format in A4; the UUIDv7 follow-up moves member/owner,
  operation and account ids together.
- Q6 Creator name = login; handle = login made valid; collision with `hr` or invalid → deterministic suffix; host fills it.
- Q7 organization_repository {kind:"remote", remote_url}; local/other → 400 {field, rule:"remote_required"};
  credential_ref absent in A4 (present → {rule:"unsupported"}); syntax only, no network.
- Q8 Organization view adds slug and offices[] (HQ first); web-api-v1 §3 updated.
- Q9 GET /organizations always asks the host; host pushes account.changed {account_id} after linking; web drops that
  account's cached sessions.
- Q10 Refusals before reservation → rejected (UI mints a new id); after reservation → 503/504 and the UI re-sends the same
  id. organization.cancel deferred (new FEATURES row).
- Q11 Slug prefilled kebab-case from the name until edited; host never derives it.
- Q12 Shell (FC-018 2, 3, 8): owner said another agent builds the other pages. A4 owns only a new selection module,
  the welcome Open action and `/` routing; it does not edit (shell) layouts. FC-018 (2) shell-name part, (3) switch and
  (8) shell-after-reload move to the shell agent's acceptance; record in FC-018.
- Q13 Spec copy (offices §5a / OFC) wins; rewrite FC-018 items accordingly.
- Q14 Data-root contract paths → B1b; record the gap.
- Q15 Old-store "Owner" member name stays until B1b deletes the old store.
- Q16 Drop is_owner from HR; HR attributed to the creator; HR paused/not ready (FC-031).

## Coordination (max 2 writers)
- A1.8 (retire the app-token path) touches org-proto msg/calls/token, org-client, worker serve.rs, host secrets.rs,
  service/mod.rs, host-control secret.rs, UI wire.ts/proto.ts/model.ts/mock.ts. A4 avoids them: new dispatch in a new
  service/organizations.rs (mod.rs gets one hook + the provision.v1 deletion), gateway create params in
  org_proto::gateway, UI readers in a new src/lib/platform/organizations/ module.
- Order: writer 1 = A1.8 (after A1.7 merges); writer 2 = A4.1 (after A1.7 merges) → A4.2 (after A1.8 merges, or
  before A1.8 starts) → A4.3 → A4.4 → A4.5. A3 B7/B8/B9/B11 touch api.rs/browser.rs/ops.rs: serial with A4.2/A4.3,
  never interleaved; B9 (rename) before A4.2 or after all of A4.

## Blocks
### A4.1 · Creator person in the selected bootstrap (contract + office) — foundation
Owns: crates/org-proto/src/selected_bootstrap.rs; crates/org-office/src/organization/bootstrap.rs (+tests);
crates/org-office/src/hq_creation.rs; crates/org-host/src/provisioning.rs (creator from Account, Q6);
crates/org-host/src/hostdb/provisioning.rs; provisioning/tests.rs; web_process_tests.rs + enc1_process_tests.rs (fixtures).
Contract: PlanV1 {version:1, name, slug, repository_remote_url, state_branch} (home_team_name deleted; team = name);
PreparedV1 + creator {employee_id, name, handle}; valid(): creator.employee_id == owner_member, handle valid and not hr,
ids distinct. No shim.
Tests: materialized person + team lead + attribution; Facts::validate; retry verifies without rewriting; hr collision;
login→handle; files on disk under durable/state/<org>/teams/<team>/people/<id>/profile.md.

### A4.2 · organization.create (async) and organization.list on the web connection; provision.v1 deleted
Owns: host-control provision.rs → organization.rs, browser.rs, ops.rs, journal.rs, tests.rs, lib.rs; org-host new
service/organizations.rs, service/mod.rs (hook + deletion), provisioning.rs (admit/drive split, driver registry,
re-drive), offices.rs (provision_hq returns adopted; list projection), hostdb/provisioning.rs (NameTaken/SlugTaken
before insert), service/authentication/browser.rs (expose validate_browser → Account).
Contract: organization.create {auth, operation_id, plan} → Accepted | Completed{organization view} | errors
not_authorized, invalid_request{field,rule}, already_exists{field}, operation_reused, clock_skew, expired,
unavailable/storage_error (same id re-drives). organization.list {auth}. Push account.changed after linking.
Tests: refused off web connection / host.sock (stores nothing); non-admin, revoked, other-origin refused; duplicate
name (case-insensitive)/slug → already_exists before any key/custody write; replay; operation_reused; poll during drive
returns accepted < 1 s; driver failure keeps reservation and next poll re-drives; adopted-then-crashed completes; list
shows only linked; account.changed before reply.

### A4.3 · Web routes + real-process acceptance (the API other pages use)
Owns: apps/agentic-org-web/src/api.rs, session/cache.rs (drop by account), host event sink, api/tests.rs;
org-proto gateway mod.rs/views.rs + v2 vectors (+ TS vector counts); new web_process_tests/organizations.rs;
web_process_tests.rs (HTTP create helper; delete provision_hq helper); recovery_custody.rs port; enc1 port;
human/recovery.rs (--org <name>).
Mapping: Accepted → 202 {operation_id, outcome:"accepted"}; Completed → 200; refusals → 400/409/403;
unavailable/storage_error → 503; timeout after write → 504 unknown.
Real-process tests (temp roots, real images, loopback): (1) link → register → create → poll completed; files on disk
(organization.yaml, repositories.yaml kind remote, team.yaml name + lead, people/<creator>/profile.md,
agents/<hr>/profile.md); UUIDv7 org/office ids; GET /session shows employee_id = creator. (2) restart: same cookie+proof
lists it with HQ; files unchanged; link survives. (3) lost response: same op → completed same ids; different payload →
409 operation_reused. (4) SRV-24 two orgs on one listener; duplicate name 409; local/file:// repo 400 with field.
(5) recovery once via native release; code never in any HTTP response or log. (6) ENC-2 scan (A1.7 scanner).
(7) ingress refusals reach no host op. Own port base for real-instance tests.

### A4.4 · Delete name-only remnants (after A1.8 merges)
calls.rs OrganizationCreate/Params, org-proto re-exports/tests, org-client create_organization + tests, service/mod.rs
test imports, test_support, apps/agentic-org tests. rg 'organization\.provision|OrganizationProvision|create_organization\('
is empty.

### A4.5 · Create page and welcome list (UI; last UI we build)
Owns: routes (onboarding)/create, (onboarding)/welcome, src/routes/+page.ts; src/lib/create-flow.ts (+test);
src/lib/open-flow.ts (+test, off OfficeApi); new src/lib/platform/organizations/{api,readers,selection,slug}.ts (+tests);
new e2e/organizations.spec.ts (mocked HTTP); new e2e-real/create-organization.spec.ts. No (shell) layout edits (Q12).
Behaviour: name, slug (Q11), Org Git URL, state branch; ownership/recovery note with the instance name from GET /instance;
one operation_id per attempt; Creating… disabled; poll on 202; Close allowed; lost/504 → "Checking whether <name> was
created…"; failure keeps fields, Try again (new id only after rejected), Copy details without secrets; OFC-22 copy for 403;
success selects and enters. Welcome list: HQ office, host, created date, open-now marker; unavailable not openable;
open re-reads (vanished → "Couldn't open it"); HQ unreachable explains and still offers Create; none → SIGN-9.
Tests: vitest; mocked Playwright FC-018 (1),(4)-(7),(9)-(11), OFC-16/17/18/20/21/22/24 (axe); real-instance Playwright:
setup → register → create (four fields) → enters → `/` goes straight in → reload keeps it → list shows it open.

### A4.6 · Docs (rides with each block)
FEATURES FC-013, FC-016, FC-018 (reconcile per Q13, Q12 split), FC-029, FC-031, new organization.cancel row;
web-api-v1 §2/§3 + what the code does today; create-organization.md; office-host-boundary.md:397-402;
offices-and-teams-ux §5a (slug prefill, field error copy); implementation-plan §A4 code today + §B1b paths note;
doc 01/04 creator id format (Q5).

## Risks
Serial web link (async mandatory; poll-latency test); stuck reservation until cancel exists; ULID/UUIDv7 drift for
member/operation ids; two owner records until B1b; login→handle mapping (fuzz); real-instance port contention (own
port base); ordering with A3 B7/B8/B9 on api.rs/browser.rs/ops.rs; recovery CLI password until A5.

## Acceptance map
Done-when (browser create survives restart; owner person in top-level team and lead; files under durable/state;
recovery once; ENC-2; UUIDv7 org/office ids): A4.1-A4.3 real-process tests 1, 2, 5, 6 + A4.5 real Playwright.
One create command: A4.2 + A4.4. FC-031 HR lead: A4.1 + test 1. FC-029: test 1. SRV-24: A4.2 + test 4. SIGN-23, OFC-*:
A4.5. RCV-14: A4.3. FC-018 (1),(4)-(7),(9)-(11): A4.5; (2) `/` part A4.5, shell part → shell agent; (3),(8) → shell agent.

## Revision 2 — lead rulings on the independent plan review (2026-11-18)
1. (P1) Creator employee id is UUIDv7 in A4 — Q5 reversed. new_pending mints owner_member/creator.employee_id with
   ids::uuid_v7(); PreparedV1::valid() requires UUIDv7; widen every 26-char member check (org_create_staging.owner_member_id,
   account_links.member_id, writer Admission, office hq_creation, launch contract is_valid_member_id) to accept the canonical
   UUIDv7 form in A4.1; test the creator id is UUIDv7. Operation and account ids stay ULID in A4 (host-db only); the
   UUIDv7 follow-up moves them.
2. (P1) Restart / abandoned client: (a) on startup the host scans reserved provisioning rows and re-drives each under the
   driver registry; (b) organization.list includes the account's in-progress creates as {state:"creating", operation_id,
   name} so the welcome page resumes them (no client-side durable id needed); (c) test: host restarts mid-drive with no
   client poll and the org reaches linked and is listed.
3. (P1) Lock split: admit holds the gate only for validate, name/slug check, custody put of the pending code, and the
   reserve_provisioning commit, then returns accepted. The drive runs on a registry thread without the admission gate;
   it rechecks admin/revoked at its start. A poll reads the journal and the registry's in-flight flag without blocking
   and starts a drive only if none is alive (check-and-set under a short lock). Tests: two concurrent polls start one
   driver; a poll during another create's admission returns < 1 s.
4. (P2) Polling uses the designed lookup route GET /api/v1/operations/host/{id}, backed by a host operation.lookup on the
   web connection (same browser auth; only the account's own operations). Same-id POST replay stays for a lost response.
5. (P2) Host operation ids are ULIDs in A4: the UI mints ULIDs; the web refuses another format with 400
   invalid_request {field:"operation_id"} before any host call; vector test; recorded in web-api-v1.
6. (P2) Recovery release with a terminal password is an interim, time-boxed exception to ADR-0011 acceptance 5, closed by
   A5 (browser-approved CLI sign-in, FC-173). Record it in .agent-kit/DECISIONS.md and an ADR-0011 status note. A4
   acceptance wording: "delivered once via interim password sign-in; ADR-0011 acceptance 5 not yet met".
7. (P2) After completion the create page shows the recovery-pending state with the exact
   `agentic-org recovery release --org '<name>'` command (shell-quoted safely) and the Waiting state; the organization view
   carries recovery: pending|delivered|acknowledged; mocked + real Playwright cover it.
8. (P2) A4.2 reorders new_pending: name/slug check under the gate → custody put → reserve; case-folded uniqueness stays as
   the insert backstop; a failed reservation insert removes the custody entry (test: no orphan custody entry).
9. (P3) Keep the cross-account replay → operation_reused test; organization.create/list/operation.lookup refused on
   host.sock and agent connections.
10. (P3) web_process_tests.rs: A4.1 then A4.3, serial. A4.5 starts after A1.8 merges; apps/agentic-org-ui
    src/lib/offices/model.ts is read-only for A4.
11. (P3) Paths: crates/org-host/src/human/recovery.rs. Poll: 1 s, backing off x1.5 to 10 s, stop on completed/rejected.
Status: READY for A4.1 after A1.7 merges (A4.1 does not touch A1.8 files).

## Revision 3 — lead rulings after A4.1+A4.2 (1dbd5b41) and its review (2026-11-18)
- account.changed is pushed directly on the admitted web connection(s), the way session.ended is — not through the
  host.sock broadcast subscribers; native host.sock subscribers never see it.
- organization.list result keeps the built shape: `organizations: [...]` plus a separate `creating: [{operation_id, name,
  slug}]`; document it in web-api-v1.
- Review findings to fix before A4.3 work: F1 (P2) after taking the drive claim, re-read the reservation and return if
  linked; drive from the fresh row. F2 list propagates a DB error and represents a missing organization (skip + log)
  instead of failing the list. F3 the gate test must fail, not hang (recv_timeout). F4 in-transaction administrator
  failure → not_authorized. F5 startup sweep deletes custody `recovery.<op>` entries with no reservation row, plus tests
  for the staged path (same id continues; another account → operation_reused; name taken → entry deleted). F6 one owner
  for the handle rule (reuse the office constants/rule; no second literal list).

## Revision 4 — from the B1a plan review (2026-11-18)
A4's bootstrap adopts the data-contract bytes now, so organizations created before B1b still open after it: the HR
profile has `status: active` and NO `agent_status` field (docs/design/organization-data test_schema.py:843 asserts no
such column); agent profile path state/teams/<team-id>/agents/<employee-id>/profile.md. B1a will add a shared golden
fixture of the creation files; A4's bootstrap test must pass against it when it lands.

## Revision 5 — lead rulings after run 3 (0bb117c3, a7ed956d) (2026-11-18)
- Run-3 decisions 1, 2, 4, 6, 7 accepted. Decision 5 accepted: an explicit host unavailable/storage_error on create
  answers 504 {outcome:"unknown"} (the same id continues); document it in web-api-v1 §5 in place of the planned 503.
- Decision 3: the new gateway/organizations.rs types are the contract. The old gateway OrganizationView and its vectors
  are deleted in A4.5 when the UI reader moves to the new view; until then both exist — record in FC-018.
- recovery --org <name>: LinkedOrg in the account.verify result gains `name` (additive host-control field, with vectors);
  the CLI resolves the name among the account's linked organizations; two with the same name cannot exist (name unique
  per instance); unknown name → a refusal naming the organizations it can see.
- Prove (test) that a session.validate reply and account.changed keep host order on the web's one ordered reader, so a
  stale session cannot be re-cached after the change; if it can, fix it.
- Close the open test gaps: F2 (forced registry error in list), F4 (reach or delete the unreachable branch), HEAD on the
  lookup route, the cache-drop test calls process::session_events.

## Revision 6 — lead rulings after run 5 (beb3f009, 87c41de7) (2026-11-18)
- Run-5 decisions 1 and 3 accepted. Decision 2 (startup resume retries a reserved create up to 3 times) accepted with:
  a stop flag so the retry thread ends at Host::stop and never writes custody/registry afterwards; a permanent refusal
  is not retried; a unit test (fake Hq failing once → start with a reserved row → linked, drives_started()==1).
- The mid-create restart real-process test holds the drive with a gate so it cannot link before the host stops.
- Plan test 5 captures process stderr and every poll reply; test 7 also proves body refusals reach no host op at all
  (or states exactly which op they reach and why that is acceptable); the "local with path" case asserts its field.
- Leak at host stop (a strong reference to the registry and custody survives drop(host); suspected writer Lease cycle,
  writer.rs:81): trace it for at most ~6 turns; fix if small, else leave a precise note for a follow-up row. Production
  exits the process, so it does not block A4.
- apps/agentic-org-ui wire.ts:127 refusing `-` in lookup ids is fixed in A4.5 with a shared vector holding a UUID id.
- No organization created before 87c41de7 exists outside tests; no migration.
