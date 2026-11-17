# Host library ↔ agentic-org-office: the boundary (phase 1, HQ only)

> **Amended by [ADR-0016](../adr/0016-no-application-level-data-encryption.md)
> (2026-11-17).** Organization data is no longer encrypted by the application. Database
> encryption is to be removed. The per-office key, its custody and its delivery at
> launch stay, renamed the office secrets key and used only to wrap an office's
> private keys; a missing key no longer prevents an office from opening its data.
> Writer custody and the private launch itself stay. Until the code changes, this document describes the
> current implementation.

Status: **current launch-v2 evidence plus selected target adaptation**, reconciled
2026-11-15. The host library is `crates/org-host`, composed by `agentic-org serve`;
the office executable is `agentic-org-office`. [Executable migration](executable-migration.md)
records implemented versus unavailable commands. No key-store namespace, signing
identifier, grant namespace or persisted identity is renamed by those filenames.

## Owner decisions 2026-11-17 that this document must be read with

Nothing has shipped, so there are no protocol versions to keep side by side. Where the
text below says "launch v2" it describes what the host code writes today, and where it
says "launch v3" or "pending" it describes the contract. The requirements are:

| Entry | Requirement | Code today |
| --- | --- | --- |
| B-06 | **There is one launch contract.** It is the verified-rendezvous design described here as launch v3: the host passes a rendezvous, the worker connects to it and is admitted by its process identity, and secrets and office facts travel over that connection. The older first-line contract is removed, not kept as a fallback, and the version number restarts at 1. | The host implements both. It uses the rendezvous form only for a registered, protected instance and falls back to the first-line form otherwise (`crates/org-host/src/supervisor/child.rs`). The worker implements both. Remaining work: remove the first-line form and the gate. |
| B-07 | **People are authorized by grants.** Every request made for a person carries that person's principal and a host-issued grant, which the worker verifies ([ADR-0006](../adr/0006-selected-task-acceptance.md), [ADR-0012](../adr/0012-office-channel-lifecycle.md)). `app_token` and the `office.app_token` call are removed. | The launch line still carries `app_token`; the worker stores the grant key unused. |
| B-08 | **Seven web channels per office.** The host refuses the eighth, matching the worker and ADR-0012. | Already seven: the host's channel registry enforces `MAX_GATEWAY_CHANNELS` and a test asserts the eighth is refused. The figure of 63 in ADR-0012 was observed at an earlier commit. Nothing to change. |
| B-10 | **The host serves the work reads the web needs** through the office channel, so the browser reads tasks, projects and people through the same path as commands. | `host.sock` dispatches only `hello` and `instance.status`. |
| F-03 | **No database key.** `db_key` leaves the launch contract with [ADR-0016](../adr/0016-no-application-level-data-encryption.md); the host passes the office secrets key instead. | The launch line carries `db_key`. |
| B-09 | **macOS and Linux, including a container** ([ADR-0017](../adr/0017-linux-and-container-hosting.md)). Process admission, the credential provider, confinement and service lifecycle are platform contracts with a macOS and a Linux implementation. The macOS-only statements below describe the code today. | Workers start only on macOS. |

These are requirements for implementation, tracked in the [feature checklist](../FEATURES.md):
B-06 and B-07 under FC-017, B-10 under FC-013, F-03 under FC-162 and B-09 under FC-164.

## Selected provisioning and storage responsibilities

The [organization-data contracts](organization-data/README.md) own the target
sources. The host retains keys, instance/catalog locks, typed administration and
private worker launch. A separate `agentic-org-web` owns browser HTTP and has no
office DB key/entitlement. Creation requires an authenticated instance administrator,
including during initial HQ-only setup; an organization owner or office certificate
alone cannot create another organization. Browser requests never select a raw local
RPC method or managed storage path.

| Source/resource | Target owner and durability |
| --- | --- |
| Organization identity and HQ bootstrap | Typed provisioning plan records explicit slug, remote organization repository registration and `state_branch`, office definition and initial team choices. Every bootstrap employee, including HR and any owner-person, needs a valid home team; identity is distinct from execution config. See [create flow](create-organization.md). |
| `durable/state/` | Ordinary organization Git files, stable-ID hierarchy. Current organization/office markers remain local catalog evidence, not replacements for those files. |
| `durable/journal/`, `durable/authority/`, accounting and code checkpoints | Protected acknowledged unpublished changes, allocations, usage streams and local/unpushed work recovery. Preserve before acknowledging; Git publication alone is insufficient. |
| `durable/messaging/`, `durable/recovery/`, `durable/budgets/` | Worker/domain-owned durable source envelopes, attachments, receipts/results, streams/high-waters, snapshots/cleanup, policy versions/periods/extensions/holds. Resolve the one [storage root](organization-data/README.md#shared-storage-contract) before opening caches; never put their only copies in disposable SQLite or shared Git. |
| Private notes and credentials | Separate durable author-private notes DB with no administrator read exception; host/private authentication stores and local credential bindings stay protected. Shared receipts must not reveal private-note existence. |
| `runtime/<office-id>/office.db` and repository caches | Rebuildable projection/transient state only after complete source verification. One bare cache per office/repository, no project integration-branch policy embedded in the cache. |
| Task workspaces | Employee/team hierarchy from [document 04](organization-data/04-agent-structure.md#runtime-location), project/task/repository stable IDs; retained through handoff until closure, with explicit safe migration when placement changes. |

The current worker's encrypted `org.db`/`office.db` still contain authoritative
records and machine credentials. They are **not disposable projections in the
code today**; they become so only when the store writes the selected durable
sources, with verified restore and separately reviewed paths/sandbox policies. Preserve Git-metadata,
credential, other-task and managed-state denies; do not relabel an old allowlist.
Instance roots, service names, sockets and orphan-key sweeps must be scoped without
rewriting the existing credential service/signature identifiers. Cross-instance
catalog loss is never proof of orphaned credentials.

### Additive HQ creation context (REC-ESC-1)

Launch v3 create has an optional `hq_creation` object with exactly
`{version:1,operation_id:<canonical ULID>,escrow_profile:1,public_root:<absolute canonical state root>}`. In the code today the object is optional and a create without it is the earlier
create; under the decision of 2026-11-17 every create carries it (FC-142). A
protected host provisioning operation supplies this context, its stable
`owner_member_id` security principal, and the retained recovery code over the
existing post-admission handoff. Neither this object nor assignment grants
provisioning authority. Unsupported versions/duplicates fail before mutation.

For this context only, the worker durably freezes nonsecret request identity in
`hq-creation.v1.json` under the office lock before initial preparation. The initial
local SQLCipher transaction atomically stores operation/owner binding, usable CA
key, public envelope and completion/version. The new local-only table is excluded
from replication and selected journal capture. Interrupted preparation resumes
only the identical intent; it verifies retained material rather than generating
again. An empty version-zero database from interrupted initialization may finish;
foreign schemas, missing committed material or changed input remain intact and
require recovery. This local intent does not establish protected writer custody or
copy/restore authority. Those must be verified by the host before selected
readiness. See [ADR-0007](../adr/0007-hq-creation-escrow.md).

The explicit public root must already exist, resolve without symlink/path aliases,
and be disjoint from the private office directory. It is frozen in the intent,
not taken from a browser path. Before Created, exact committed public envelope
bytes are immutably published there as `security/ca-escrow.v1.bin`. Reopen can
repair absence from the retained local record; conflicting bytes cause recovery,
with both identities preserved. A public-only read DTO excludes the usable key.

### Web process entry (GW-SBX-MAC-1)

The verified release's `agentic-org-web --serve` loads its sibling `ui/` public
build before confinement. The host spawn closes unintended inherited descriptors
with `POSIX_SPAWN_CLOEXEC_DEFAULT`, retaining only its three launch/stdio pipes;
the child independently checks that exactly descriptors 0/1/2 remain. Unexpected
descriptors cause process exit before readiness, rather than closing a descriptor
potentially owned by another Rust value. Assets remain bounded immutable memory.
After the ADR-0008 single-thread checks and parameterized policy succeed, only
the exact rendezvous is connected. Strict bounded header reception accepts no
early rights; the next marker must carry transfer then listener, with exact
socket-type/CLOEXEC checks before `fds.received`. The same socket then carries
host control; neither host.sock nor an office pathname is a fallback. Actual
`tls.fetch` supplies the verified listener configuration before HTTP accepts.
Host EOF terminates the web process and all its browser/native authority.

Web admission uses `com.ellosoft.agentic-org.org-gateway`, separately from the
worker signing identifier, with the compiled Developer ID team, hardened runtime,
no debug/injection entitlements and no Keychain access-group entitlement. The
host rechecks the accepted reader's audit token, image continuity and kernel
sandbox before sending either descriptor. The host-owned private connection is
distinct from `host.sock`: only it can fetch the per-bind listener snapshot.
Web office channels travel as `ChannelKind::Gateway` records on its admitted
datagram transfer socket; native channels retain the existing stream-marker
delivery and `ChannelKind::Local`. Both retain descriptors until their correlated
receipt; sessions/grants remain connection-bound and end at control EOF. (Amended by
[ADR-0013](../adr/0013-stored-browser-sessions.md): browser sessions are stored at the
host and no longer end at control EOF; native sessions stay connection-bound.)

The private version-1 instance configuration has an optional `browser` listener
snapshot, absent for existing native-only installations. Protected setup writes
it under the instance lock; startup never takes browser configuration from HTTP
or a request path. A bind uses one validated immutable snapshot. The initial
host composition admits `http_loopback`; HTTPS remains unavailable until its
stored pair has the required host-side parsing, matching and coverage checks.
It must not bind or downgrade HTTPS through that missing path. Browser service
starts only for a registered, protected HQ instance. Its child is supervised with
bounded restart/backoff and killed/reaped on host shutdown; failures never publish
selected task readiness. A failed browser leaves the native recovery/control
connection available.

### Selected identity carrier

> **Owner decision 2026-11-17 (conflict plan entry B-04): every ID is a UUIDv7.** The
> `selected_v1` profile below is therefore the target for all records, not a second
> profile beside an older one. Nothing has shipped, so the older alphanumeric-ID
> validator, the version-1 marker and the union path validator are removed instead of
> kept, and the check that an HQ create carries its creation context applies to every
> create (FC-142).

**What the code does today (replaced by the decision above).** Organization and
office entities created under the selected profile use canonical lowercase UUIDv7
text, including hyphens. Launch-v3 adds the explicit optional
`id_profile: "selected_v1"` to its office context; without it the code applies
its older alphanumeric-ID validator. The selected profile requires UUIDv7
office/organization IDs and explicit HQ creation context on create. The profile
does not cover instance IDs, security-member IDs or operation IDs. This does not
confer writer or owner authority.

The plaintext runtime marker uses `office.json` version 2 for this profile,
requiring UUIDv7 office and organization IDs; the code also still reads
version 1. The safe storage-path validator accepts both the older plain
IDs and canonical UUIDv7, solely as a single path component, and the command and
wire validators keep their narrower rules. Host custody/catalog and worker
open/discard paths use the shared safe storage validator; no arbitrary opaque ID
is interpolated into a path.
Socket handles remain separately bounded and collision-checked.

### Selected provisioning port (implementation in progress)

The additive native `organization.provision.v1` call carries `session`, canonical
ULID `operation_id` and a version-1 plan with explicit `name`, `slug`,
`repository_remote_url`, `state_branch` and `home_team_name`. It preserves the
name-only `organization.create` and existing HTTP DTOs. The host rechecks the live
instance administrator and binds the operation/fingerprint to that account;
organization ownership alone does not suffice. No storage path is a request field.
The admitted initial profile creates one explicit root home team, an HR agent
identity in it (execution remains unavailable without its separate configuration),
and no owner employee. The protected owner security member remains distinct.

The host freezes selected UUIDv7 entity IDs and key fingerprint in its protected
SQLCipher reservation. The code first enters credential custody under the
operation's opaque handle; the reservation contains no code/key. Retry verifies
that binding and the original office key, never generates replacements for missing
committed custody. A per-operation gate serializes worker attempts. Interrupted
selected preparations/keys are retained and excluded from legacy discard/sweep.
The launch context carries the exact nonsecret prepared bootstrap plan and IDs;
worker intent retains them. The host resolves private/public roots inside the
locked instance, installs protected writer custody before selected admission and
keeps a pending outcome until coherent canonical bootstrap and worker readiness.
Initial canonical files and trusted directory generation enter the existing work
acceptance journal together; they do not allocate work-origin event positions.
Remote connectivity/publication and HR execution remain explicitly unverified.
This describes the intended adapter, not completed setup or adoption evidence.

## Target launch and recovery gate

Launch-v2 examples in §§1–6 below preserve the exact baseline protocol and security
review. The executables had other names at that review; today they are
`agentic-org-office` and `agentic-org`, and reusable host code is
`org-host`. `office.json` v1 and launch v2 are independent of organization-data
`schema_version`/`revision`. No extra slug/repository/home-team fields are added to
the v2 first line by this documentation change. A reviewed provisioner
must carry the full selected create plan and current stored-marker adoption rules
before enabling target creation. Initial setup and production web handoff remain
unavailable; launch v3 and satellite create remain separate pending contracts.

Restart/rebuild verifies trusted org/HQ/CA identity, journal envelope hashes,
contiguous streams/incarnations, highest authority epoch, completed snapshot bytes
and actor/fingerprint-bound command outcomes. `outcome_unknown` is reconciled before
any side effect repeats. Preserve the existing combined messaging allocator and its
high-water, independent work stream, opaque/native/base snapshot evidence and
acknowledged numbering allocations. A copied checkpoint cannot promote an office.

Before launching an agent, restore HQ's durable placement generation and execution
journals, rebind legitimate credentials, and verify old-process cleanup or an explicit
machine fence. Never kill a restored PID without ownership mark/generation and
machine/boot/start checks. Cache deletion does not prove the old process stopped;
missing evidence means recovery required. Document 11's permits and placement
generations are separate from process-cleanup generations and task transport
ownership generations. Host EOF, descriptor isolation, key-missing refusal,
adopt/discard quarantine and bounded owned-child cleanup below still apply.

## Implementation evidence by source checkpoint

### F0 integration baseline

F0 imports documentation over integration base
`73a39c58d683b00ad8620d05f7d5ae10fdc17bc9` without importing the selected
host-control implementation. In this checkout,
`crates/org-host/src/service/mod.rs` dispatches only registered-instance hello
and count-only instance.status; session/channel methods are refused. The web
executable still refuses service startup because authenticated host handoff and
the browser backend are not wired. See the [F0 executable status](executable-migration.md#f0-integration-baseline).

### Preserved selected host-control integration (unaccepted)

The following observations describe the **unaccepted**
`codex/v1-assignment-integration` reservoir at
`b77cbd6876d72f76cc3b7f1d5fa3c3d96f424c09`. They are not implementation or
verification imported by F0. The [source map](../../.agent-kit/tasks/v1-delivery-source-map.json)
pins the preserved source; its extraction and acceptance belong to later units.

A registered host with an opened protected account registry uses launch v3 for
existing office starts. The rendezvous reader must match the admitted child audit
token (and PID), immediately before any key is sent. The host retains transferred
descriptors until `fds_received`; unknown receipts, timeout or a broken transfer
connection close that worker's channel service. In the code today, unregistered starts and
the earlier create/discard contract still use launch v2.
This transition does not itself admit selected storage or publish its capability.

On local host-control connections, `office.open_channel` carries its existing
typed response followed by `HOST_SOCK_MARKER` and one descriptor. The client sends
the additive typed `fds.received` call with `{of: channel_id}` on that same
connection. The host retains its descriptor until that receipt; a five-second
deadline or an unexpected receipt closes the connection. No other outbound frame
may interleave between the response and its descriptor marker. Grants are issued
only for a received channel recorded for the same live session and connection.
Session end revokes its worker grants; it never changes task assignment authority.
These are host-control API 1 additive methods, not a change to work event codecs.

The additive `session.inspect {session}` operation returns a live protected
account view (account, canonical login, administrator flag, linked organizations
and office IDs) and integer `server_time_ms`, `absolute_expires_at_ms` and
`idle_expires_at_ms`. It authorizes on the issuing connection and rechecks the
registry. Expiration timestamps project the remaining monotonic lifetime onto
the host's current wall clock; inspection never resets the absolute lifetime.
This lets browser session responses report host authority rather than infer
deadlines or permissions from a prior sign-in. It returns no handle, grant or key.

At that checkpoint, account sign-in, renewal and logout have a registry-backed
implementation. Protected first bootstrap, selected writer admission, complete
release/service composition and browser handoff still require their remaining
integration and execution evidence; this section claims none of them complete.

At that checkpoint, `crates/org-host/src/service/mod.rs` dispatches registered-instance
hello, count-only instance.status and the registry-backed session/channel methods
described above, refusing unregistered instances. The old
sessionless host methods below are historical requirements, not currently exposed
operations or a workaround for unavailable setup. The launch-v2 framing itself
remains in crates/org-office-launch.

### Phase-1 launch-v2 evidence

The remaining numbered sections record the inspected phase-1 host/worker contract
and its review constraints, including the macOS app-token caller path. Exact
wire/key/signature values are kept here for implementation review. The retired
signed-Tauri public caller, account-wide home and bundle-relative installation are
not target installation requirements; standalone principal grants, scoped instances
and verified packaged helper admission replace those roles without relaxing custody.
Where code differs, describe it as implementation evidence and resolve the
contract explicitly; this documentation does not override code.

## 1. Who owns what on disk

For this **current launch-v2 baseline**, `<home>` is resolved by `org_proto::location`
(`$ORG_OFFICE_HOME` or `~/Library/Application Support/agentic.org`). Target instance
provisioning must map/protect this control subtree without moving durable sources
or changing credential identities implicitly.

| Path | Written by | Read by |
| --- | --- | --- |
| `<home>/host.lock` | host: **the single-host lock**, an exclusive `flock` held for the host's whole life. A second host on the same home exits at once, so one host creates every office. Inside it, the **catalog mutex** (in-process) guards only a create's name check and reservation (see 4). | host |
| `<home>/host.sock` | host (0600) | app, `agent-os` (run by hand; office-run sessions compute their socket path, §3) |
| `<home>/run/<handle>.sock` | server (0600) | app, `agent-os` (address from the host's `office.list`, or computed when `ORG_OFFICE` is set, as in office-run sessions, §3) |
| `<home>/offices/<office-id>/` | **server** creates it (0700) on `--create` | host lists the folder names |
| `…/office.lock` | server: `flock` from before it opens anything until it exits | nobody else, ever |
| `…/office.json` | server, **written last** on `--create` (tmp + rename + fsync): this is the "stored" marker | host (the catalog) |
| `…/org.db`, `office.db`, `workspace/` | server | server |
| the office's database key (amended 2026-11-08: key custody): a Keychain item, service `org.agentic.office-key`, account = the office id (release); a file in the dev-keys folder (debug builds and tests) | **host** creates it before the office's first `--create`, and deletes it only after `discarded` | host, which sends it on the pipe (see 3); the server never reads a key store |

**The catalog** = the folders under `offices/` named by a plain id that hold a valid `office.json`.
A satellite whose `office.json` records a terminal join `state` stays in the catalog (status
`unavailable`, its `join_*` reason) but is never spawned, at startup or later (2026-11-10).
The host looks only at folders named by a plain id; any other folder (such as `.refused`) is never
started or discarded.
- **No `office.json`:** an unfinished create. The host never starts it. At startup, or after that create
  failed before `office.json` was in place, it runs `agentic-org-office --discard <id>`, with the office's
  key on the first line, or `"db_key": null` when it holds none for that id (amended 2026-11-08: key
  custody). The server takes the lock (same wait), and removes the folder only if the office never
  finished (with no folder, it takes no lock, creates nothing and reports `discarded`, see 3):
  - it refuses (`already_exists`) a folder with `office.json`, or with databases that hold a committed
    organization. One exception (2026-11-10): a satellite whose join never completed, removed with
    `office.remove_pending` (`office.json` with `role: satellite`, a `pending` or terminal `state`, and
    an `org.db` with no applied snapshot, checked with the key); an `active` office or any applied copy
    is still refused;
  - with a key, it deletes only when no database file exists, or when it opens them with that key and
    they hold no committed organization; with `db_key: null`, it deletes only a folder with no database
    file;
  - a database it can't open or check (the key doesn't open it, SQLCipher fails), or any database file
    when it has no key, means it refuses (`storage_error`).

  On a refusal the host logs it and leaves the folder (and keeps its key) for a person. So a real
  office that lost its `office.json` is never destroyed automatically. The server never removes a key:
  the host deletes its own key item for that office only after the server reports `discarded`.
- **An unreadable or invalid `office.json`** (other than a mismatched id, below): the host logs it and
  leaves the folder alone. It neither starts it nor runs `--discard`.
- **An `office.json` whose `office_id` isn't the folder's name:** the host refuses to start it and moves
  the folder to `offices/.refused/<id>`, `<id>` being the folder's name (nothing deleted, the key stays).

**A refused create:** if a server reports a `created` office other than the one asked for (office id,
`org_id`, `org_name`, `office_name` or role), the host stops it and moves the folder of the id **it
asked for** to `offices/.refused/<id>` (out of the catalog; nothing deleted, the key stays). It never
touches a folder named by a different id the server reported; it only logs it.

**Lock-held-means-running:** an office's `office.lock` is held if and only if a server is running
that office. The server **waits** for the lock at start (blocking, up to 10 s, then `failed: in_use`),
so after a crash or a host restart, a slow-exiting old server and its replacement never overlap. The
host never takes an office's lock and never decides an office is stopped from a lock probe. Status comes from supervision (see 4).
The `office.lock` fd is close-on-exec, like every fd the server opens (see 2): a child that inherited it
would keep the lock after the server died.

### `office.json` (schema v1, the server writes it, the host reads it)
```json
{"v":1,"office_id":"01…","office_name":"HQ","role":"hq",
 "org_id":"01…","org_name":"Acme","created_at":"2026-11-08T02:10:00.000Z"}
```
No secrets. At most 64 KiB, mode 0600, written to a temp file, fsynced, renamed into place, then the
folder fsynced. Readers read it without following symlinks, only as a regular file owned by this user,
and check that `office_id` names its folder. Unknown fields (phase 2 adds `hq_address`) and an unknown
role are kept when a server rewrites it (e.g. on an organization rename, later). A satellite's
`office.json` also carries `state`: `pending` while its join is open, then `active`, or a terminal
`expired`, `cancelled` or `rejected`; the worker writes it, and the host doesn't restart a terminal one
(2026-11-10, see the offices design, "Provisioning a satellite"). On `role: satellite`, an absent or
unknown `state` is treated like an invalid `office.json`: logged, never started or discarded. A
satellite's `pending` is its join state; the host reports it as `starting` with `join_pending`, which
is unrelated to HQ's `OfficeInfo.status: pending` (an invited office).

## 2. Starting a server

```
agentic-org-office --create          # create a new office from the pipe message, then serve it
agentic-org-office --office <id>     # serve an existing office
agentic-org-office --discard <id>    # remove an unfinished office, then exit (the host then deletes its key)
```
- The host always sets `ORG_OFFICE_HOME` explicitly on the child, so both use the same home.
- No details on the command line beyond the office id (the design: details travel through the pipe).
- **stdin is the control pipe from the host.** EOF on stdin = the host is gone, so the server shuts
  down: it stops accepting, finishes in-flight writes, removes its socket, releases the lock and
  exits 0. (The design's "exits when the pipe to the host closes".) The child also stays in the host's
  process group.
- **stdout carries the server's control lines to the host, and nothing else.** Logs go to stderr,
  which the host sends to `<home>/logs/<handle>.log` (`logs/` 0700, files 0600; rotate at 10 MiB, keep one old file, so a crash loop can't fill the disk).
- **No child can read the host's lines or write a control line.** The structural guarantee is the fd
  swap, which the server does itself, first thing at startup, before any thread or child starts: it dups
  fds 0 and 1 to new close-on-exec fds, which carry the pipe, and reopens 0 and 1 on `/dev/null`.
  `org-supervise` is the single spawn path (see 4): `posix_spawn` with `POSIX_SPAWN_CLOEXEC_DEFAULT`,
  so a child inherits only the fds it's given. It doesn't do the swap. Every other fd the server opens
  is close-on-exec too, the `office.lock` fd above all. The host opens its own fds close-on-exec too, so
  no child holds `host.lock`.
- Framing: JSON, one object per line, at most 1 MiB per line (`org_proto::frame`). Every line has `"v":2`
  (amended 2026-11-08: key custody: version 2 added the database key; a line of any other version,
  version 1 included, is `unsupported_version`). `office.json` keeps its own version, `"v":1`.

## 3. The pipe protocol (v2)

> **Pending: launch v3** (amended 2026-11-10, PR #5 post-merge fixes). Plan task X-02, the
> `org-proto` v2 and launch v3 contract, is an **approved draft not yet in this repo**
> (rev 13, approved by ada 2026-11-10; `projects/ac/agents/ac-fullstack-engineer-rowan/knowledge/designs/x-02-org-proto-v2.md` in the
> agents repository). Other docs here that cite "X-02 §…" mean that draft, and this note is their
> anchor until X-02 lands here. Launch v3 changes this section:
> - the host spawns the server suspended, verifies its image, and only then creates a per-spawn
>   **verified rendezvous** socket; the server connects to it, and the host checks its token again;
> - secrets (`db_key`, the grant key, a satellite invitation's bearer secret) and the server's events
>   (`created`, `ready`, `discarded`, `failed`) travel over that connection; stdout becomes untrusted;
> - the first line keeps only non-secret fields (for a satellite `create`, the invitation's
>   non-secret reference).
>
> Pending in X-02 §12 Q9, not yet in its §5: the office temp handle on the first line (at startup,
> before it starts any worker, the host allocates and claims one for every catalog office that lacks
> one), the `joined` event, moving the watchdog's `child`/`child_gone` lines (below) off stdout, and a `layout_changed`
> record that asks workers to refresh sandbox policies.
>
> **Until then:** a satellite create is refused (below); office-run sessions don't launch, because they need
> the office temp handle (fail closed: an employee's session or job shows Needs you, a one-off run
> ends with that outcome; no fallback temp directory); hires and office creation still allocate their
> temp handles in `office.db` but create no temp folder (agent-sessions, `TMPDIR`);
> the watchdog's `child`/`child_gone` lines stay on stdout under v2;
> setup- and host-made layout changes are deferred while affected sessions run (agent-sessions,
> "Standalone-v1 managed state and grants").
>
> **A satellite `create` needs launch v3.** Until v3 lands, the v2 protocol below is the contract,
> and it refuses `role: satellite` (the `create` bullets below).

**Host → server** (stdin; the first line, on **every** start, matching the flag):
```json
{"v":2,"op":"create","org_id":"01…","org_name":"Acme","office_id":"01…","office_name":"HQ","role":"hq","app_token":"<64 hex>","db_key":"<64 hex>"}
{"v":2,"op":"start","app_token":"<64 hex>","db_key":"<64 hex>"}
{"v":2,"op":"discard","db_key":"<64 hex>"}      // or "db_key":null; the field is required
```
- `create` with `--create`, `start` with `--office`, `discard` with `--discard` (amended 2026-11-08: key
  custody; `--discard` read no line before). A first line that's malformed, an unknown op or for
  another flag → `failed: invalid_request`. EOF before the first line → exit 0 without a `failed` line
  (the host is gone).
- After the first line, a server ignores every further stdin line, known ops included, so a
  second `start` can't swap the token or the key mid-life. stdin stays open only so its EOF can stop
  the server.
- **`app_token`** (ADR-0003 §2.1): the office's ephemeral app token, made by the host for each office
  each time the host starts, kept only in memory, never in argv, env, a file, a log or the Keychain.
  The server keeps it only in memory.
- (amended 2026-11-08: key custody) **`db_key`** (ADR-0002): the office's 32-byte SQLCipher key, 64
  hex digits, from the host's key store, on every create, start and discard line (`null` on a discard
  when the host holds none). It takes the same secret-line path as the app token: never in argv, env,
  a log or a file, and the host holds it in memory only from creating or loading it until the line is
  written. The server uses only the key on its first line: it never creates, stores, loads or deletes
  a key, and never touches the Keychain. It wipes every copy after it first opens the databases,
  except the one its store keeps for its reader pool (in memory only, zeroed on drop, never logged).
- Launch v2 creates only HQ offices; any other role is refused. (Amended 2026-11-10: satellite creates come
  with launch v3, see "Pending: launch v3" in §3; launch v2 keeps refusing them. A satellite create
  skips the organization-name check below, see the offices design, "Provisioning a satellite".)
The host has already checked the name, its uniqueness (ignoring case) among this Mac's organizations,
and that the office id's 12-character handle is unique (ignoring case). The server checks the name again
with the same rule (one implementation, see 6). Lines after the first are ignored; an unknown op on the
first line is `invalid_request`.

**Server → host** (stdout):
```json
{"v":2,"event":"created","office":{…office.json…}}      // --create only: the databases and
                                                        // office.json are durable
{"v":2,"event":"ready","socket":"/…/run/3RXF2NS0SH83.sock","pid":4242}  // listening, hello works
{"v":2,"event":"discarded"}                                            // --discard only, then exit 0;
                                                                       // also with no folder (idempotent): see below
{"v":2,"event":"failed","code":"key_missing","message":"…"}             // then exits non-zero
```
- `created` comes before `ready`. `organization.create` answers the client as soon as `created`
  arrives (the contract's "succeeds once stored"), with the HQ status `starting`, or `active` if
  `ready` came too.
- `ready` means the socket is bound and accepting: `hello` answers.
- `--discard` with no folder takes no lock and creates nothing: it reports `discarded`, and the host
  then deletes its key item for that office, if any (amended 2026-11-08: key custody).
- `failed` codes, and what the host does with each:

| code | meaning | host |
| --- | --- | --- |
| `invalid_request` | a bad first line or bad args | fatal: `unavailable`; starts again only with a host restart and a fixed binary |
| `unsupported_version` | `v` unknown | fatal: `unavailable`; starts again only with a host restart and a fixed binary |
| `invalid_name` | name breaks the rule | create fails: `invalid_name` |
| `already_exists` | `--create` but the folder or office.json exists; `--discard` of an office with office.json or a committed organization (except a satellite join that never completed, see the discard exception in 1) | create fails: `already_exists`; discard: log, leave it |
| `key_missing` | (amended 2026-11-08: key custody) `--office`: the key the host sent doesn't open the office's existing databases (ADR-0002); the server deletes nothing. The host also sets `key_missing` itself, without a spawn, when its key item for the office is gone. `--discard` says `storage_error` instead | fatal: `unavailable` (`key_missing`); no automatic restart, but `office.restart` retries it. The host never makes a new key for an existing office |
| `in_use` | lock not acquired within 10 s | restart with backoff |
| `storage_error` | anything else (I/O, SQLCipher); `--discard` of a folder whose databases it can't open or check, or with any database file when `db_key` is `null` | restart with backoff (create: fails with `storage_error`; discard: log, leave it and its key) |

- A message is for people: never the key, paths or SQL. Unknown codes count as `storage_error`.
- (amended 2026-11-08: key custody) **The server never deletes a key, and the host keeps the key after a
  failed `--create`.** If `office.json` is in place, the office is stored and the host adopts it (see
  4). If not, the host runs `--discard` with that key and deletes the key only after `discarded`: a
  database left behind without its key could never be checked or discarded. On a discard refusal the
  key stays with the folder.
- EOF on stdout with no `failed` line, or exit before `ready`, means a crash: restart with backoff.
  Exception: a satellite worker whose `office.json` records a terminal join `state` exits cleanly and
  isn't restarted (2026-11-10).
- (amended 2026-11-08: agent sessions rev 3.3, ada) **Children, for the watchdog.** After `ready`, the server also sends a line for each agent runtime it spawns, and one when that child is reaped and cleaned up ([`agent-sessions.md`](agent-sessions.md), "Processes and safety"):
  `{"v":2,"event":"child","pgid":4250,"pid":4250,"started_at":"…","mark":"…"}` and `{"v":2,"event":"child_gone","pgid":4250}` (amended 2026-11-08: key custody: `"v":2`, like every pipe line).
  The host keeps them in memory only, and when the server dies it runs the orphan cleanup on them before it restarts the server. The mark isn't a secret.
  Office-run agents' tools can't connect to `host.sock` (a fixed deny in `agent-sessions.md`). `agent-os` there always has `ORG_OFFICE`, so it computes its office's socket path (`org_proto::location::office_socket_path`) and never needs `host.sock` or `office.list`.
- (amended 2026-11-08: agent sessions rev 3.3, ada) **A host ignores an event it doesn't know** (it logs the event's name once), so a server can add events without a host change. Only `failed` codes have their own unknown rule (above).

## 4. What the host does
- **Status per office** (`OfficeInfo.status`): `starting` (spawned, no `ready` yet; at host startup,
  also every catalog office not spawned yet, see Startup: this widens `starting`) → `active` (`ready`,
  child alive) → `offline` (exited; a restart is pending, with `next_restart_at`, unless the reason is
  `stopped_by_owner`) → `unavailable` (fatal code, or restarts given up). The `ready` wait is 30 s
  (the first SQLCipher open can be slow), then kill + backoff.
  - **A pending satellite join** (2026-11-10): the worker sends `ready` as soon as its socket is bound,
    whatever its join state, so the 30 s rule covers only the start. While `office.json` records
    `state: pending`, the host reports the office as `starting` with `status_reason: join_pending`,
    never `active`, for as long as the join lasts (up to the invitation's expiry). It moves the office
    to `active` only when the worker reports `joined` over the rendezvous, after approval and the
    required initial synchronization; the worker writes `state: active` first, then sends `joined`.
- **Status reasons** (`OfficeInfo.status_reason`, set with `offline` and `unavailable`, and with `starting` only for `join_pending`), one for every
  way a start or a running server ends:

  | what happened | `status_reason` | status |
  | --- | --- | --- |
  | EOF on stdout without a `failed` line (a crash, before or after `ready`) | `crashed` | `offline` |
  | no `ready` within 30 s (the host kills it) | `start_timeout` | `offline` |
  | `failed: in_use` | `in_use` | `offline` |
  | `failed: storage_error`, or an unknown code | `storage_error` | `offline` |
  | `failed: key_missing` | `key_missing` | `unavailable` |
  | `failed: unsupported_version` | `unsupported_version` | `unavailable` |
  | `failed: invalid_request` | `invalid_request` | `unavailable` |
  | the give-up rule (below) | `restarts_exhausted` | `unavailable` |
  | the owner stopped it (no call does yet) | `stopped_by_owner` | `offline`, no restart pending |
  | a satellite join is waiting for approval or its initial synchronization | `join_pending` | `starting` (the worker is running) |
  | a satellite join ended (`office.json` `state`) | `join_expired`, `join_cancelled` or `join_rejected` | `unavailable`, no restart |

  `invalid_name` and `already_exists` end a create only; that office never enters the catalog.
- **Restart:** backoff 1, 2, 4 … 60 s, reset after 60 s of uptime. Fatal codes never restart on
  their own. A `key_missing` office starts again only with `office.restart` or a host restart.
  `unsupported_version` and `invalid_request` start again only with a host restart and a fixed binary
  (`office.restart` refuses them).
- **Shutdown** (SIGTERM from launchd at logout, or the host exiting): close every child's stdin,
  wait up to 10 s, then SIGKILL what's left.
- **Create:** under the catalog mutex, and only for this step: check the name and its uniqueness
  (counting names reserved by creates in flight), make the org and office ids (re-roll on a handle
  clash), reserve the name and handle, then release the mutex. Outside it: create the office's key and store it
  in the key store (amended 2026-11-08: key custody), spawn `--create` with it, and wait for
  `created` (or `failed`) for at most 30 s. On failure, keep the key, run `--discard` with it (rule in 1),
  return the error and emit nothing, unless `office.json` is present (adopt, below). On success, emit `organization.created` on `subscribe`. The reservation ends when
  the create fails, or once the new office is in the catalog.
- **Capabilities:** the host's `hello` lists in `capabilities` the optional calls and events it implements (`office.app_token`, `office.restart`, `office.status_changed`; see `org_proto::capability`), so the app checks before relying on one.
- **App-only calls and the exec swap** (ada, 2026-11-08): the client rule: a client reads `hello`'s answer before sending an app-only call (`office.app_token`, `office.restart`), and sends nothing else until the answer arrives. The host check: the host accepts an app-only call only if it arrived after the accept was sampled and nothing follows its request line (otherwise `not_authorized`). The reason is the exec swap: a process can connect, keep the socket in a child, send the call, then `exec` the signed app, and `LOCAL_PEERTOKEN` is the *current* token of the process that last touched the socket. So at accept the host samples the audit token, then the bytes already buffered (`FIONREAD`, N0); on each app-only call it requires the request to start at or after N0 and the current token to equal the accept-time one byte for byte, and runs the code-signature check (`SecCode`) on that token.
- **App-only calls and the inherited-socket swap** (ada, 2026-11-08: host review H1): first thing in the app's main, before any output or threads, the app fstats fds 0-2 and dup2s `/dev/null` over any that is a socket; it never reads or writes an fd it didn't open. After sending an app-only call, the client sends nothing else on that connection until its answer arrives. The reason: a process that inherited a copy of the app's connection (or handed the app one on its stdio) can touch it, and any touch, even a read or a zero-length write, moves `LOCAL_PEERTOKEN` to the toucher. So on each app-only call the host also refuses (`not_authorized`) unless nothing has arrived after the request line: nothing buffered past it in its reader and nothing waiting on the socket (`FIONREAD` 0), checked on the connection's reader before it reads anything more.
- **Host decisions** (ada, 2026-11-08): `office.app_token` gives the token of a `starting` or `active` office. The give-up rule counts 10 failed runs including the first start, and a run that stays up 60 s after `ready` clears the count. `office.status_changed` is level-triggered: each event carries the office's state at that moment, intermediate states may be skipped and duplicates are possible, so clients replace their record idempotently. `office.restart` on an office the host couldn't supervise tries to start it now, and answers `unavailable` if it still can't. A non-fatal `failed` followed by the ready timeout reports that code's reason, not `start_timeout`. `invalid_name` and `already_exists` from an `--office` run are never restarted on their own: the office stops `unavailable` with `invalid_request`. `key_missing` follows org-proto: `unavailable`, no automatic restart (so it doesn't count toward the give-up rule), and `office.restart` retries it.
- **Key custody** (ada, 2026-11-08): the host creates and holds every office's 32-byte database key.
  - **Where:** release builds keep it in the Data Protection keychain, a generic password with service
    `org.agentic.office-key` and account = the office id, in an access group from the signed host's
    `keychain-access-groups` entitlement, `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`, not
    synchronizable. Other code gets `errSecItemNotFound`, never a prompt. Debug builds and tests use a
    dev-keys file store instead and never write a real Keychain item.
  - **When:** only a create makes a key, and the host makes it, before the office's first `--create`.
    The host loads it before every `--office` and `--discard` and sends it on the first line (see 3).
    It never re-creates a key for an existing office: a missing key item is `key_missing`, set by the
    host without a spawn, and so is a key that doesn't open the databases (`failed: key_missing`).
    Either way the office is `unavailable` with `key_missing`, with no automatic restart;
    `office.restart` retries it.
  - **Deleting:** only after the server reports `discarded` for that office. After a failed create the
    host keeps the key, and it adopts the office or discards it with the key. **An orphan sweep** at host
    startup (release builds only; debug builds skip it and log that) finds the key items whose office
    has no folder under `offices/`, isn't under `offices/.refused/` and isn't named by any
    `office.json` there. It never deletes on first sight (ada): it marks an orphan in a host-side
    record (`<home>/orphan-keys.json`, 0600, written atomically: `{"v":1,"first_seen_ms":{"<office id>": <unix ms>}}`, office ids and first-seen times only, never key bytes) and deletes the item only if it's
    still orphaned at a host start 7 or more days later. An item whose folder comes back is unmarked.
    A record it can't read means no deletions that run (it starts over), and a clock before 2026 skips the sweep. Both steps are logged, with ids only.
    If anything it must look at can't be read (`offices/`, `offices/.refused/`, any `office.json`
    there, an office's folder, or the key store's list), it doesn't sweep at all, and logs a WARNING
    at every start until that's fixed: one unreadable `office.json` stops all orphan deletion. A
    folder that can't be checked is never counted as an orphan. (amended 2026-11-08: review L1, ada)
    It also marks and deletes nothing, with a WARNING, when `offices/` is empty or unreadable while
    the key store holds keys, or when more than half of the keys would be orphans at once: a home
    moved, restored or re-created elsewhere makes every key look orphaned, and 7 days later the
    sweep would otherwise delete them all.
  - **The release home is pinned:** a release host refuses to start unless the shared home rule
    (`ORG_OFFICE_HOME`, then `HOME`) resolves to the account's home,
    `<home dir>/Library/Application Support/agentic.org`, with the home dir taken from the user
    database, not the environment. So one host can never take another home's offices for orphans
    (every release host of the account shares the Keychain group).
  - **Only the signed server gets a key** (amended 2026-11-08: whole-branch review H1): before it
    writes a server's first line (create, start or discard), a release host checks the child it just
    spawned, before the child has run a single instruction: every server is spawned **suspended**
    (`POSIX_SPAWN_START_SUSPENDED`, and with only its three pipes, `POSIX_SPAWN_CLOEXEC_DEFAULT`),
    because an unvetted image's first code could start a helper that inherits stdin, then `exec` the
    genuine server and pass a later check. The host takes the suspended child's audit token
    (`task_name_for_pid`, then `TASK_AUDIT_TOKEN`; it names the image the exec loaded), gets its code
    object through it (`SecCodeCopyGuestWithAttributes`), and requires `identifier
    "com.ellosoft.agentic-org.org-server" and anchor apple generic` with the Developer ID
    markers and `certificate leaf[subject.OU] = "<team id>"`, the hardened runtime and none of the
    rejected entitlements (the same checks as the app's, ADR-0003 §2.1). Only then does it resume the
    child (`SIGCONT`); a child that fails is killed while still suspended, so it never ran. Right
    before the line, it takes the token again and refuses a child that changed (an `exec` after the
    check). A child that fails, or whose token can't be taken, is killed and reaped and gets
    nothing; the start counts as `storage_error`. A release host with no team id compiled in starts no server at all (logged as a
    WARNING). This stops a same-user process that runs the signed host from a copy, a symlink or a
    hard link next to its own `agentic-org-office` from receiving the keys, with one **residual
    risk (open)**: any same-user process can resume a suspended child (`SIGCONT`). An attacker that
    resumes the child before the host takes its token (stopping or starving the host, e.g. a copy
    of the host it launched itself, makes that easy), lets it fork a helper that
    keeps stdin and turn itself into the genuine server (`POSIX_SPAWN_SETEXEC` with
    `START_SUSPENDED`), passes the check and the helper reads the key.
    Closing that needs the key handed only to an authenticated reader (e.g. a per-spawn socket
    whose peer audit token must equal the admitted one): a launch-contract and server change.
    Debug builds don't check (their servers are unsigned; logged at startup).
  - **Historical no-desktop-migration decision:** no compatibility import is required from the undeployed desktop. This does not authorize discarding real local office state, protected source journals or credentials. Use disposable development fixtures only.
  - **This Mac only** (the user's decision): keys never leave it. A "move this office" export, protected
    by the owner's recovery code, comes later as a feature.
  - **Release trust inputs are compiled in:** the team id, the bundle id, the Keychain access group,
    and the office-home rules. The server is the `agentic-org-office` next to the host's own
    executable (canonical path), and is admitted only if it passes the signature check. Release
    builds refuse `--home`, `--server-bin` and `--app-team-id`; debug builds keep them. (amended
    2026-11-08: review M1) At startup a release host also reads its own signing information once
    and refuses to start unless its Keychain access group is in its `keychain-access-groups`
    entitlement **and** it runs with the hardened runtime and without
    `com.apple.security.get-task-allow`, `com.apple.security.cs.allow-dyld-environment-variables` or
    `com.apple.security.cs.disable-library-validation`: a host other code can get into would give
    every key away. (amended 2026-11-08: review L5) A release-profile build with debug assertions on
    (the profile, `-C debug-assertions` or `-C opt-level=0`) fails to compile (`build.rs` marks release-profile builds and `lib.rs` has a `compile_error!` for that cfg with `debug_assertions`); so does any build for the app bundle (`ORG_BUNDLE=1`, which the bundling must set) with debug assertions on, whatever its profile (`ORG_BUNDLE` must be unset or exactly `1`; documentation and doctest builds are exempt, since they ship nothing): it would ship every development
    path.
  - **Release prerequisite** (ada): verify the `keychain-access-groups` entitlement on the first signed
    build. If it needs a provisioning profile, the host ships as a helper app bundle
    (`Contents/Library/LoginItems/…`). The legacy-ACL fallback is used only if the Data Protection
    keychain proves impossible, and then it goes back to ada first.
  - **Why:** a same-user process could stop the host and run the signed `agentic-org-office` itself with
    its own app token, and a Keychain item owned by the server would trust that server. With the host
    holding the keys, a server started by anyone else gets no key and can't open `org.db`.
- **host.sock:** 0600 and `run/` 0700. On accept, the host also checks the peer's uid (`getpeereid`) is its own (defence in depth).
- **Fatal status and restart:** `unavailable` (with `status_reason`) lasts until the owner calls `office.restart` from the app, or the host restarts (for `unsupported_version` and `invalid_request`, a host restart with a fixed binary only). `office.restart` starts an `offline` or `unavailable` office now; for any other status it returns `conflict`. The exception is a terminal satellite join (`join_*`), which is `unavailable` but can't be restarted: `office.restart` returns `unavailable` for it. The `join_*` reasons are shown by the CLI (`status`, `doctor`); their browser copy is a spec follow-up (offices-and-teams-ux §3.3). It's app-only: the host checks the caller's code signature, as for `office.app_token` (`not_authorized` otherwise). It clears `unavailable`, resets the backoff and the give-up count (the failure count goes to 0), and retries `key_missing` (it goes straight back if the key is still missing); it refuses `unsupported_version` and `invalid_request` (`unavailable`). Every status change is pushed as `office.status_changed` on `subscribe`.
- **Startup:** the host binds `host.sock` before it spawns anything. (amended 2026-11-08: key custody) It
  runs the orphan key sweep in release builds (see Key custody). It answers `hello`, `office.list` and `subscribe` at once. `office.list` shows every catalog office (except a terminal satellite join, shown `unavailable`) as `starting` from the start, before the host has spawned it (this widens `starting`, see Status per office). Calls that need a settled catalog (`organization.create`, `office.restart`, and `office.app_token` for an office still being discarded) wait for the startup discards, at most 30 s, then return `not_ready` ("host starting"; temporary, retry shortly).
- **A create that stored its office but didn't report `created`** (it died, or sent a `failed` line after `office.json` was in place): if `office.json` matches the plan (id, org_id, org_name, office_name, role), the host adopts it: the create succeeds (status `starting`), it starts `--office`, and it emits `organization.created`. It never runs `--discard` once `office.json` is present. A mismatch is a refused create.
- **Spawning:** `org-supervise` is the single spawn path: every process the host or a server starts goes through it (`posix_spawn` with `POSIX_SPAWN_CLOEXEC_DEFAULT` and explicit file actions for fds 0/1/2), so a child inherits only the fds it's given. That needs libc FFI; the `unsafe` is confined to `org-supervise`. It complements the server's fd swap (see 2), which is the structural guarantee and doesn't depend on it.
- **Give-up rule:** 10 failed restarts within 30 minutes → `unavailable` with `restarts_exhausted` (no more automatic restarts until `office.restart`, which resets the count). A **failed restart** is an exit before 60 s of uptime: a crash, EOF without `ready`, a `failed` line, or the ready timeout. A crash after 60 s of uptime counts as a fresh start, not a failure. While `offline` with a restart pending, `next_restart_at` says when.
- **Socket length:** every office socket path in a home has the same length (12-character handles),
  so the host checks `host.sock` and a sample `run/XXXXXXXXXXXX.sock` against the 103-byte limit **at
  startup** and refuses to start with a clear message. No per-create error code is needed.

## 5. Historical phase-1 library layout

The source layout below describes the inspected phase-1 baseline. The implemented
extraction moves host modules to `crates/org-host`, CLI/serve composition and host
integration tests to `apps/agentic-org`, and the worker to
`apps/agentic-org-office`. See [executable migration](executable-migration.md).
- `apps/org-office-host/`: `src/main.rs` (thin: args, location, signals, `Host::run`) and
  `src/lib.rs` with modules `catalog` (scan, office.json, name uniqueness, create), `supervisor`
  (spawn, the pipe protocol, status, backoff, shutdown), `service` (org-proto handlers: `hello`,
  `organization.create`, `organization.list`, `office.list`, `office.restart`, `office.app_token`,
  `subscribe`), and (amended 2026-11-08: key custody) `keys` (the key store: the Data Protection
  keychain in release builds, the dev-keys files in debug builds).
- The supervisor takes the server program's path as config (default: the `agentic-org-office` next to
  the host binary in the app bundle; a release build uses only that one, next to the host's own
  executable (canonical path), and admits it only if it passes the signature check, see Key
  custody). The host's tests use a **fake office server** (a test helper binary
  that follows section 3), so the host is tested without SQLCipher or the Keychain (its keys go to the
  dev-keys files or an in-memory store), and doesn't wait for server-dev's work to land. The fake must never ship: it's an example or a dev-only bin the bundle never picks up.
- The host doesn't depend on `org-office` (no SQLCipher in the login service) or on
  `agentic-org-office`'s library (an app doesn't depend on an app).

## 6. Decisions (ada, 2026-11-08)
1. **The pipe protocol and `office.json` types live in a new contract crate, `crates/org-office-launch`**, not `org-proto` (the client contract stays small). It depends only on `org-proto`, `serde` and `serde_json`. The lead owns it as a contract. It's additive and versioned (`"v":1`, now 2 for the pipe lines since key custody; lines after the first are ignored, and an unknown op on the first line is `invalid_request`; unknown events and fields are ignored; unknown failed codes count as `storage_error`).
2. **The name rule is `org_proto::OrganizationName`** (ada, branch `ada/proto-org-name`): `parse(&str) -> Result<_, NameError>`, `.as_str()` (trimmed), `.key()` (Unicode lowercase of the trimmed name, no normalization). `NameError` is `Empty | TooLong | ControlChar` with `.message()`, mapping to `invalid_name`.
3. **The minimal listener** lives in one module (`service/listener.rs`), uses only `org_proto::frame` and the typed `Call`/`EventKind`, and grows no router. It's swapped for server-dev's shared crate once that's extracted.
4. The design's "`--create --role hq`" (line 35) is wrong. Only `--create` goes on the command line. ada fixes the wording.
5. **`--discard`** (server-dev's proposal, agreed by River and ada 2026-11-08) replaces the host removing unfinished folders itself. (amended 2026-11-08: key custody) The server no longer removes keys: the host deletes its key item after `discarded`, and its startup sweep (release builds) removes key items still without a folder 7 days after it first saw them. It never destroys an office that finished (office.json or a committed organization), or one it can't check.
6. **The app token** (ADR-0003 §2.1, ada 2026-11-08) is ephemeral and goes in the first line of every start.
7. **Refused creates are quarantined**, not deleted: the requested id's folder moves to `offices/.refused/` (from the review of `org-office-launch` @ 26ed861).
8. **Key custody** (ada, 2026-11-08): the host creates and holds every office's database key and sends it on the launch pipe (launch version 2); the server never touches a key store. See "Key custody" in 4.
