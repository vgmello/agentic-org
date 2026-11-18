# Web HTTP API v1

The browser's contract with `agentic-org-web`: routes, sessions, outcomes, lookup,
the event stream and capabilities. `agentic-org serve` starts the web process; the
[standalone design](standalone-server.md#5-http-and-browser-contract) places it.
Records, fields and identities are owned by the
[data contract](organization-data/README.md); where an example here disagrees with
it, the data contract wins.

Nothing has shipped, so this contract changes in place: a route is changed to its
selected shape when its target is built, with its conformance vectors in the same
change, and no adapter, version negotiation or second DTO is kept
([no compatibility obligations](organization-data/README.md#no-compatibility-obligations-before-v1-ships)).

## 1. Scope and placement

`/api/v1` covers version and capabilities, browser authentication,
instance/organization/office status, operation outcomes and lookup, multiplexed
feeds, and typed domain operations. It is not a JSON-RPC tunnel: every route maps
to one typed operation.

`org_proto::gateway` holds the transport-neutral browser wire DTOs. Shared
conformance vectors live under `crates/org-proto/test-vectors/v2/`, read by the
Rust tests and by the TypeScript mirror in
`apps/agentic-org-ui/src/lib/platform/http/`. Both readers pass the same vector
set. Vectors are never imported by production code and never become a mock
fallback. `org-host-control` stays private to host, web and setup; the browser
never sees host-control methods or secrets. The `org-proto` local API version,
the launch contract, host-control and HTTP `/api/v1` are separate version spaces.

The word `gateway` in these module names is the code's spelling of "web"; renaming
it is mechanical (implementation plan, L10) and does not change the routes.

## 2. HTTP surface

All JSON responses use `Cache-Control: no-store`, except hashed UI assets. The page
and API share the configured origin. A missing API route is a JSON 404, never SPA
HTML. Unknown operations are never forwarded to a local socket. `HEAD` on a GET
route answers that route's status and headers, through the same pair check, with
no body; on any other route it is `invalid_request`.

| Method/path | Body/result | Access and source |
| --- | --- | --- |
| GET `/api/v1/version` | `{api_version:1, contract_revision}` | Public. Constant protocol metadata only; nothing about the instance, organization or accounts |
| POST `/api/v1/session/sign-in` | `{login,password}` → session view plus proof | Host `session.sign_in`; configured-origin validation, per-login and per-address limits (429) |
| POST `/api/v1/links/register` | `{link,login,password}` → session view plus proof | Public; host `link.register`; redeems a registration link once (§4); per-address limit (429) |
| POST `/api/v1/links/set-password` | `{link,password}` → `{}` | Public; redeems a set-new-password link once and ends every session of that account |
| POST `/api/v1/links/approve` | `{link,decision:"approve"\|"decline"}` → `{}` | Pair; signs a terminal in as this account, or declines (§4) |
| POST `/api/v1/session/password` | `{current_password,new_password}` → `{}` | Pair; ends every other session of the account |
| POST `/api/v1/instance/administrator` | `{account_id,password}` → `{}` | Pair; the administrator only; hands the role over in one operation |
| GET `/api/v1/session` | Session view, without proof | Cookie and proof pair; a proof is never recovered from the cookie alone |
| POST `/api/v1/session/refresh` | `{}` → session view plus rotated proof | Pair; host `session.renew`; both browser values rotate atomically |
| POST `/api/v1/session/sign-out` | `{}` → `{}` | Pair; ends the host session, clears the browser pair and closes streams |
| GET `/api/v1/capabilities` | Capability view (§7) | Pair; authorized scopes only |
| GET `/api/v1/instance` | Instance view (§3) | Pair; host `instance.view` (web connection only); host-derived status, no paths, sockets or credentials |
| GET `/api/v1/organizations` | `{organizations:[...], creating:[{operation_id,name,slug}]}` | Pair; host `organization.list`, asked on every request and never answered from a cache. `organizations` holds the organization views (§3) of the linked organizations only; `creating` holds the account's own creates that have not completed, each followed through the host lookup |
| POST `/api/v1/organizations` | Instance-scoped envelope with the [create payload](#selected-create-organization-payload) → 202 `accepted`, or 200 `completed` with the organization view | Pair; host `organization.create`, the administrator only. The same `operation_id` with the same payload replays the stored answer and never creates a second organization |
| POST `/api/v1/organizations/operations/{id}/cancel` | `{}` → cancellation result (§5) | Pair; host `organization.cancel`; the same account or the administrator; the original create ID |
| POST `/api/v1/organizations/{org}/people/{person}/access` | Envelope with `{action:"invite"\|"new_link"\|"remove"}` → `{link}` for the first two, `{}` for remove | Pair; the organization's owner or the administrator; a link is returned once and never again |
| POST `/api/v1/organizations/{org}/owner` | Envelope with `{person_id,password}` → `{}` | Pair; the owner only; the person must have an account |
| GET `/api/v1/organizations/{org}/offices` | `{offices:[...]}` | Pair; host `office.list` narrowed to an authorized organization |
| GET `/api/v1/organizations/{org}/snapshot` | `{snapshot,resume_cursor}` | Pair; organization snapshot at HQ with a cursor at the same position |
| POST `/api/v1/organizations/{org}/offices/{office}/restart` | Office-scoped envelope with `{}` | Pair; host `office.restart`; the organization's owner or the server's administrator |
| GET `/api/v1/operations/{kind}/{id}` | Scoped lookup (§5) | Pair; `kind=host\|org\|work\|msg`; query fields below, checked before lookup. `GET /api/v1/operations/host/{id}?instance_id=` is the progress lookup of a create (host `operation.lookup`); an account sees only its own operations |
| POST `/api/v1/events` | Subscriptions and cursors (§6) → SSE | Pair; streaming fetch, no proof in the URL |
| POST `/api/v1/events/poll` | Same body → bounded frame batch | Pair; short-poll alternative with no held connection |

Domain routes for tasks and organization changes are in [Tasks](#tasks) and §9.

**Request shape.** Every POST needs `Content-Type: application/json` (an optional
`charset=utf-8` and nothing else); anything else is HTTP 415 `invalid_request`,
also on the public routes, which have no proof. `OPTIONS` on `/api/v1` is HTTP 405
`invalid_request` with `Allow: GET, HEAD, POST`; no response carries an
`Access-Control-*` header.

**Host operations behind the session routes.** The web process reaches the host
only over its own admitted control connection; `host.sock` and every other
connection get `not_authorized` for these calls and nothing is stored. The web
process sends SHA-256 digests of the cookie and proof, never the values, and the
configured origin as `{scheme,host,port}`. Each result is the session record
`{session_id, account:{id,login}, admin, orgs:[{org_id,employee_id,office_ids}],
server_time_ms, idle_expires_at_ms}`.

| Host method | Params | Result and refusals |
| --- | --- | --- |
| `link.register` | `{link, login, password, client_address, cookie_digest, proof_digest, origin}` | Session record. Rule failure: `invalid_request` with `{field, rule}`. Unknown, used, cancelled or expired link: `not_found`, checked before any password hashing, and counted on the client address. Locked address: `rate_limited` with `retry_after_ms` |
| `session.sign_in` | `{login, password, client_address, cookie_digest, proof_digest, origin}` | Session record. An unknown login, a wrong password and a revoked account get one identical `not_authorized`. A locked login or address (IPv6 by /64): `rate_limited` with `retry_after_ms`, before any password check. Host full: `unavailable` |
| `session.validate` | `{cookie_digest, proof_digest, origin}` | Session record, touched. No match, another origin or an idle session: `not_authorized`. Removed access ends every session of the account |
| `session.end` | `{session_id}` | `{}`. Deletes the record, then pushes `session.ended {session_id}` before the reply on the same ordered connection; an unknown ID is not an error |

Every session the host ends, also one reaped as idle or ended by a refusal, is
pushed as `session.ended`. Browser sessions have no absolute lifetime and no
per-connection or per-account cap: only the 30-day idle rule and a host-wide bound
of 4096 live sessions.

**Lookup query.** `instance_id` is always required. `kind=host` forbids `org_id`
and `office_id`; `kind=org` requires `org_id` and forbids `office_id`;
`kind=work|msg` requires both. The web process resolves HQ for `org`; work and
message lookups go to the office that admitted the ID. Empty values, repeated keys
(even with identical values), unknown keys and malformed IDs are HTTP 400
`invalid_request`, checked before authorization. The adapter passes decoded query
pairs to `LookupQuery::from_fields` without collapsing duplicates into a map. The
query's instance must equal the session's instance; a mismatch is the same 400
scope refusal as for mutations and is never forwarded. A hidden scope and a missing
record return the same 404 body, byte for byte. `gateway-lookup-query.json` pins
these rules.

**Snapshot.** The organization snapshot's opaque `resume_cursor` continues exactly
after the snapshot position, with no gap or duplicate. A raw position is not a
browser resume token.

### Selected create-organization payload

`POST /api/v1/organizations` carries what the organization needs
([create-organization design](create-organization.md),
[organization structure](organization-data/01-organization-structure.md)):

| Field | Required | Meaning |
| --- | --- | --- |
| `name` | yes | Display name |
| `slug` | yes | Lowercase kebab-case, unique in the instance's organization catalog; never recalculated after a rename |
| `organization_repository` | yes | Registration of the remote Git repository that holds the organization's files: exactly `{kind:"remote", remote_url}`. Never a secret value. Validation checks syntax only and makes no network request. |
| `state_branch` | yes | The branch organization files are published to. There is no default. |

There is no team field. Creation makes one top-level team named after the
organization, with the creator's person as its lead and the built-in HR lead as
its other member ([create-organization design](create-organization.md)). The
result is the organization view (§3) with `slug`. Connectivity and publication of
the remote repository are reported as unverified until online operation is
authorized.

**Envelope.** `{operation_id, scope:{kind:"instance", instance_id}, params}`, with
`params` the four fields above and nothing else.

**Only a remote repository.** A `kind` other than `remote`, a `file://` address and
a `path` key (present with any value, `null` included, under any kind) are each
HTTP 400 `invalid_request` with `{field: organization_repository, rule:
remote_required}`. A `credential_ref` key is refused the same way with rule
`unsupported`: the reference to an office-held credential is not built yet.

**Who refuses, and how.** The create has two refusal shapes, and they are never
mixed:

| Decided by | Response |
| --- | --- |
| The web process, before any host call (a malformed envelope or operation ID, a payload rule) | HTTP 400 `{error:{code:"invalid_request", message, field?, rule?}}`. `field` and `rule` are present only when one named input is at fault; a malformed envelope, a body that is not JSON or a scope that does not match the path carries neither. Nothing is stored and no host call is made |
| The host | `{outcome:"rejected", operation_id, error}` with the HTTP class of the error's code: 400 (`invalid_request`, `clock_skew`), 403 (`not_authorized`: the account is not the administrator) or 409 (`already_exists`, `operation_reused`) |
| The host, for an operation ID it has never seen that is older than 29 days (the admission window, section 5) | HTTP 410 `{error:{code:"expired"}}`: an error, not an outcome. An ID the host already knows replays its stored state at any age |

A name or a slug that is already taken, by an organization or by a create still in
flight, is 409 `already_exists` whose `field` is `name` or `slug`. The remaining
answers are 202 `{outcome:"accepted", operation_id}` (the host stored the create and
is carrying it out), 200 `{outcome:"completed", operation_id, result}` with the
organization view, and 504 `{outcome:"unknown", operation_id}` when the host is
unavailable, reports a storage failure or its answer is lost: the web link reports
all three as one failure, so no 503 is promised for this route. After a 504 the
browser keeps the ID and either posts the same body again or polls the lookup; it
never mints a new ID for the same intent.

**Progress lookup.** `GET /api/v1/operations/host/{id}?instance_id=` answers
`{result:{operation_id, kind:"host", outcome, state, result?|error?}}`: `accepted`
with `state: reserved`, `completed` with `state: linked` and the organization view,
or `rejected` with the stored error. An operation of another account and an unknown
ID are the same 404. A lookup the host refuses is the lookup's own error and never
a stored refusal of the create: `expired` is HTTP 410, anything else is 503
`unavailable`.

**Operation ID format.** In A4 a host operation ID is a canonical ULID minted by
the browser. Any other format, on the create and on the host lookup, is HTTP 400
`invalid_request` with `{field: operation_id}` (rule `format`) before any host
call. The format becomes UUIDv7 with the other IDs (FC-142).

### Tasks

Task routes are under `/api/v1/organizations/{org}/tasks` and follow the
[task data contract](organization-data/12-task-assignment-and-history.md). Every
route keeps exact `instance_id` and `office_id` query fields; the organization and
task come from the decoded path. Duplicate, missing, unknown and inapplicable query
fields are refused before dispatch.

| Route | Purpose |
| --- | --- |
| GET `.../tasks?kind=status\|list` | Status, or a page of at most 50 cards. `list` takes `queue=all\|owned\|assigned_team\|assigned_employee`, an explicit `target_id` for any queue but `all`, optional `after` and `limit` (1–50) |
| GET `.../tasks/{task}` | Detail |
| GET `.../tasks/{task}/history` | Events, with optional decimal `after` and `limit` |
| GET `.../tasks/{task}/assignment-targets` | `target_kind=team\|employee`, optional `after`, `limit` |
| GET `.../tasks/{task}/content` | One chunk of a content reference: `revision`, `kind=snapshot\|event`, `sha256`, `byte_length`, `offset`, `limit` |
| POST `.../tasks/{task}/assignment` | Set the assignment pair |

Read responses are `{result: ...}`; no native grant enters browser JSON. The native
read is `task.selected.read`, an object query tagged by `kind` (`status`, `list`,
`detail`, `history`, `targets`, `chunk`).

- **Assignment.** The envelope is `{operation_id,scope,params}` with `scope` exactly
  `{kind:"office",instance_id,org_id,office_id}`. `params` holds exactly
  `expected_revision`, `assigned_team_id` and `assignee_id`; both IDs are required
  and may be an explicit `null`. The principal supplies the actor; no actor or
  writer epoch comes from params. The native call is `task.set_assignment`
  `{change_id,task_id,command}`.
- **Revisions are decimal strings** in browser JSON, up to `9223372036854775807`,
  in every card, reference, result and refusal. Decode raw bytes straight into
  strict DTOs; never collapse duplicate keys through a generic JSON value.
- **Outcome.** `completed` with `result:{task_id,task_ref,revision,event_id,
  assigned_team_id,assignee_id,office_id}`: the task's ID and reference, its new
  revision and the identity of the history event it wrote, the assignment pair
  after the change, and the office that admitted it. Or `rejected` with `code`
  and, for a conflict, the current assignment state (organization, office, task,
  revision, both assignment fields) at the decision point. Work lookup of the
  operation returns the same. No codec defines the completed result yet;
  `org_work::selected_assignment::AssignmentPlan` holds the planned organization,
  task, next revision and change.
- **Content references** name the task, exact revision, kind, digest and byte
  length. A chunk is at most 256 KiB of raw bytes, sent as hexadecimal; clients
  concatenate bytes before decoding UTF-8. A chunk request rechecks the task and
  actor scope and never accepts a path or an unrestricted blob URL. A reference is
  not authority.
- **History coverage** that is not established is `unknown`, never invented.
- **Browser retry state.** Before a possible dispatch the browser stores only the
  original operation reference, bound to instance, account, office and task. It
  never keeps a replayable command outbox or proof. After a reload it reconciles
  through lookup; unknown, missing or expired is never permission to mint a new ID
  and repeat the intent.

The `selected_tasks_v1` capability is advertised only when the office's journal,
files, owner binding and host admission are live and verified; the presence of the
codec or configuration does not enable it.

### Protected views

Read only authorized projections of protected chat, usage, budget and recovery
sources. Private task notes stay author-only, with no owner or administrator
exception and no shared receipt that reveals they exist. No generic SQL or lookup
endpoint exposes raw paths, source envelopes, amounts or private result blobs.
Cost views keep complete, partial and unavailable usage distinct and follow the
[cost access rules](../specs/costs-and-insights-ux.md); `stop_pending` is not a
completed hold, and release caps warn only.

## 3. Views and command envelope

Session view (IDs are UUIDv7 text; shortened here):

```json
{"instance_id":"0193…","account":{"id":"0193…","login":"alex"},"admin":true,"organizations":[{"org_id":"0193…","employee_id":"0193…","office_ids":["0193…"]}],"server_time_ms":1794312000000,"idle_expires_at":"2026-12-10T12:00:00.000Z"}
```

`server_time_ms` is integer Unix milliseconds; there is no RFC 3339 alias. The
client validates it as a nonnegative JavaScript-safe integer and uses the offset to
mint operation IDs with server-corrected time. Only sign-in and refresh add `proof`.
Host session handles, grant MACs and channels never appear in a view.

**Instance view:** `{instance_id,name,server_version,web_status,listen_scheme,tls,server_time_ms}`.
`server_version` is the release pin's version; `web_status` is `ok|degraded`;
`listen_scheme` is `https|http_loopback`; `tls` is `ok|missing|expiring`, and is
absent until the instance serves TLS (the loopback listener has none). Unknown
enum values decode to `unknown`. The view has no paths, endpoints, sockets,
handles or certificate material.

**Organization view:** `{id, slug, name, created_at, employee_id, recovery, offices}`.
`employee_id` is the account's person in that organization, `recovery` is
`pending|delivered|acknowledged`, and `offices` lists the organization's offices on
this instance as office views, HQ first. **Office view:** identity, organization,
name, role, host, status, `status_reason` and `next_restart_at`, without the local
endpoint. The recovery code never crosses HTTP. Status reason
`unsupported_version` is distinct from the request error code.

`organization.create` completes with exactly the organization view; `office.restart`
completes with `{}`. Lookup returns the same projection with identical bytes. These
host-result projections, and their lookups, refuse the object keys `endpoint`,
`socket`, `path`, `recovery_code`, `owner_grant` and `host_control`, and every key
ending in `_path`, at every nesting depth including inside arrays. The rule applies
to these host results only, not to domain records such as repository paths.

**Mutation envelope.** The path fixes the typed operation:

```json
{"operation_id":"0193…","scope":{"kind":"office","instance_id":"0193…","org_id":"0193…","office_id":"0193…"},"params":{}}
```

`scope` is a tagged union: `instance {instance_id}`, `organization {instance_id,org_id}`,
`office {instance_id,org_id,office_id}`. A path and envelope mismatch is refused
before dispatch. Identity comes from the session pair, never from a supplied
account, employee or principal. A lookup in a foreign scope is `not_found`; an
absent or bad pair is `not_authorized`.

**One operation ID.** It is a UUIDv7 and an idempotency key, not an ordering key.
The web process passes it on as the host `operation_id`, organization `command_id`,
work `change_id` or `msg.send.id` without minting another. Receipts and revisions
determine server order. Global replay and deduplication belong to the office or
host that admits the operation, so two requests with the same ID, params and
account are safe; a different payload or account gets `operation_reused` (409).
HQ refuses an ID whose time is more than five minutes ahead (`clock_skew`) or more
than 29 days old (`expired`). The browser keeps no operation registry or outbox. An
`expected_version` or `expected_revision` stays in the operation's typed params.
Fields that would let a caller choose an agent's home (`home`, `clear_home`,
`profile.home`) are refused on presence, including `null`.

**Paths.** Raw path segments are validated before routing, without normalizing.
Encoded `%2f` or `%5c` in any case, a raw backslash, empty segments and `.` or `..`
segments (including `%2e` spellings) are 400 `invalid_request`. Clients
segment-encode IDs. A raw `%252f` is a literal percent escape: decode at most once.
The HTTP library truncates a literal `#` and what follows before routing; validation
and authorization use that same retained path, and the discarded bytes never select
a route or file. A raw wire regression pins this.

**Not found and refusals.** A hidden and a missing resource get the same
`not_found`. A visible resource the caller cannot change is 403 `not_authorized`.
A confirmed `not_found` on submit means nothing was applied. A grant or scope
refusal is not stored (lookup returns `not_found`); a rule refusal is stored and
replays. The client keeps the draft, never resubmits automatically, and uses a new
ID only for a new attempt the person chooses. If the submit reply was lost, a
lookup `not_found` stays unresolved.

## 4. Browser sessions and failures

[ADR-0013](../adr/0013-stored-browser-sessions.md) owns session storage and its
criteria SES-1 to SES-8. In short: the host stores each browser session as a record
holding digests of the cookie and proof; the web process keeps an in-memory cache,
validates an unknown cookie with one host call, and stores nothing. A restart of
either process, an update or a lost control connection does not end a session.
Sign-out, expiry, a password set from a link and removed access end it at once,
and a password change ends every other session of the account: the host
deletes the record, then tells the web process to drop the cache entry and close
its streams. The host owns accounts, password checks, expiry, grants and
revocation; the office owns domain authorization. The web process opens no database
and holds no key.

**Cookie and proof.** The cookie name is instance-specific; `HttpOnly`,
`SameSite=Strict`, `Path=/api/v1`, no `Domain`, and `Secure` over HTTPS. An explicit
loopback HTTP configuration may omit `Secure`; the pair is still required. Each
value has 256 bits of entropy; the proof is 64 lowercase hexadecimal digits. Every
protected read, command, stream, poll, refresh and sign-out needs the cookie and
`X-Org-Session-Proof`, bound to the configured scheme, host and port, the instance
and the account. No wildcard credentialed CORS, secret in a URL, cross-origin
redirect or cookie-only recovery. The web process checks the configured Host and
the browser Origin; when Origin is absent on a same-origin GET, the valid proof
carries the origin binding.

**Accounts and principals.** Every account is tied to a person in each
organization it can open; the session view's `employee_id` for an organization is
that person, and the principal the office receives carries it, so what the account
does there is recorded under that person's name. An account is never tied to an
agent, and an agent never holds a browser session. One account carries the
administrator flag; `admin: true` in the session view marks it. Accounts are
created only by redeeming a registration link; no route creates one from a
supplied login.

**One-time links.** Registration, set-new-password and terminal-approval links are
made by the host, which keeps only a digest of each. The secret travels after `#`
and the page sends it in a JSON body, so it never appears in a request line, log or
redirect. A link works once and expires (24 hours for registration and
set-new-password, 10 minutes for approval); a used, expired or cancelled link is
HTTP 404 `not_found` with no detail about the account.

Credentials go straight into redacting, zeroizing secret types and are never
logged. Only sign-in, the link routes, password change and the two hand-over
routes accept a password. An unknown login and a wrong password get
the same `not_authorized` shape. Sign-in limits may return `retry_after_ms` and
never reveal whether an account exists.

A protected request with no valid session is HTTP 401 `not_authorized`, with the
optional safe `session_reason: session_ended`. An authenticated request the person
may not make is HTTP 403 `not_authorized`. Neither is a stored business rejection.
After sign-in nothing is resent automatically. Signing in again as the same account
keeps drafts; switching account clears the old account's views in every tab. A lost
transport during a mutation can still mean unknown: the end of a session does not
prove that an earlier operation failed, and lookup after sign-in uses the original
ID.

## 5. Outcomes, errors and lookup

Mutation response:

```json
{"operation_id":"0193…","outcome":"accepted"}
{"operation_id":"0193…","outcome":"completed","result":{}}
{"operation_id":"0193…","outcome":"rejected","error":{"code":"conflict","message":"The item changed."}}
{"operation_id":"0193…","outcome":"unknown"}
```

- `accepted` requires a durable record at the owner binding the ID, fingerprint,
  account, scope and outcome. A queue in the web process's memory is not enough.
- `completed` proves the owner committed.
- `rejected` is a confirmed refusal. It is never used for a timeout or a lost
  connection after a possible dispatch.
- `unknown` is unresolved. The UI keeps the ID and looks it up; it never mints a
  fresh ID automatically. A client may produce `unknown` locally on transport
  failure.

There is no `queued` outcome. Requests to an offline office follow the operation's
own stated queue-or-refuse rule; arbitrary commands are never queued.

**Lookup** returns a flat record `{operation_id,kind,outcome,result?|error?,state?}`,
wrapped as `{result:<record>}`, or a lookup error `not_found` or `expired` as
`{error:...}`. A stored `outcome:"unknown"` is invalid: uncertainty is not a durable
record, and the Rust types refuse to serialize it. A `not_found` within the window
stays unresolved if dispatch may have happened. Office `committed` maps to
`completed`; a stored office refusal maps to `rejected`; the host journal's
`reserved` maps to `accepted` and `linked` to `completed`.

Only `organization.create` has `state`: `reserved` with `accepted`, `linked` with
`completed` and its result, and `abandoned` with no outcome or payload, which means
final and not created. Cancelling a create returns HTTP 200
`{result:{operation_id,state:"abandoned"|"linked"}}`; `linked` means it was too
late. While the create is still running, cancel returns HTTP 409
`{error:{code:"conflict",message:"The create is still in progress; try again.",operation_id,state:"in_progress"}}`
and may be retried with the same empty body. Only the same account or an instance
administrator may cancel; anyone else gets `not_found`.

Organization and work operations are admitted for 29 days and retained 30; the host
journal keeps 30 days. A new operation states its window. The same account after a
new sign-in can look up and replay; another account or another payload gets
`operation_reused`.

**Decoding rules**, each pinned by shared vectors:

- An unrecognized error code inside a known `rejected` outcome stays rejected and
  final. Keep the raw code and message, show a generic "not done", stop lookup.
- An unrecognized outcome means update needed, final for polling. It is neither
  success nor rejection.
- The literal domain error code `"unknown"` becomes outcome `unknown`, even inside a
  rejected lookup record. Keep the ID and continue lookup.

Reads succeed as `{result:...}` and fail as `{error:{code,message,...}}`. An
envelope with both result and error, or a mismatched outcome and payload, is
refused. User messages carry no SQL, secret or internal path. The JSON code is
authoritative; the HTTP status is a classification:

| HTTP | Meaning and codes |
| --- | --- |
| 200 | Read, session or completed outcome |
| 202 | accepted |
| 400 | invalid_request, invalid_name, cycle, not_empty, owner_protected, checklist_incomplete, unknown_method, clock_skew, unknown_recipient |
| 401 / 403 | No valid session / signed in but not allowed: not_authorized |
| 405 | invalid_request: a method the API does not serve, such as `OPTIONS`, with `Allow` |
| 404 | not_found, unsupported route; never the SPA fallback |
| 409 | conflict, already_exists, operation_reused, snapshot_required |
| 410 | expired |
| 413 | too_large, refused before forwarding |
| 415 | invalid_request: a POST whose `Content-Type` is not `application/json` |
| 426 | unsupported_version: refresh or update; never mock data |
| 429 | rate_limited, with `retry_after_ms`; refused before any password check or dispatch, safe to retry after the delay |
| 503 | not_ready, unavailable, storage_error, or a timeout confirmed before dispatch |
| 504 | Outcome unknown after a possible dispatch, including a timeout; never rejected or accepted |

`rate_limited` comes from the web process's own limits or from the host's sign-in
and registration throttle.

**Error body.** `{error:{code, message, field?, rule?, retry_after_ms?}}`. The
optional fields appear only when set: `field` and `rule` name the input and the
rule it failed on a 400 `invalid_request` (`login`: `format`, `taken`;
`password`: `too_short`, `too_long`), and `retry_after_ms` is the wait in
milliseconds on a 429. An unexpected office error is logged as an
internal invariant failure and returned as rejected `unavailable` (503) only when
nothing ran; anything that may have committed is unknown (504). Retries of
`not_ready` are bounded and safe only when non-admission is confirmed.

Internally, the office's mutation receipts may hold an `outcome_unknown`
reconciliation state
([recovery contract](organization-data/16-recovery-and-office-continuity.md)). The
public API maps it to the unresolved behaviour above without asserting a commit or
causing another side effect.

## 6. Stream and short poll

`POST /events` takes `{subscriptions:[{scope,feed,cursor?}]}`. Feeds are
`instance_status`, `organization`, `work` and `messaging` with presence, each
available when its capability is published. A cursor is opaque and bound to the
instance, the principal's authorization, scope, feed and epoch. A cursor is never a
grant, proof or session handle.

SSE `event: frame` carries JSON `data` tagged by `type`:

- `ready {subscriptions:[{scope,feed,cursor}]}` acknowledges the admitted feeds.
- `change {scope,feed,cursor,event,data}` carries a typed payload and, where
  supported, its actor. Each feed advances on its own; there is no global order.
- `resync {scope,feed,reason:snapshot_required|slow_consumer}` invalidates that
  feed's cursor; the client takes a new snapshot. A full per-client queue sends this
  and then closes.
- `heartbeat {server_time_ms}`, about every 15 seconds, with no cursor.
- `session_ended {reason?}` stops every feed; the stream then closes.
- `unavailable {scope,feed,error}` marks one feed unavailable, without mock state.

There is no global SSE `id` or `Last-Event-ID`. Expired, foreign or ahead cursors
need a resync. Unauthorized subscriptions are refused before streaming headers and
rechecked after a reconnect or session change. The web process renews its office
subscription grants before they expire, and revocation at the office ends the
subscription. No frame carries a grant or channel material.

**No gap between snapshot and stream.** The snapshot response's `resume_cursor`
encodes its position with the instance, organization, feed and epoch, and `/events`
accepts it and subscribes after that position. Snapshot and cursor are taken
together in the same authorized scope. Reconnect keeps order, drops duplicates and
old positions, and turns an epoch change into `snapshot_required`. Coalesce
reloads, never durable changes. For host status, which cannot be replayed, the web
process subscribes before it lists on the same host connection, publishes the
baseline before newer buffered events, and lists again after every gap. Message and
presence hints trigger an authoritative listing after a gap.

**One stream per origin.** One streaming fetch per origin, instance and principal
serves every tab, owned by a SharedWorker or a tested Web Locks/BroadcastChannel
leader. Followers never stream. Leader loss reconnects from the scoped cursors;
sign-out or an account switch closes the old stream. Without shared coordination,
the client uses `/events/poll`, which takes the same body and returns
`{frames:[...],retry_after_ms}` with finite limits and no held request. HTTP/1.1's
small per-origin connection pool must keep room for commands; HTTP/2 is not assumed.
Queue, frame and poll limits are finite and fixed before release.

**Client reconnect.** After a disconnect or `snapshot_required` the client
reconnects, is authorized again, takes a new snapshot and resumes from its
position. It waits 1, 2, 4, 8, 16, then 30 seconds between attempts, resetting the
sequence only after a connection has lasted at least 10 seconds, so an immediately
closed stream never causes a one-second loop. A person's Retry shortens the
remaining wait to one second, never zero. Loss of liveness is reported once, not
on every failed attempt. An unknown or malformed event payload does not end the
stream.

## 7. Capabilities

The capability view is
`{api_version:1,contract_revision,instance_id,operations:[{name,scope,available,reason?}],feeds:[...]}`.
Names are typed operation and feed names (`organization.create`, `office.restart`,
`org.snapshot`, …), not strings accepted by a dispatch endpoint. `available` means
the real authorized operation works end to end. A missing or unknown capability is
unavailable, and the UI shows that. A capability whose scope kind is unknown is
unavailable even if its name is known; other capabilities in the response stay
usable. An event frame with an unknown scope is unsupported, and the other frames
in that poll batch still decode.

`contract_revision` grows when capabilities or fields are added; clients ignore
unknown fields and capabilities. A revision lower than the client requires means
update needed (426 on use). A breaking change before v1 ships is made in place,
with the UI and vectors in the same change.

## 8. Conformance

Shared JSON vectors cover, with valid and invalid cases: the four outcomes and their
exclusive payloads; scope, path and instance mismatches; linked versus foreign
lookup; the one-ID mapping to native commands; the clock offset; the privacy of
`/version`; unknown and false capabilities; the session view without host handles;
cookie-only and missing-proof refusal; unsupported routes; `unsupported_version` as
reason and as error; offline refusal and possible-send unknown; per-feed cursors,
epochs, resets and slow consumers; optional fields and unknown enum values. Typed
readers assert the expected values and reject invalid fixtures, and a mutation
probe proves a wrong expectation fails.

DTO tests make no security claim about the live web process. Security acceptance
exercises real admission, cross-port cookie attacks, session and stream revocation,
limits, eight tabs, and loss and reconnect.

The web DTO module in `org-proto` refers to domain DTOs on purpose; domain policy
never imports HTTP transport behaviour. The web process routes to typed host and
office capabilities.

## 9. Organization mutations

Organization changes are POST routes under `/api/v1/organizations/{org}`. The web
process owns a fixed table from path to typed operation and never rewrites a
browser string into a native method. Entity IDs travel in params.

`org_proto::gateway::org_mutations` holds strict DTOs, the native-call mapping and a
pure reply classifier. The route set follows the
[organization structure](organization-data/01-organization-structure.md): teams are
created, renamed, moved, given a lead and archived; an employee is added, updated,
moved to another home team, given a manager and removed; a project's accountable
owner team is transferred separately from editing its contributing teams. There
are no membership routes: an employee has exactly one home team, and no request can
leave an employee without one.

**Decoding.** The web process first selects the route and validates the session
pair. The decoder then checks the organization scope against the route and the
session's instance, refuses unknown fields and positional records, and builds a
typed native request with `command_id` equal to `operation_id`. Every versioned
operation uses one `ExpectedVersion` type. No domain checks (ID shape, name, cycle,
ownership) happen in the web process. An optional string accepts absent or `null`
as none. A boolean clear flag defaults to false and refuses `null`; a value and its
clear flag together decode, and the office refuses them. A decoded mutation keeps
its scope and identity private and hides business values in debug output.

**Reply classification:**

- Committed with a position becomes completed (200) with exactly `result:{}`. The
  position is checked, never published as a resume token.
- Committed without a position, an unknown status, an undecodable reply, or no
  answer after a possible dispatch becomes unknown (504), never accepted or
  rejected.
- A confirmed domain refusal keeps its public code, message and class from §5. An
  unexpected native error becomes sanitized `unavailable` (503) with an invariant
  log. The allowlist of native codes that pass through is explicit; codes belonging
  to HTTP admission or other domains never pass through with a native message.
- A native `not_authorized` makes the web process check whether the host session is
  alive. If it is, the web process renews once and dispatches once more with the
  same operation ID. A second refusal is 403. If renewal is refused, a
  host-confirmed session end is 401, an organization the account can no longer open is 404, and anything
  else is 503. A refusal code alone never proves the session ended.

Each operation stays unavailable, and is refused with 503 before native dispatch,
until its routing, dispatch, production gate and session tests pass. Per-account
ownership informs the capability display only; the office enforces authorization. A
worker `not_found` does not by itself mean access was lost; a snapshot refresh
returning 404 confirms that.

## What the code does today

- `org_proto::gateway` implements the DTOs, outcome classifier, session and stream
  types and the organization mutation codec, with vectors in
  `crates/org-proto/test-vectors/v2/`.
- Organizations (A4): `POST /api/v1/organizations`, `GET /api/v1/organizations` and
  `GET /api/v1/operations/host/{id}?instance_id=` are served by
  `apps/agentic-org-web/src/api.rs` over the host's `organization.create`,
  `organization.list` and `operation.lookup`, which the host answers on the admitted
  web connection only. The browser DTOs are `org_proto::gateway::organizations`
  (`SelectedCreateParams`, `SelectedOrganizationView`, `SelectedOrganizationList`,
  `host_operation_id`), pinned by `test-vectors/v2/gateway-organizations.json`
  (18 cases). The list always asks the host.
- The UI's create page and organizations page read these routes through
  `apps/agentic-org-ui/src/lib/platform/organizations` (A4.5); the UI's older
  organization readers (`readOrganization`, `readOrganizationList`,
  `readCreateRequest`) are deleted, and its lookup reader accepts `-` in a lookup
  ID. The older Rust gateway `OrganizationView` (`gateway/views.rs`, with
  `OrganizationCreateOutcome`, `OrganizationCreateLookup` and the gateway
  `OrganizationList`) still exists: only `org-proto`'s own vector tests
  (`gateway/tests.rs`) use it, and no route, host call or UI reader does.
- `POST /api/v1/organizations/operations/{id}/cancel` is not served (JSON 404): no
  host call cancels a create, so a reserved name and slug stay reserved until the
  create completes (FC-188, deferred). The lookup state `abandoned` of §5 is
  therefore never produced.
- The host pushes `account.changed` on the web connection when what an account can
  open changes (an organization it created was linked). The web process then drops
  that account's cached sessions, so the next request validates at the host and
  sees the new organization and person. A session validated before the event is
  not cached after it.
- The mutation codec carries the older membership operations
  (`team.add_member`, `team.remove_member`, `team.set_title`, `member.move`,
  `project.add_team`, `project.remove_team`) and the name `member`. They are replaced
  by the routes of §9 when the organization model is built (implementation plan B2).
  The 400 class still lists `not_a_member`, which the older membership operations
  still return.
- Operation IDs are ULIDs (`crates/org-proto/src/ulid.rs`); they become UUIDv7
  under FC-142.
- The task codec (`selected_tasks_v1`) exists in the office's acceptance store; no
  route reaches it.
- The host stores browser sessions in its registry behind the `BrowserSessions`
  contract (`crates/org-host/src/sessions.rs`) and answers `link.register`,
  `session.sign_in`, `session.validate` and `session.end` on the web process's
  control connection only (`crates/org-host/src/service/authentication/browser.rs`).
  The web process (`apps/agentic-org-web/src/api.rs`) serves `GET version`,
  `POST session/sign-in`, `POST links/register`, `GET session`, `POST session/sign-out`
  and `GET organizations` over them. It mints the cookie and proof, sends only their
  digests and the accepted socket's peer address (never a forwarding header), and keeps
  validated sessions in an in-memory cache bound to its control connection; the host's
  `session.ended` push drops an entry. `GET instance` validates the pair, then asks the host's
  `instance.view` (web connection only) for the name and web state; it omits `tls`
  while the listener is loopback HTTP. `HEAD` serves the GET routes without a body.
  The pair is minted from the system generator (`getentropy`): the sandboxed web
  process cannot open `/dev/urandom`. `session/refresh`, the other link and password
  routes, events and the other domain routes are still JSON 404. Every POST needs JSON
  (415 otherwise); `OPTIONS` is 405; `Sec-Fetch-Site: cross-site` is 403; host
  refusals carry `field`/`rule` and `retry_after_ms` through to the error body. Real-
  process tests over loopback TCP prove registration from a link, sign-in, restart
  survival (SES-1, SES-2), sign-out while cached (SES-3), one validate then memory
  (SES-5) and that every ingress refusal reaches no host op
  (`crates/org-host/src/web_process_tests/browser_sessions.rs`). Native
  command-line sessions stay in memory and end with their connection.
