# Gateway HTTP API v1

Wire `api_version: 1`, `contract_revision: 2`. The core follows design revision 2;
the organization mutation codec in §9 follows Rowan's approved design revision 3.
Both were agreed 2026-11-10 by Rowan (technical lead) and ada. This publishes types and shared
conformance fixtures before the gateway implementation; it does not advertise
runtime availability. Basis: `standalone-server.md` §5–6 and approved X-02 rev13
(see `office-host-boundary.md` §3 for its agents-repository location).

## 1. Scope and placement

Publish the first `/api/v1` contract before gateway implementation (B1-05/06).
Cover version/capabilities, browser authentication, instance/organization/office
status, operation outcomes/lookup, and multiplexed feeds. Later typed domain
operations extend it before their screens connect. Section 9 adds their codec;
HTTP composition and enablement remain separate. This is not a JSON-RPC tunnel.

Approved placement: `org_proto::gateway` owns transport-neutral browser wire DTOs;
`crates/org-proto/test-vectors/v2/gateway-*.json` owns the core shared examples;
`test-vectors/v2/gateway/` owns organization mutation and encoding-guard vectors.
`org-host-control` remains private to host/gateway/setup; no browser import
of host-control methods or secrets. `org-proto` versions, launch v3, host-control
v1 and HTTP `/api/v1` remain independent version spaces.

FE-1 (Jules) owns `apps/org-ui/src/lib/platform/http/wire.ts` and
`gateway-vectors.test.ts`, reserved to his session for the minimum TS mirror and
reader. Kai owns Rust DTOs/vectors/reader and lands them first; Jules's companion
branch follows from those landed files. B1-02 is complete only when both readers
pass the same vector set on v1. This does not start F1-02's transport
implementation. Vectors never enter production
wire imports and are never production mock fallback.

## 2. HTTP surface (routes frozen by Rowan)

All JSON responses use `Cache-Control: no-store`, except hashed UI assets. The
page and API share the configured origin. Missing API routes are JSON 404, never
SPA HTML. Unknown operations do not forward to a local socket.

| Method/path | Body/result | Access and source |
| --- | --- | --- |
| GET `/api/v1/version` | `{api_version:1, contract_revision:2}` | Public, constant protocol metadata only; no instance/organization/account information |
| POST `/api/v1/session/sign-in` | `{login,password}` → session view plus proof | Host `account.verify`; configured-origin validation, bounded limits |
| GET `/api/v1/session` | session view, excluding proof | Existing cookie/proof pair; never recover a proof from cookie alone |
| POST `/api/v1/session/refresh` | `{}` → session view plus rotated proof | Existing pair; host `session.renew`; rotate both browser values atomically |
| POST `/api/v1/session/sign-out` | `{}` → `{}` | Existing pair; end host session, clear browser pair and terminate streams |
| GET `/api/v1/capabilities` | capability view (§7) | Pair; authorized scopes only |
| GET `/api/v1/instance` | instance view (§3) | Pair; host-derived status, no catalog paths or private listener/socket credentials |
| GET `/api/v1/organizations` | `{organizations:[...]}` | Pair; host `organization.list`, linked organizations only |
| POST `/api/v1/organizations` | command envelope with `{name}` | Pair; host `organization.create`, instance admin only |
| POST `/api/v1/organizations/operations/{id}/cancel` | `{}` → dedicated cancellation result (§5) | Pair binds selected instance; host `organization.cancel`, same account/admin, original create ID |
| GET `/api/v1/organizations/{org}/offices` | `{offices:[...]}` | Pair; host `office.list` narrowed to an authorized organization |
| GET `/api/v1/organizations/{org}/snapshot` | `{snapshot:OrgSnapshot,resume_cursor}` | Pair; canonical `org.snapshot` on selected HQ, cursor at the same snapshot position |
| POST `/api/v1/organizations/{org}/offices/{office}/restart` | office-scoped command envelope with `{}` | Pair; host `office.restart`, account linked to organization; no admin bypass |
| GET `/api/v1/operations/{kind}/{id}` | scoped lookup (§5) | Pair; `kind=host\|org\|work\|msg`; query fields `instance_id`, `org_id`, `office_id` as specified below, checked before lookup |
| POST `/api/v1/events` | subscriptions/cursors (§6) → SSE | Pair; streaming fetch, no proof in URL |
| POST `/api/v1/events/poll` | same subscriptions/cursors → bounded frame batch | Pair; bounded short-poll alternative, no held connection |

Lookup query spelling is fixed: `instance_id` is always required; `kind=host`
forbids `org_id` and `office_id`, `kind=org` requires `org_id` and forbids
`office_id`, and `kind=work|msg` requires both `org_id` and `office_id`.
The gateway resolves HQ for `org`; work/message lookups go to the office that
admitted the ID. All IDs are 1..64 ASCII letters/digits, matching the existing
plain office-ID shape. Empty values, repeated keys (even identical values), unknown
keys and malformed IDs are HTTP 400 `invalid_request`, checked before authorization.
The adapter passes decoded query pairs to `LookupQuery::from_fields` without first
collapsing duplicates into a map. The query's instance must equal the session
pair's instance; mismatch is the same 400 scope-mismatch refusal as mutations,
never forwarded. A well-formed hidden scope and a missing record return the same
404 body, byte for byte. Shared `gateway-lookup-query.json` pins the fields,
per-kind requirements, malformed/duplicate refusals, instance binding and visibility.

The minimum organization snapshot is an M1 dependency and is included here;
B1-10/11 add further organization/work operations before M2 screens connect.
Existing OrgSnapshot and positions are reused. The response's opaque resume
cursor continues exactly after the snapshot position, without a gap or duplicate;
a raw Position alone is not a browser resume token.

## 3. Views and command envelope

Session view:

```json
{"instance_id":"01K9P000000000000000000001","account":{"id":"01K9P000000000000000000002","login":"alex"},"admin":true,"organizations":[{"org_id":"01K9P000000000000000000003","member_id":"01K9P000000000000000000004","office_ids":["01K9P000000000000000000005"]}],"server_time_ms":1794312000000,"absolute_expires_at":"2026-11-11T00:00:00.000Z","idle_expires_at":"2026-11-10T12:30:00.000Z"}
```

**Clock frozen by ada:** `server_time_ms` is integer Unix milliseconds. No
RFC3339 `server_time` alias is accepted. The conformance vector includes offset
calculation; validate a nonnegative JavaScript-safe integer at runtime.

Only sign-in and refresh add `proof` in the no-store response. Host session handles,
grant MACs and channels never appear in this view. `server_time_ms` lets FE-1 mint
ULIDs with the server offset; gateway prechecks `clock_skew`, owner rechecks it.
Host defaults are 12 h absolute/30 min idle; refresh cannot extend absolute expiry.

Instance view: `{instance_id,name,server_version,gateway_status,listen_scheme,tls,
server_time_ms}`. `server_version` is the release pin's version. `gateway_status`
is `ok|degraded`, `listen_scheme` is `https|http_loopback`, `tls` is
`ok|missing|expiring`. Unknown enum values decode to explicit `unknown`. There are
no paths, endpoints, sockets, handles or certificate material in this view.

Organization view reuses existing organization identity/name/created_at and adds
`recovery: pending|delivered|acknowledged`. Office view reuses OfficeInfo's identity,
organization_id/name/role/host/status/status_reason/next_restart_at, **omitting
`endpoint`**, which is a local socket detail. Recovery code never crosses HTTP.
Status reason `unsupported_version` is distinct from the request error code.

Completed result types, frozen by Rowan `01M72Z9MC0CRC011GJ2EWQYBQD`:
organization.create returns exactly OrganizationView identity/name/created_at/recovery;
office.restart returns `{}`. Offices and health come from the offices list/feed,
not these completed results. Lookup uses the same public projection, with identical
serialized result bytes. The exact refused object keys are `endpoint`, `socket`, `path`, `recovery_code`,
`db_key`, `owner_grant` and `host_control`, plus every key ending in `_path`
(including `display_path` and `socket_path`). This applies recursively at every
object nesting depth, including objects within arrays, in both create/restart
mutation results and their lookup results. Unknown additive business fields that
do not match this rule remain ignorable. The shared mutation-result vectors pin
the named keys, suffix rule and nested-key rule. This guard is specific to these
public host-result projections, not domain records such as repository paths.

Mutation envelope (path fixes the typed operation):

```json
{"operation_id":"01K9P000000000000000000006","scope":{"kind":"office","instance_id":"01K9P000000000000000000001","org_id":"01K9P000000000000000000003","office_id":"01K9P000000000000000000005"},"params":{}}
```

`scope` is a tagged union: `instance {instance_id}`, `organization
{instance_id,org_id}`, `office {instance_id,org_id,office_id}`. Path/envelope
mismatch is refused before dispatch; authorized identity comes from the pair,
never from a supplied account/member/principal. Foreign authorized-scope lookups
use `not_found`; absent/bad pair uses `not_authorized`.

There is exactly **one operation ID**. Gateway maps it to host `operation_id`, org
`command_id`, work `change_id` or `msg.send.id` without generating another ID.
Operation IDs are idempotency keys, not ordering keys (D-GW3, ada
`01M72X7AAJTVZZW4MCQ6GSK2RV`). Secure random ULIDs need no same-millisecond
monotonic ordering: receipt and versions determine server order. Concurrent calls
may coalesce per browser handle; global replay/dedup belongs to the worker, so two
handles with the same ID, identical params and the same account are safe through
server replay. Different params/account get operation_reused409 after move2a
(native invalid_request before it). The ULID time comes from server_time_ms-corrected
time, never the raw local clock; HQ rejects a new ID over five minutes ahead with
clock_skew or over29days old with expired. No browser operation registry/outbox.
Native field does not also appear in browser params. `expected_version` stays in
the operation's typed params, with the existing per-entity semantics; no second
envelope version competes with it. Gateway reconstructs the existing typed params
before hashing/dispatch. Business params are otherwise unchanged. Retired
`home`/`clear_home`/`profile.home`/`app_token` are refused on presence, including null.

Final D-GW1/D-GW2, ada `01M72WNGF3DJHX1HZXAC2MA8H3`, with Rowan's correction:
validate raw path segments before routing, without normalizing. Refuse encoded
`%2f`/`%5c` in any case, raw backslash, empty segments and exact `.`/`..` segments
(including `%2e` in any case and mixed spellings) with 400 invalid_request.
Clients segment-encode IDs and treat an ID separator as a client bug. A raw segment
such as `%252f` represents a literal percent escape; decode at most once, never a
second time into a separator. The leading root slash is not itself a segment.

B1-05 accepted parser limitation (D-B59; Rowan, 2026-11-11): these rules apply
to the target presented by pinned Hyper `=1.12.0` and lockfile `http` `1.5.0`.
[RFC 9110 §7.1](https://www.rfc-editor.org/rfc/rfc9110.html#section-7.1) excludes
fragments from a request's target URI; [RFC 9112 §3.2.1](https://www.rfc-editor.org/rfc/rfc9112.html#section-3.2.1)
defines origin-form using only path and query. A client-sent literal `#` is
malformed, but `http` truncates it and everything after it before routing.
The gateway accepts this parser behavior: validation and authorization use the
same retained path, and discarded bytes do not select routes or static files
or appear in responses. A raw wire regression pins status, body and policy
headers to those of the equivalent effective API target. No parser fork or
additional framing layer is used; this limitation is recorded as a LOW for review.

Hidden resources and missing resources have the same not_found response. A visible
resource the caller cannot change is 403 not_authorized; there is no new permission
code. A confirmed submit not_found is a rejected attempt with nothing applied.
Grant/scope refusal is not stored at HQ (lookup returns not_found); a rule refusal
is stored and replays. Keep the draft, never auto-resubmit, and use a new ID for a
new user-directed attempt. If the submit reply was lost, lookup not_found remains
unresolved: it cannot prove which of those histories occurred.

## 4. Browser sessions and failures

Gateway keeps only the in-memory mapping between the random browser cookie/proof
pair and host-issued session handle on its admitted control connection. Host owns
accounts, password verification, expiry, grants and revocation. Worker owns domain
authorization. Gateway does not open any registry or office DB and has no DB key.

Cookie name is instance-specific; HttpOnly, SameSite=Strict, Path=/api/v1, no
Domain, Secure in HTTPS. Explicit configured loopback HTTP can omit Secure; pair
validation still applies. Each browser value has 256 bits of independent entropy. The proof wire spelling is
64 lowercase hexadecimal digits; B1-06 creates it with a secure random source.
All protected reads/commands/stream/poll/refresh/logout require cookie plus
`X-Org-Session-Proof`, bound to configured scheme/host/port, instance and account.
No wildcard credentialed CORS, secret URL/query/path, cross-origin redirect or
cookie-only recovery. Check configured Host and browser Origin; when Origin is
absent for same-origin GET, the valid proof still supplies origin-bound authority.

Credentials go directly into redacting/zeroizing secret types, never logged Value.
Only sign-in accepts passwords. Unknown login and wrong password have identical
`not_authorized` response shape. Sign-in limits may supply `retry_after_ms`; limit
messages reveal no account existence. B1-06 tunes concrete budgets.

On logout, expiry, password change, unlink/revoke or control EOF, invalidate the
pair and stream. Control EOF ends all host sessions on that connection. A gateway
restart loses mappings; require reauthentication. A browser cannot infer restart
from an unknown cookie: use generic `session_ended` unless the gateway has trusted
evidence. No automatic resend after sign-in. Same-account reauth retains user
drafts; switching account clears the old account's views in all tabs (SRV §4a).

Protected auth failure is HTTP 401 with error code `not_authorized` and optional
safe `session_reason: session_ended|hq_restarted`; ordinary owner permission
failure is HTTP 403 `not_authorized`. Neither is a stored business rejection.
Lost transport during a mutation may still mean unknown; session end does not
prove that an earlier operation failed. Lookup after reauth uses the original ID.

## 5. Outcomes, errors and lookup

Discriminated mutation response:

```json
{"operation_id":"01K9P000000000000000000006","outcome":"accepted"}
{"operation_id":"01K9P000000000000000000006","outcome":"completed","result":{}}
{"operation_id":"01K9P000000000000000000006","outcome":"rejected","error":{"code":"conflict","message":"The item changed."}}
{"operation_id":"01K9P000000000000000000006","outcome":"unknown"}
```

`accepted` requires a durable owner journal/ledger record binding ID, fingerprint,
account/scope and outcome; placing work in a gateway memory queue is insufficient.
`completed` proves owner commit. `rejected` is confirmed refusal; it must not be
used for timeout/connection loss after possible dispatch. `unknown` is unresolved,
not durable success/failure; the UI keeps the ID and performs lookup. Never mint a
fresh ID automatically. Transport/client failure can synthesize unknown locally.

Lookup returns accepted/completed/rejected, or an error `not_found`/`expired`.
A literal stored `outcome:"unknown"` is invalid: uncertainty is not a durable
record. Rust refuses to serialize that client-only unresolved lookup state, even
when it was inferred by decoding a stored rejection's literal error code
`"unknown"`. This differs from emitting a stored `outcome:"unknown"` record.
In-window not_found remains unresolved if dispatch may have happened; it never
means an automatic fresh-ID retry. `queued` is deferred to X-05, per X-02 §4.2.
Worker `committed` maps to completed; worker stored rejected maps to rejected;
host journal reserved maps to accepted and linked to completed. A targeted
cancellation returns HTTP 200 `{result:{operation_id,state:"abandoned"|"linked"}}`:
abandoned means cancelled now/earlier; linked means too late, creation completed.
In progress is HTTP 409 with an error body, never a success result:
`{error:{code:"conflict",message:"The create is still in progress; try again.",operation_id,state:"in_progress"}}`.
It can be retried with the same `{}` body, which has no new ID
or params/fingerprint, and does not go through ordinary mutation dispatch. Only
the same account or an instance admin can cancel; others get not_found. Lookup of
the original ID reports the final state. A missing record is not authorization to retry
outside the window. Org/work existing 29-day admission / 30-day retention rules
remain; host journal is 30 days, same ULID skew/age rules. New ops must state their
window. Same account after new sign-in can lookup/replay; another account or
different payload gets `operation_reused`, not an overwritten result.

`CancelResultState` excludes `in_progress`; `GatewayError::create_in_progress`
constructs its conflict body with the original ID. Lookup queries also have private
fields: construct them through checked decoding or `LookupQuery::from_fields`,
then read the IDs through accessors.

Rowan `01M72VD8TD7AAC5MSFV3XVDVDA` freezes a flat lookup record:
`{operation_id,kind,outcome,result?|error?,state?}`. Only organization.create has
state: reserved with accepted, linked with completed/result, abandoned with no
outcome or payload. Abandoned is final not-created, never rejected/completed and
needs no new code. Stored domain refusals have rejected/error; other host ops
(office.restart included) have completed/result or rejected/error without state.
`not_found` and `expired` are errors of the lookup itself. As with other reads,
the HTTP envelope is `{result:<flat record>}` or `{error:<lookup error>}`.
Response decoding distinguishes three cases (Rowan `01M732VV3VSXP5VZGR70ZT7AR6`):
- An unrecognized error **code** inside a known rejected outcome remains rejected
  and final. Retain the raw code and server message; show generic “not done” and
  stop lookup. Its HTTP class cannot be inferred from that future code alone.
- An unrecognized **outcome** becomes Unsupported/update-needed, final for polling
  purposes. Stop lookup; it is neither success nor a rejection.
- The literal domain error code `"unknown"` becomes unresolved outcome unknown,
  including when received inside a rejected lookup record. Keep the original ID
  and continue lookup. This conversion does not permit a literal stored
  `outcome:"unknown"` record.

Shared mutation and lookup vectors cover each of these three cases. They also
pin the cancellation conflict's operation_id/state metadata on the error.

The shared `gateway-lookups.json` includes all six requested host examples and
org/work/msg records, state/outcome errors and unresolved-versus-expired failures.

Reads/session success: `{result:...}`; failures: `{error:{code,message,...}}`.
Mutation outcomes carry operation_id. Envelopes reject ambiguous result+error or
outcome/payload combinations. Unknown future response variants are explicit
unavailable/update states, never success. User messages carry no SQL, secret or
internal path. The JSON domain code is authoritative; HTTP is classification:

| HTTP | Meaning/codes |
| --- | --- |
| 200 | read/session/completed outcome |
| 202 | accepted |
| 400 | invalid_request, invalid_name, cycle, not_empty, not_a_member, owner_protected, checklist_incomplete, unknown_method, clock_skew, unknown_recipient |
| 401 / 403 | unauthenticated pair / authenticated unauthorized; not_authorized |
| 404 | not_found, unsupported route; never SPA fallback |
| 409 | conflict, already_exists, operation_reused, snapshot_required |
| 410 | expired |
| 413 | too_large (body refused before forwarding) |
| 426 | unsupported_version (refresh/update, no mock fallback) |
| 429 | rate_limited (new gateway code) with retry_after_ms; confirmed pre-dispatch rejected, safe to retry after delay |
| 503 | not_ready, unavailable, storage_error, confirmed pre-dispatch timeout |
| 504 | outcome unknown after possible dispatch, including timeout; never rejected or accepted |

Preserve public domain codes plus X-02's new codes; `rate_limited` belongs only
to `org_proto::gateway`. Legacy `not_bound` is not browser vocabulary (Rowan
`01M72VD8S19CZZES5PBG2X3SNQ`): an unexpected worker error is logged as an
internal invariant failure and translated to rejected `unavailable` (503), a
confirmed pre-execution refusal. The JSON outcome is authoritative, never HTTP status
alone. A domain `unknown` code always becomes outcome unknown. A confirmed
pre-dispatch timeout can be rejected503; possible dispatch always means unknown504.
Retries of `not_ready` are bounded and safe only when non-admission is confirmed.
Offline restart/create and arbitrary domain operations are never queued silently.
For organization mutations, §9 overrides the old unexpected-result fallback:
an undecodable or possibly committed result is unknown504, not rejected503.

## 6. Stream and short poll

`POST /events` body: `{subscriptions:[{scope,feed,cursor?}]}`. `feed` initially
`instance_status`, later `organization`, `work`, `messaging`/presence per published
capability. Each cursor is opaque and bound to instance, principal authorization,
scope, feed and incarnation/epoch. Cursor is never a grant/proof/session handle.

SSE `event: frame`, JSON `data` tagged by `type`:

- `ready {subscriptions:[{scope,feed,cursor}]}` acknowledges admitted feeds.
- `change {scope,feed,cursor,event,data}` carries existing typed payload and actor
  where supported. Each feed advances independently; no global total-order claim.
- `resync {scope,feed,reason:snapshot_required|slow_consumer}` invalidates that
  feed cursor; client obtains a new snapshot. Bounded queue then closes on overflow.
- `heartbeat {server_time_ms}` has no replay cursor; interval proposed 15s.
- `session_ended {reason?}` stops all feeds; stream then closes.
- `unavailable {scope,feed,error}` marks a feed unavailable without mock state.

No SSE global `id`/Last-Event-ID is used as a substitute for per-feed cursors.
Expired/foreign/ahead cursors require resync; unauthorized subscriptions are
refused before streaming headers, and rechecked after reconnect/session changes.
Gateway refreshes worker subscription grants before expiry; worker revocation
ends subscriptions. A browser frame never contains worker grant/channel material.

For organization durable feeds, the snapshot response includes `resume_cursor`
encoding exactly its Position P with instance/org/feed/epoch binding. `/events`
accepts it as the organization's cursor, validates current authorization, and
subscribes after P. The snapshot and cursor are captured together on the same
authenticated scope; no window of changes is skipped. Work follows this pattern
when its snapshot endpoint is published. Reconnect preserves order, drops duplicate/old positions, detects epoch
changes and requests snapshot_required. Coalesce reloads, not durable changes.
For nonreplayable host status, subscribe BEFORE list on the same host connection;
publish its baseline before buffered newer events, and re-list after every gap.
An opaque gateway generation cursor cannot claim host journal replay support.
Ephemeral message/presence hints trigger authoritative listings after any gap.

One streaming fetch per origin/instance/principal across eight tabs, owned by a
SharedWorker or tested Web Locks/BroadcastChannel leader. Followers never stream.
Leader loss reconnects from scoped cursors; logout/account switch terminates the
old stream. If no shared coordination, bounded short polls only. `/events/poll`
accepts the same scope/cursors and returns `{frames:[...],retry_after_ms}` with
finite body/count limits and no held request; identical resync/auth semantics.
Concrete queue/frame/poll limits are B1-05/08 choices before shipping, not unlimited.

## 7. Capabilities and rollout

Protected capability view: `{api_version:1,contract_revision:2,instance_id,
operations:[{name,scope,available,reason?}],feeds:[...]}`. Names are the typed
operation names (`organization.create`, `office.restart`, `org.snapshot`, ...)
and feed names, not executable strings accepted by an arbitrary dispatch endpoint.
Availability means the real authorized operation is wired end to end. Missing or
unknown capability means unavailable; UI shows an explicit unavailable state.
Contract publication does not advertise runtime support before B1-05 onward.
Request scopes stay closed. Response scopes preserve future kind objects rather
than refusing the capabilities response or poll batch. A capability with an
unknown scope is unavailable even when its name is known and available is true;
known capabilities in the same response remain usable. An event frame with an
unknown scope (including any ready subscription) becomes Unsupported; other
frames in that poll batch still decode. The shared views/stream vectors pin the
preserved capability object, unsupported frames and mixed known/future batch.

Unknown operation names are unavailable. A higher contract_revision is additive:
ignore unknown fields/capabilities; lower than the client's required revision
means update needed (426 on use). Breaking changes require a new api_version.
Version mismatch → refresh/update. No Tauri invocation or production fake.

## 8. Conformance gate and next decisions

Shared JSON includes valid and invalid cases for: all four outcomes/exclusive
payloads; scope/path and instance mismatch; linked versus foreign lookup; same ID
through native command/change mapping; clock offset; public version privacy;
capability unknown/false; session view (no host handle); cookie-only/missing proof
refusal; unsupported API; distinct unsupported_version reason/error; offline
refusal/possible-send unknown; cursor per feed, epoch/reset/slow consumer; optional
fields and unknown enum compatibility. Typed readers assert expected values and
reject invalid fixtures; mutation probe proves wrong expectations fail.

No live gateway security claim from DTO tests. B1-05/06/08 must exercise real
admission, cross-port cookie attacks, session/stream revocation, limits, eight tabs
and loss/reconnect. Domain vectors remain in org-office/org-work (B1-03).

Decision record (Rowan `01M72QXR241XRPW9QBY098EB06`, ada as cited above):

1. **D1 approved:** additive gateway DTO module in org-proto, routes §2 frozen;
   scope fields explicit, one ID, expected versions in typed params.
2. **D2 decided:** authoritative JSON outcome; completed200, accepted202,
   unknown504; error class table above, rate_limited429 pre-dispatch only.
3. **D3 decided:** minimum instance fields/constant public version as above;
   typed capability names, additive revisions/new API version for breaking changes.
4. **D4 decided:** targeted idempotent cancellation of original create ID (200 result for abandoned/linked,
   409 conflict error with operation_id/state for in_progress),
   no new ID/fingerprint. Q9 held.
5. **D5 approved:** nonreplayable instance baseline/resync and shared stream/poll
   shape; finite queue/frame/poll budgets before shipping in B1-05/08.
6. **D6 M1 snapshot/clock, decided by ada:** minimum organization snapshot with
   opaque resume cursor continuing exactly after its position, no gap/duplicate;
   integer Unix `server_time_ms` in session/heartbeat/all reported server times.
   Add a shared snapshot→changes→reconnect/expired-cursor fixture and clock offset
   vector. Routes frozen by Rowan. Rust DTOs/vectors/reader first, Jules TS
   mirror/reader afterward; both green on one vector set before B1-02 is done.

Architecture: existing Ca of org-proto makes changes additive and narrow; no
host-control/public coupling and no new crates/dependencies. Shared wire types
reference existing org/domain DTOs intentionally; domain policy never imports HTTP
transport behavior. Gateway composition routes to typed host/client capabilities.

## 9. Organization mutation codec (wire revision 2)

This extends the core with Layer A from Rowan's approved mutation-route design
(agents repository commit `7b560c0` plus his session/grant clarifications).
`org_proto::gateway::org_mutations` contains strict DTOs, an explicit native-call
mapping and a pure reply classifier. It adds no router, dispatcher, grant service,
worker rules or operation enablement. Native socket `API_VERSION` stays unchanged.
The browser client still accepts core revision 1; these new capabilities need
revision 2 and their published availability.

Every path below is POST under `/api/v1/organizations/{org}`. The production HTTP
composition owns the fixed path-to-`OrgOperation` table; it must never rewrite a
browser string into a native method. Entity IDs stay in params.

| Path suffix | Native call |
| --- | --- |
| `/teams/create` | `team.create` |
| `/teams/rename` | `team.rename` |
| `/teams/move` | `team.move` |
| `/teams/set-lead` | `team.set_lead` |
| `/teams/delete` | `team.delete` |
| `/teams/add-member` | `team.add_member` |
| `/teams/remove-member` | `team.remove_member` |
| `/teams/set-title` | `team.set_title` |
| `/members/move` | `member.move` |
| `/members/add-person` | `member.add_person` |
| `/members/update-person` | `member.update_person` |
| `/members/remove` | `member.remove` |
| `/members/set-reports-to` | `member.set_reports_to` |
| `/projects/add-team` | `project.add_team` |
| `/projects/remove-team` | `project.remove_team` |

The composition first selects the route and validates the cookie/proof pair.
`OrgOperation::decode` then checks organization scope against the route's org and
session instance, rejects unknown fields and positional records, and reconstructs
a typed native request with `command_id = operation_id`. The numeric native RPC
ID is separate transport correlation. All eleven versioned operations use the
same `ExpectedVersion` type: nonnegative integral JSON values at most 2^53 - 1.
No business ID-shape, name, cycle, ownership or other domain checks are added.

Each optional string accepts absent/null as None and omits that native field.
`clear_contact` defaults to false (omitted in native JSON), preserves true, and
refuses null. Contact together with clear_contact decodes successfully: its
refusal belongs to the worker. A validated `OrgMutation` keeps its scope and
identity private and elides business values in Debug.

The classifier consumes a correlated native reply with a numeric native ID:

- Committed with a position becomes completed200 with exactly `result:{}`.
  The position is an invariant check, never a published resume token.
- Committed without a position, queued/unknown status, an undecodable reply or
  no answer after possible dispatch becomes unknown504. It never becomes
  accepted202 or rejected503. Safe issue tags let the composition log invariants
  without retaining a native payload.
- Confirmed domain refusals retain their public code/message and the §5 class.
  Literal error code unknown remains unknown504. Unexpected native errors,
  including not_bound and unknown native refusal names, become sanitized
  unavailable503 with an invariant issue. Native errors cannot supply gateway
  session, cancellation or identity hints. Rate limiting is gateway-owned.
  The native code allowlist is explicit and exhaustive: codes belonging to
  HTTP admission or other domains (including unsupported_version and timeout)
  cannot pass through with a native message. HTTP admission can still emit
  its own documented errors before dispatch.
- Native not_authorized returns `NeedsAuthorizationRecheck` outside the
  serializable browser outcome type. B1-06 checks host-session liveness. For a
  live session it renews once and redispatches once with the same operation ID,
  regardless of the old grant's expiry. A fresh worker refusal becomes403.
  Renewal refusal never redispatches: host-confirmed session end becomes401,
  a live unlinked organization becomes404, and other failures become503.
  A refusal code alone never proves that the host session ended.

All fifteen operations remain unavailable until their Layer B routing, captured
native-dispatch, production gating and session/authorization tests pass. Three
also retain domain/product gates: member.remove needs the history/people-only
owner decision; project.add_team and project.remove_team need B1-10's area
reconciliation. Per-account ownership informs capability display only; the
worker enforces authorization. A disabled operation must be refused503 before
native dispatch even when a client ignores its capability.

Layer A vectors live in `test-vectors/v2/gateway/`: 16 mapping cases, 233 strict
input/optional cases and 38 reply cases, plus one core outbound-encoding guard.
Each case is tagged Rust or Rust+TS; native reply classification is Rust-only,
while its browser JSON and mapping paths/envelopes are shared with Jules's
reader/URL builder. The Rust reader does not claim HTTP-path routing coverage.
The original eleven core files remain separate; the lookup-query fixture's
explicit contract_revision2 selects nineteen cases (205 core cases total).
Rust's revision constant and pinned public metadata advance together.

B1-05/06 owns Layer B: unknown paths and unsupported methods get JSON404; auth,
scope, strict decode and production enablement precede captured dispatch. It
must exercise all mappings, unknown params on every path, all three production
gates, host-session/grant renewal, hidden-scope404, and unchanged operation IDs
on conflict/replay. No capability may be enabled solely because Layer A passes.
A worker not_found does not itself mean access was lost; an authoritative
snapshot refresh returning404 confirms that separately.
