# Design: standalone Agentic Org server (v1)

Status: **proposed implementation design**, 2026-11-09. The product decisions were
made by the owner in the architecture discussion; wire formats, platform adapters
and release mechanics still require review and implementation. Owner: ada.
Base inspected: `v1` at `7a8827875e41cac45b75cbf170d6703417652379`.

Implements [server installation and access](../specs/server-installation-and-access-ux.md)
and [ADR-0005](../adr/0005-standalone-server-v1.md). The
[removal inventory](standalone-server-removal-inventory.md) is the implementation
audit. This is a direct pivot of an undeployed application: remove the superseded
paths rather than maintaining a desktop compatibility layer or importing old data.
Implementation ordering below describes dependencies, not a migration rollout.
The owner-requested [project structure](project-structure.md) adds target application
names, domain modules containing vertical slices, shared-code placement, and a
source-to-target reorganization plan under the [architecture rules](../architecture-rules.md).

## 1. Architecture and evidence

```mermaid
flowchart TD
    Browser[Browser: static SvelteKit application] -->|HTTPS API and events| Gateway[org-gateway: instance browser HTTP]
    Gateway -->|Authorized local operations| HQ[org-server: HQ office]
    Gateway -->|Restricted create/list operations| Instance
    HQ --> HQWork[HQ storage and local agent supervision]
    Sat[Satellite office] -->|Outbound authenticated duplex connection| HQ
    Sat --> SatWork[Satellite storage and local agent supervision]
    Agents[Satellite agents] --> CLI[org-agent]
    CLI -->|Local authenticated protocol| Sat
    OS[launchd or systemd] --> Instance[Instance host supervisor]
    Instance --> HQ
    Instance -->|Supervises| Gateway
    Setup[Setup and local lifecycle CLI] --> OS
```

HQ and satellite are persisted **office roles**. An instance is the local service,
its catalog and configuration; it may host several offices. An office belongs to
one organization. A browser session, an instance administrator, an office identity
and an employee identity are distinct principals. A certificate for one office is
never a human owner session or permission to administer the whole instance.

The same release supports both roles. HQ serves the frontend and browser APIs for
its organization, including the synchronized view of its satellites. Multiple HQ
offices in one instance may share one browser listener, with explicit organization
routing and authorization. An instance containing only satellites has no public
application listener. It does not automatically become HQ when the connection fails.

The inspected branch already has a static SvelteKit adapter, a local host, per-office
processes, SQLCipher storage, Unix-socket clients and runtime contracts. It does
**not** have the proposed HTTP server or the `org-sync`, `org-sync-proto` and
`org-pki` crates. `apps/org-office-host/src/sync.rs` is a mutex utility, not office
replication. Browser mode currently selects mocks in office/org/work API factories;
chat always selects its mock. Static asset serving alone cannot make these real.

## 2. Release, install and runtime boundaries

Use one user-facing executable, provisionally `agentic-org`, for setup and local
lifecycle commands. The bootstrap only resolves the platform/version, downloads
and verifies that executable and invokes setup. It is small enough to audit; it
does not own office provisioning logic. Existing installs are discovered before
activation, so re-running bootstrap cannot silently update every instance.

**Proposed packaging:** the release executable carries the static asset/server
payload and platform-matched internal host, office and `org-gateway` helpers plus the small
`org-agent` helper. It unpacks verified payloads into an immutable versioned
release directory. One download may yield multiple local executables. This retains
the existing process and signing boundaries and the tiny CLI instead of requiring
every agent tool invocation to start a universal server program. `org-relay` is a
legacy alias, not a v1 installation prerequisite. Exact release sizes are measured.

Retain the host and per-office worker processes. The service manager starts the
resolved host executable for an instance; foreground `serve` uses the same entry
point. Setup is not an extra long-running supervisor. Avoid installing from an
agent turn or downloading dependencies inside normal daemon startup.

The server implementation may use a small HTTP adapter crate (Axum/Tokio is a
candidate, not an existing dependency). Libraries own behavior; binaries compose
them. Do not link one app's library from another app to assemble the release.
Extract shared behavior into a crate when a real second caller or a meaningful
dependency/visibility boundary justifies it.
Embedding a built executable as a release payload is packaging, not an app-to-app
Rust dependency. Rename the retained browser application to `apps/org-ui/` with
frontend package name `org-ui`, and the Rust server app to `apps/org-server/`
with Cargo package/executable `org-server` (Rust library import `org_server`).
Delete the Tauri-only shell according to the inventory. Keep licensed Tailwind Plus
sources inside the renamed product UI application and update AGENTS.md's path
references with implementation; its license restrictions remain in force.

This renames the worker, not the office model: `org-office-host` still supervises
one worker per office with the existing private launch/key boundaries. The
user-facing setup/release executable above remains provisionally `agentic-org`.
The [project structure](project-structure.md) distinguishes current audited paths
from target names and lists affected workspace, tests, build and packaging references.

The first release must verify download authenticity, executable signatures where
applicable, checksums of embedded helpers and architecture compatibility. Extract
to a private staging directory; reject path traversal and symlink redirection;
publish a complete release atomically. A release pin selects setup, host, worker, gateway, assets
and `org-agent` together. The short bootstrap trusts a configured release signing
key; a checksum downloaded from an unauthenticated source is not sufficient.

### Browser listener and privilege boundary

**Chosen topology:** one new `org-gateway` helper (`apps/org-gateway/`, planned)
per instance terminates browser HTTP/TLS, serves `org-ui` assets and manages browser
sessions/events for all authorized HQ organizations on that listener. It runs as
a supervised child of `org-office-host`, separately from each `org-server` worker.
An instance with no HQ has no public browser listener. Office enrollment/mTLS still
terminates at its reviewed federation component, not this browser proxy.

The gateway receives browser-session/TLS material and narrowly scoped host/office
clients, never office database keys or the host's key-access entitlement. It does
not open office databases. The host retains catalog locking, credential custody,
worker provisioning and private launch. Typed local operations preserve actor,
instance/organization scope, operation ID and expected versions; they are not an
unrestricted JSON-to-host tunnel. Private peer admission and platform filesystem/
process controls must enforce this separation within the stated same-UID limits.

**Create organization** requires an authenticated **instance administrator**.
Owning one organization alone grants no right to create another. Local protected
setup establishes the first instance administrator and creates the first HQ;
later browser creation enters through the gateway's restricted host operation.
The host validates the admitted gateway capability and administrator grant, bound
to the protected instance administrator identity and requested action (never a
browser-supplied role flag), before provisioning, creates the key itself and returns an operation result, never a key.
The same human may hold both roles, but office certificates and employee tokens
grant neither. Tests cover two HQs on one listener and rejected owner-only creates.
**Add office on this instance** (OFC-2) also provisions a key and a worker
on the host, so it needs the same instance-administrator grant; owning the
organization is not enough. Inviting, approving or removing a satellite stays open
to any authorized owner (OFC-7); OFC-7's "add" for the same-instance case needs a
spec follow-up. The gateway op is an open point for X-02 §12 Q9 (a draft not yet in this repo; see office-host-boundary.md §3, "Pending: launch v3"). (2026-11-10,
post-merge review question.)

### Setup executable package identity

Use Cargo package `org-setup` (`apps/org-setup/`, planned) for the setup/lifecycle
binary provisionally named `agentic-org`. The inspected Tauri shell already uses
Cargo package `agentic-org`. Keep the new package ID distinct, and remove the old
Tauri workspace member before building/staging the new binary of the same name.
Do not build both into the same output path or package a stale Tauri executable.
This is a buildable source-change ordering requirement, not a compatibility layer.

## 3. Instance selection and storage

An omitted `--name` selects `default`. `--name lab` selects exactly `lab`. Normalize
and validate names consistently across CLI, registry and service names, using a
restricted portable alphabet and rejecting traversal. The instance has a random
immutable ID separate from that name. Renaming and data relocation are not v1 UI
features. A copied directory is not automatically a newly enrolled office.

Proposed default data roots:

| Platform | Instance data root | Runtime/service details |
| --- | --- | --- |
| macOS | `~/Library/Application Support/agentic.org/instances/<name>/` | Per-user launchd label derived from instance ID; short private runtime socket directory |
| Linux | `${XDG_DATA_HOME:-~/.local/share}/agentic-org/instances/<name>/` | systemd user unit derived from instance ID; sockets under a validated private runtime directory |

Setup may accept an explicit absolute data directory. It saves the resolved path;
services and agents never infer it from a later shell's cwd or HOME. Instance
configuration is server-managed and readable only by authorized local users. It
names office IDs, release pin, endpoints and references to credentials, not inline
private keys. SQLCipher databases, workspaces, logs and setup journal live under
the selected root; binaries live outside it. File modes remain private.

Keep office control state in `<instance root>/office-state/`, with each office at
`office-state/offices/<office id>/`. This subtree is `<office home>` (or `<home>`)
in the retained host/session contracts. Employee homes are server-provisioned
siblings at `<instance root>/agents/<office id>/<member id>/`. Both belong to the
instance, while employee homes remain outside the control-state subtree as the
session security rules require. Task worktrees keep their existing office-owned
`workspace/projects/` layout. The instance lock protects the full root; the host
and office locks also protect their own state. None of these paths is browser-editable.

Do not put long data-root paths directly into macOS Unix-socket names: the existing
103-byte limit still applies. A private short runtime directory uses an opaque
instance handle plus office handles. Authenticate the endpoint as well as checking
path ownership; changing a socket location must not weaken agent binding.

Before reading mutable state, hold an instance lock on the actual data directory.
Retain each office's lock and its single-writer invariant. Resolve/check aliases
and refuse symlink replacement so two names cannot operate one directory. Setup
also holds a name/registry lock until it has durably reserved the ID/root. Socket,
port and service collisions fail explicitly. Never silently select another port
on restart because HQ's saved address is part of enrollment configuration.

Credential names, key sweeps, cleanup, logs and process ownership include instance
and office identities. A host only sweeps its own catalog; the previous account-wide
orphan sweep cannot delete another instance's keys. Agent homes are scoped by stable
office and member IDs within the instance. Office storage and configuration are not globally
agent-readable just because everything is under one root. OS-user sharing is not
a security isolation boundary between instances; stronger isolation needs separate
users/containers. The runtime sandbox/permission model must still enforce its denies.

## 4. Setup and service lifecycle

Setup is a resumable state machine. For an HQ: reserve instance, verify release,
write config, establish the instance administrator, provision the office, register
service, start, probe. For a satellite: reserve instance, verify release, write
config, register service, start, probe, then hand the invitation to the running host
with `office.join` on `host.sock` and poll its status (`awaiting_approval` for
non-interactive setup). `office.join` is gated by the signed `agentic-org` peer and
the destination consent setup captured, plus an administrator session when the
instance has accounts. On a satellite-only instance the service host creates its
account registry database (X-02 §6.3; a draft, see the launch v3 note in office-host-boundary.md §3) at its first start, with the op journal and
no accounts (X-02 §12 Q9).
The satellite worker generates and keeps the TLS identity key; see the offices design. Each irreversible identity/provisioning step has a durable operation
ID and completion marker before another invocation may advance it. The satellite
worker persists its TLS identity key, request and invitation key in its `office.db`,
and writes `office.json` (`state: pending`), before sending the request; after an ambiguous reply query that
request's outcome rather than minting a second office. Resume the same pending
approval after restart. If setup re-runs `office.join` for its own pending office, gets
`already_exists`, and the status read shows that `office_id` as a pending join, it shows the existing request and check code (JOIN-6) instead of
failing. An explicit mode/config change on existing state is refused
with instructions; setup does not silently convert roles.

Interactive and automated input feed the same validation layer. Noninteractive
mode has no hidden prompt; bounded waiting can finish with an `awaiting_approval`
result. Error codes distinguish invalid input, permission failure, address conflict,
unsupported version, unavailable credential provider and incomplete enrollment.
Secrets use a protected descriptor/file or dedicated stdin input. Normal JSON
status, diagnostics, journals and argv never contain them.

macOS uses a per-user launchd registration independent of SMAppService/Tauri.
Linux supports a systemd user service and foreground execution for environments
without it. A service unit/label resolves to one saved instance ID, root and pinned
release, with an explicit runtime PATH/environment. Reconcile service registration
idempotently; do not take over an unrelated unit with a colliding name. Report
login/session/lingering requirements instead of claiming unconditional boot startup.

`start`, `stop`, `status`, `logs`, `doctor`, `upgrade`, `uninstall` and `serve`
select the same instance. Local lifecycle control works when HTTP is down. An
explicit stop must not trigger the service manager's crash-restart policy. Stop
admission, cancel ephemeral streams, drain writes and stop/reap owned runtime trees
within deadlines. A disconnected browser does not cancel durable submitted work.
Host death retains the existing office-stop/reconciliation behavior; supervise
children rather than leaving agents orphaned.

Release pins are per instance. An explicit upgrade stages/verifies a new complete
release, stops that instance, handles supported new-format schema changes, changes
its pin and probes readiness. Retain the prior release and a recoverable data state.
Never roll back only the executable after an incompatible schema change. Shared
release garbage collection checks all instance pins and running references. No
desktop-data migration or old IPC support is built. Uninstall preserves data;
explicit deletion is scoped, checks the stopped state and cannot remove another
instance's directory or shared release.

## 5. HTTP and browser contract

Serve static assets and `/api/v1/` from the same HQ origin. Hashed assets can be
cached; the entry document is revalidated. SPA fallback applies only to browser
navigation routes, never missing API paths, stream routes or assets. Static file
handling cannot traverse into instance data. The frontend does not need SSR or a
Node server. API/frontend version mismatch gives a refresh/update state rather
than falling back to mock data.

The first HTTP adapter must inventory the existing organization, work, messaging,
process and session contracts. Keep typed domain operations, error codes and
authorization checks; no unrestricted JSON-to-local-RPC tunnel. Remove Tauri
invoke/listen paths and production mock selection when wiring each HTTP caller.
Tests can inject explicit fakes. Mock reducers are not authoritative server rules.

Use HTTP requests for reads/commands and a shared authenticated SSE connection for
ordinary server-to-browser events. The browser transport coordinator owns at most
one streaming fetch per browser origin across login/session changes, bound to the
active instance/principal and multiplexing authorized
organization feeds with separate cursors. Tabs subscribe through a SharedWorker;
a Web Locks/BroadcastChannel leader is an acceptable tested fallback. Followers
must not open their own streams. If shared coordination is unavailable, use bounded
short polling with no long-lived connections. Do not assume HTTP/2 on loopback:
HTTP/1.1's small per-origin connection pool must retain capacity for commands.
Reauthentication/logout terminates the old stream, and leader loss uses cursor-based
reconnect. Prove that eight tabs can issue commands and replace a closed leader.

Use streaming `fetch` with credentials and the session-proof header described below;
native EventSource cannot carry that header. Proofs never enter stream URLs. WebSocket support is justified only for a concrete duplex
feature. Browser terminal takeover is not a general-purpose remote shell: v1 can
keep takeover as an explicitly authorized local CLI operation on the office host,
with browser status/log inspection and existing exclusive-session rules.

Mutations carry stable operation IDs and expected versions. A durable acceptance
records operation ID, payload digest, principal/scope and outcome state in one
transaction. Reusing an ID with another payload is refused. A lost response is
resolved by retry/lookup of that ID within the supported deduplication window;
outside it, report expiry rather than blindly rerunning. Reuse existing org/work
deduplication semantics and document windows for new live-control operations.
`accepted` is not `completed`. Requests to an offline office follow the individual
operation's explicit queue/reject rule; never queue arbitrary commands indefinitely.

Snapshot and stream positions must compose without a gap: obtain a snapshot at P,
then consume updates after P, or buffer the stream during snapshot capture. Define
cursor scope per organization/feed and preserve authorization on reconnect. An
expired cursor requires a new snapshot. Use bounded per-client queues; disconnect
slow consumers with a resync signal instead of unbounded buffering. Coalesce
ephemeral status where safe. Give reads and commands timeouts, body limits, rate
limits and backpressure. Distinguish unknown outcome from confirmed rejection.

Existing `org-client` and SQL access are blocking/thread-based. An async HTTP
adapter must use bounded dedicated workers/queues or carefully bounded blocking
tasks. Never block async executor threads or hold a mutex across an await. Timeout
does not imply a blocking operation stopped; preserve operation IDs and track its
eventual outcome. Do not wrap each request in an unbounded `spawn_blocking` call.

## 6. Authentication, keys and platform gates

Owner authentication replaces signed-desktop-client authorization. Bootstrap is
local and protected: initialize owner credentials through setup, or use a short-lived,
single-use exchange bound to the intended instance. Do not expose an unclaimed
public admin interface or place a reusable owner token in a browser URL.

**A cookie alone never authenticates a browser operation.** Cookies are scoped by
host, not port ([RFC 6265 §8.5](https://www.rfc-editor.org/rfc/rfc6265#section-8.5));
SameSite does not isolate `localhost` ports. Each login creates an opaque HttpOnly
session cookie, with an instance-specific name, and an independent random 256-bit
origin proof. The gateway validates the pair, bound to instance, principal and the
configured browser origin (scheme/host/port), on every protected read, mutation,
stream, refresh and logout. Cookie-only requests cannot obtain or rotate a proof.

Credential/bootstrap verification returns the proof in a no-store response body.
The UI stores it only in origin-scoped IndexedDB or memory, and sends it as
`X-Org-Session-Proof` on same-origin fetches; it never enters a URL, cookie, log,
or cross-origin redirect. A missing proof requires explicit reauthentication, not
cookie-based recovery. Shared workers/tabs use the same origin-scoped session.
Rotation/revocation covers both values; refresh requires the existing valid pair.
Store/verifier details use maintained authentication/crypto libraries and receive
security review before implementation ships.

Use appropriate Secure/HttpOnly/SameSite cookie attributes, expiry, and CSRF/origin
checks. Require HTTPS off loopback; an explicit loopback HTTP configuration still
requires the pair. Validate saved Host/Origin including port, disallow wildcard
credentialed CORS and cross-origin credential forwarding. Another port receiving
or overwriting a cookie can cause reauthentication, but cannot gain authority
without the origin proof. This does not claim protection from same-origin XSS or
an OS user already able to read browser storage; those are separate threat limits.
Default/lab and an unrelated same-host dev server are mandatory adversarial tests.

Authenticate and authorize organization access at HQ on every request and stream.
Office relay commands carry a trusted, validated actor/scope envelope; an office
certificate cannot assert arbitrary owner authority. Keep agent session tokens
restricted to their employee/session. Old app tokens may remain temporarily as
internal implementation capabilities during development, but are never exposed to
the browser or retained as a public compatibility contract; replace the app-only
surface with explicit principal authorization before completing the pivot.

Browser HTTPS certificates and office identity certificates serve different
purposes. The invitation proposal binds proof to a TLS exporter. A browser reverse
proxy cannot transparently terminate that connection and preserve this proof.
Recommended first implementation: browser HTTPS and the office mTLS/enrollment
listener have separate authentication/listener configuration on HQ; both belong
to the same installed service. Terminate office TLS at the component validating
the proof, or explicitly redesign/review channel binding before adding a proxy.
Satellite connections remain outbound. Direct HQ reachability remains required.

Retain host custody of office encryption keys and private launch channels. The
downloaded payload must preserve separate key-access identities where the platform
uses code signing; an `org-agent` alias must not gain the host's key entitlement.
The current app-only peer checks, fixed bundle-relative executable paths and one
pinned home require replacement, not removal of their security invariants.

Before release, prove these platform adapters with the real packaged binaries:

- **macOS:** standalone service registration, stable signed helper identities,
  supported Keychain access without Tauri, lock/unlock behavior and upgrade access.
  A minimal signed helper bundle may be a packaging necessity; it is not a Tauri UI.
- **Linux:** a production credential provider with an explicit unattended unlock
  policy, e.g. an encrypted vault using an externally supplied service credential.
  Do not silently ship the development plaintext key-file store or store its wrapping
  key next to the vault. Specify prerequisites and the same-UID threat boundary.
  Validate SQLCipher's crypto linkage, sandbox/process cleanup and service behavior.
- **Both:** missing or wrong keys fail closed and never generate replacement keys
  over existing data. Restore/backup support must preserve recoverable keys and
  office identity. Replication is not a backup; automatic promotion stays deferred.

Exact Linux secret-provider support and standalone macOS entitlements are release
gates, not claims verified by this documentation change. No new crypto protocol is
invented here. Use maintained libraries and pin approved dependency versions.

### CA escrow and the explicit recovery gate

Creating an HQ organization in v1 creates its CA escrow and one owner recovery
code as required by the offices design. Generate the high-entropy code server-side;
use its reviewed Argon2id/encrypted-escrow format through maintained libraries.
Persist only the encrypted CA package as replicated organization data; the recovery
code and usable CA key never accompany it to a satellite. Record the escrow version
and provisioning completion idempotently. Ordinary retries never rotate the code.

Stage the pending delivery secret in the host's credential provider, with only an
opaque delivery handle in the setup journal. Before owner acknowledgement, an
interrupted protected delivery resumes the same code/escrow; it does not silently
rotate a committed CA. On acknowledgement, erase the pending delivery secret and
never expose the code through later status/read APIs. A lost already-acknowledged
code requires the separately reviewed recovery/rotation policy, not cookie-only
redisplay or an implicit setup reset.

Setup delivers the code through this dedicated protected owner channel, distinct
from status JSON/logs, and requires acknowledgement that the encrypted recovery
package and code are retained off the HQ machine. Automation uses separate protected
outputs. A fixture must recover the same CA public identity with the code and fail
closed for a wrong code or damaged package. Escrow is not a backup of office DB
keys, repository data or work records, and does not change device-bound key custody.

**Open release decision REC-1:** the supported manual HQ-loss replacement,
re-certification, highest-epoch/fencing and credential-restoration procedure still
needs its own reviewed design and disaster-recovery evidence. Resolve it before
claiming production v1 HQ-loss recovery. Creating escrow now preserves the required
material; it does not establish a working promotion implementation. Automatic
promotion remains deferred. See the [review decisions](standalone-v1-review-decisions.md).

## 7. Reuse of HQ and satellite semantics

Retain the offices design's organization single writer, deterministic apply,
`(epoch, seq)` positions, snapshot replacement, transactional work sequence allocation,
origin incarnations, acknowledgements, deduplication, scoped backfill, ownership
generations, handover/reclaim rules and protocol negotiation. Local work can commit
while HQ is unreachable; cross-office messages and org commands keep separate
outboxes. HQ holds every task/message, satellites only relevant work. Runtime
processes, secret credentials and local workspace files are not replicated as UI data.

Enrollment keeps one-time invitations, pinned HQ identity, owner approval, office
certificates, renewal and revocation. Initial enrollment differs from reconnect:
reconnect reuses the identity and resumes positions. Local satellites use the same
identity model in v1 (plan B2-10; 2026-11-10); a proven equivalent private capability
is post-v1. Same UID alone is not proof of enrollment. Headless enrollment replaces desktop deep links. Approvals
happen in an authenticated HQ browser, independent of the browser's machine.

The existing project design currently uses local bare Git in v1 and defers real
GitHub to v2. A remote satellite cannot use an HQ filesystem path as a Git remote.
This pivot adds no folder sync or new Git hosting service. Cross-office coding must
declare a reachable supported repository transport before it is advertised. The
first satellite acceptance run can use a non-repository task; shared-repository
execution remains an explicit feature gate until that contract is resolved.

The [employee requirements](../specs/employee-supervision-ux.md) and
[runtime design](employee-supervision.md) remain responsibilities of the local
office: a deterministic supervisor per employee, owned internal delegates/helpers,
aggregate resource limits and cleanup, and native harness persona provisioning.
[Processes](processes.md#35-context-bounds-fresh-contexts-and-standards-proposed-2026-11-09)
provide bounded instruction delivery and overridable delegation guidance, with
standards retrieved on demand. [Channels](../specs/chat-ux.md) extend office-first
messaging for authorized team questions and DM follow-up; membership does not wake
every employee. A browser connection is never the lifetime owner of this work.

## 8. Implementation order and release evidence

| Work | Result required before moving on |
| --- | --- |
| Inventory and contract amendment | Apply the deletion ledger, naming/reorganization plan, API-version transition and spec precedence; identify every production mock path and unsupported server operation |
| Instance/setup foundation | Identity/root locks, service lifecycle, platform key custody, matching release payloads and repeatable automation |
| HTTP and frontend | Authenticated HQ API, asset routing, command outcomes, consistent snapshots/events and real frontend callers; remove Tauri paths in the same feature change |
| Office networking | Reviewed sync/PKI contracts, enrollment, resume, ownership and failure behavior; existing design is not implemented evidence |
| Runtime integration | Selected supported harnesses, scoped CLI binding, deterministic supervision and bounded process cleanup |
| Acceptance and deletion check | macOS/Linux release walkthrough, failure tests and no Tauri dependency or production mock fallback |

Owners follow AGENTS.md: app agent for frontend, server-dev for domain/API/storage,
River for host/platform/sync/PKI, Remy for `org-agent`, runtime owners for adapters,
Ada for shared contracts/design, Eden for UX specs. Cross-cutting changes need their
contract review; this docs proposal does not claim merge approval.

Run repo formatting, applicable Clippy and Rust tests on implementation changes,
plus frontend type checks, tests and build. Meaningful failure tests cover duplicate
setup, data-root aliases, unavailable keys, bad signatures, cross-instance cleanup,
port conflicts, interrupted enrollment, revoked/expired identity, incompatible
versions, lost replies, stream gaps, slow clients, shutdown and orphan cleanup.
Use temporary roots and fake runtimes. Real runtime and signed-platform checks are
explicit opt-in acceptance runs. See [dogfood-run.md](../specs/dogfood-run.md).
