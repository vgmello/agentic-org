# Design: the host and the office worker

How the host (`crates/org-host`, run by `agentic-org serve`) creates, starts,
supervises and stops each office worker (`agentic-org-office`), and what crosses the
boundary between them. The [standalone design](standalone-server.md) places both in
the instance; the [data contract](organization-data/README.md) owns what the office
stores. Platform-specific parts are the adapters of
[ADR-0017](../adr/0017-linux-and-container-hosting.md): process admission, the
credential provider, confinement and the service lifecycle each have a macOS and a
Linux implementation.

## Responsibilities

| The host | The office worker |
| --- | --- |
| The instance and host locks, the office catalog, accounts and sessions | Its office lock and every domain rule |
| One office secrets key per office, in its credential provider ([ADR-0016](../adr/0016-no-application-level-data-encryption.md)) | Its private keys, wrapped with that key |
| Starting, admitting, supervising and stopping workers and the web process | Its data under the [storage tree](organization-data/README.md#shared-storage-contract), its local socket and its agents |
| Grants that carry a person's principal to an office | Verifying those grants and authorizing every call |
| Typed instance administration (create organization, add office, restart) | Answering the host's events over the launch connection |

A person's request reaches the office with that person's principal and a grant from the
host, which the office verifies ([ADR-0006](../adr/0006-selected-task-acceptance.md),
[ADR-0012](../adr/0012-office-channel-lifecycle.md)). There is no application token.
Creating an organization needs the signed-in server administrator; setup creates
none. An organization owner or an office certificate cannot create another
organization. A browser request never selects a raw local method or a storage
path. The web process holds no office key and no key-store entitlement.

## 1. The office catalog

Each office has a private directory owned by the instance, holding `office.lock` and
`office.json`. The office's data lives under the organization's data root.

| Item | Written by | Read by |
| --- | --- | --- |
| The host lock | The host: an exclusive lock held for its whole life. A second host on the same instance exits at once, so one host creates every office. Inside it, an in-process catalog mutex guards a create's name check and reservation. | The host |
| The office directory | The worker creates it (`0700`) on create | The host lists the directory names |
| `office.lock` | The worker: held from before it opens anything until it exits | Nobody else |
| `office.json` | The worker, **written last** on create (temporary file, fsync, rename, directory fsync): this is the "stored" marker | The host |
| The office secrets key | The host creates it before the office's first create, and deletes it only after `discarded` | The host, which sends it over the launch connection |

**The catalog** is the set of office directories, named by a valid ID, that hold a
valid `office.json`. A satellite whose `office.json` records a terminal join state stays
in the catalog as `unavailable` with its `join_*` reason and is never started. Any other
directory (such as `.refused`) is never started or discarded.

- **No `office.json`:** an unfinished create. The host never starts it. At startup, or
  after that create failed before `office.json` was in place, it runs
  `agentic-org-office --discard <id>`. The worker takes the lock and removes the
  directory only if the office never finished; with no directory it creates nothing and
  reports `discarded`. It refuses (`already_exists`) a directory with `office.json` or
  with data holding a committed organization, except a satellite whose join never
  completed (`role: satellite`, a `pending` or terminal state and no applied snapshot),
  removed with `office.remove_pending`. Data it cannot open or check means it refuses
  (`storage_error`). On a refusal the host logs it and leaves the directory, and its
  key, for a person, so a real office that lost its `office.json` is never destroyed
  automatically.
- **An unreadable or invalid `office.json`:** logged and left alone; neither started
  nor discarded.
- **An `office.json` whose `office_id` is not the directory's name:** never started;
  the directory moves to `.refused/<id>`, nothing deleted, the key kept.
- **A refused create:** if a worker reports a `created` office other than the one asked
  for (office ID, organization ID or name, office name or role), the host stops it and
  moves the directory of the ID it asked for to `.refused/`. It never touches a
  directory of a different ID the worker reported.

**Lock held means running.** An office's lock is held if and only if a worker runs that
office. A worker waits up to 10 s for the lock at start, then fails with `in_use`, so an
old worker and its replacement never overlap. The host never takes an office's lock and
never infers status from a lock probe. Every descriptor the worker opens is
close-on-exec, so no child keeps the lock.

### `office.json`

```json
{"v":1,"office_id":"0193…","office_name":"HQ","role":"hq",
 "org_id":"0193…","org_name":"Acme","created_at":"2026-11-08T02:10:00.000Z"}
```

No secrets. At most 64 KiB, mode `0600`, written atomically as above. Readers open it
without following symlinks, only as a regular file owned by the service user, and check
that `office_id` names its directory. IDs are UUIDv7. Unknown fields and an unknown role
are kept when a worker rewrites it. A satellite's `office.json` also carries `state`:
`pending` while its join is open, then `active`, or a terminal `expired`, `cancelled` or
`rejected`; the worker writes it. On `role: satellite`, an absent or unknown `state` is
treated as invalid.

## 2. Starting a worker

```
agentic-org-office --create          # create a new office from the launch handoff, then serve it
agentic-org-office --office <id>     # serve an existing office
agentic-org-office --discard <id>    # remove an unfinished office, then exit (the host then deletes its key)
```

- Nothing but the office ID goes on the command line.
- **stdin carries only the first line.** The worker drains stdin after it and never
  treats stdin as a lifetime signal. **The rendezvous connection is the host's
  lifetime:** when it ends, the host is gone and the worker stops accepting, finishes
  in-flight writes, removes its socket, releases the lock and exits.
- **stdout is untrusted** and carries nothing the host acts on; logs go to stderr, which
  the host writes to the office's log (`0600`, rotated at 10 MiB, one old file kept).
- **No child can read or write the control channel.** First thing at startup, before
  any thread or child starts, the worker duplicates descriptors 0 and 1 to new
  close-on-exec descriptors and reopens 0 and 1 on `/dev/null`. `org-supervise` is the
  single spawn path for every process the host or a worker starts: `posix_spawn` with
  `POSIX_SPAWN_CLOEXEC_DEFAULT` and explicit actions for descriptors 0 to 2. The host
  opens its own descriptors close-on-exec.

### Admission and the verified rendezvous

There is one launch contract, in `crates/org-office-launch`. It covers every child the host
supervises: office workers and the web process (owner-delegated decision 2026-11-17). No secret is ever on
argv, in the environment or on the first line. The steps for an office worker are below; the web process
uses the same steps with its own identifier, image and listener handoff (see "The web process" below):

1. The host starts the worker, with only its three pipes, through the platform's
   process-admission adapter. On macOS the child is spawned **suspended**. On Linux
   it is started from a sealed, execute-only copy of the pinned image whose digest the
   host has already checked, so there is nothing to suspend
   ([Linux host design](linux-host.md#31-process-admission)).
2. The adapter checks that the child is the intended program. On macOS: the child's
   audit token, its code object, the identifier `com.vgmello.agentic-org.office` with
   the compiled Developer ID team, the hardened runtime and none of the refused
   entitlements; a child that fails is killed while still suspended. On Linux: the
   image's SHA-256 against the release manifest before `exec`, no loader variables in
   its environment, and the child held by a pidfd.
3. Only then does the host create a per-spawn **rendezvous** socket and write the
   worker's first line on stdin. The first line holds **no secret**: the instance, the
   office, the rendezvous path and, for a create, the organization, the names, the role,
   the owner's security principal and the HQ creation context.
4. The worker connects to the rendezvous and sends `ReadyForSecrets`. The host checks
   that the connected peer is the admitted child (on macOS the same audit token and
   PID), then answers with one handoff line carrying the office secrets key, the grant
   key and, for an HQ create, the recovery code, followed by one message carrying the
   descriptors it names. A `--discard` run gets no descriptor. The worker reads only
   with `recvmsg` and a control buffer sized for the count; truncation or another count
   is fatal.
5. The worker's events travel over the rendezvous connection: `created`, `ready`,
   `discarded`, `failed`, `joined`, and `child`/`child_gone` for each agent runtime it
   spawns and reaps (pgid, pid, start time and mark, so the host can clean up orphans if
   the worker dies). Descriptor receipts are authenticated there too. The host ignores
   an event it does not know.
6. After the first line, the worker ignores every further stdin line, so nothing can
   swap the handoff mid-life.

A secret never appears in argv, the environment, a log or a file. The host holds the
office secrets key in memory only from loading it until the handoff is written; the
worker keeps one copy in memory, zeroed on drop.

The transfer socket (a datagram socket pair, host to worker) carries channel records:
a JSON header of at most 1024 bytes and zero or one descriptor. Web channels arrive as
`ChannelKind::Web` records and native ones as `ChannelKind::Local`. An office
accepts at most eight channels, at most seven of them from the web process, so one
stays free for a local caller. Session revocation arrives as a `Revoked` record. Package
export for recovery uses a separate `recovery_public_read` record with one connected
descriptor and a one-shot 16 KiB public exchange ([ADR-0011](../adr/0011-protected-recovery-delivery.md)).

The host also allocates the office's temp handle and passes it over the rendezvous, and
tells the worker when the instance's layout changes so that it refreshes agents'
sandbox policies ([agent sessions](agent-sessions.md#managed-state)).

### HQ creation context

An HQ create carries an `hq_creation` object with exactly
`{version:1, operation_id, escrow_profile:1, public_root}`, with a UUIDv7 operation ID
and the absolute canonical state root. The protected host provisioning operation
supplies it, the owner's security principal and the recovery code. Unsupported versions
and duplicates fail before any change.

The worker freezes the nonsecret request identity in `hq-creation.v1.json` under the
office lock before preparing anything. One local transaction then stores the operation
and owner binding, the CA private key wrapped with the office secrets key (an
`org_pki::wrap` record bound to the organization, the office and the CA purpose;
ADR-0016 decision 3), the public envelope and the completion version. That record is excluded from replication and
journal capture. An interrupted preparation resumes only the identical intent and
verifies retained material instead of generating it again; foreign data, missing
committed material or changed input stay intact and need recovery
([ADR-0007](../adr/0007-hq-creation-escrow.md)).

The public root must exist, resolve without symlinks or aliases, and be disjoint from
the office's private directory; it is frozen in the intent, never taken from a browser.
Before `created`, the exact committed public envelope is published there immutably as
`security/ca-escrow.v1.bin`. Reopening repairs a missing file from the retained record;
conflicting bytes require recovery.

### Provisioning an organization

The native `organization.provision.v1` call carries the session, a UUIDv7
`operation_id` and a plan with explicit `name`, `slug`, `repository_remote_url`,
and `state_branch`. It asks for no team. The host rechecks the live administrator and
binds the operation and its fingerprint to that account. Creation makes one top-level
team named after the organization, the creator's person record in it as its lead, and
the built-in HR employee (not runnable without its own execution configuration). The
creator's account owns the organization; ownership is a permission of the account at
the host, not a field on the person.

The host freezes the UUIDv7 entity IDs and the key fingerprint in its protected
provisioning reservation, with the recovery code in credential custody under an opaque
handle. A retry verifies that binding and the original office key and never generates
replacements for missing committed custody. One gate per operation serializes worker
attempts. The handoff carries the exact prepared plan and IDs, which the worker's
intent keeps. The host installs writer custody before admitting the office and keeps
the outcome pending until the worker has written the initial files and is ready. The
initial canonical files and the directory generation enter the office's acceptance journal
together. Remote connectivity and publication, and HR execution, are reported as
unverified.

### The web process

The verified release's `agentic-org-web --serve` loads its `ui/` build before
confinement. The host spawns it with `POSIX_SPAWN_CLOEXEC_DEFAULT`, keeping only its
three pipes, and the child checks that exactly descriptors 0 to 2 remain; anything
else makes it exit before readiness. It connects only to its exact
rendezvous, and reads nothing from it and reports nothing until confinement holds:
on macOS it confines itself first ([ADR-0008](../adr/0008-gateway-macos-sandbox.md));
on Linux it connects first, because the seccomp filter then forbids any new
connection, and applies Landlock and seccomp before reading
([Linux host design](linux-host.md#33-confinement-of-the-web-process)). It accepts no descriptors in the
header; the next marker carries the transfer socket and then the listener, each checked
for socket type and close-on-exec before `fds.received`. The same connection then
carries host control; neither `host.sock` nor an office path is a fallback. `tls.fetch`
supplies the verified listener configuration before HTTP accepts. Host EOF ends the web
process.

On macOS the web process is admitted as `com.vgmello.agentic-org.web`, with
the compiled team, hardened runtime, no debug or injection entitlements and no Keychain
access group. The host rechecks the reader's audit token, image and kernel sandbox
before sending either descriptor. Browser sessions are stored at the host and survive a
control EOF ([ADR-0013](../adr/0013-stored-browser-sessions.md)); native sessions end
with their connection.

The instance configuration has an optional `browser` listener snapshot, written by
protected setup under the instance lock and never taken from HTTP. A bind uses one
validated snapshot. Loopback HTTP is admitted; HTTPS is unavailable until the host
verifies its certificate pair, and it is never downgraded. The browser service starts
once the instance is registered and its host authentication (registry) is present; no
organization or Active HQ is required, because the person registers from setup's link and
creates the organization in the browser, and routes that need an organization or office
answer their own "not yet" state (for example `organizations: []`). It restarts with
bounded backoff and is killed and reaped at host shutdown. Its failure leaves native control available.

### `failed` codes

| Code | Meaning | What the host does |
| --- | --- | --- |
| `invalid_request` | A bad first line or bad arguments | Fatal: `unavailable`; starts again only after a host restart with a fixed binary |
| `unsupported_version` | Unknown launch version | Fatal, as above |
| `invalid_name` | The name breaks the rule | The create fails with `invalid_name` |
| `already_exists` | `--create` but the directory or `office.json` exists; `--discard` of a finished office | The create fails; a discard is logged and the directory left |
| `in_use` | Lock not acquired within 10 s | Restart with backoff |
| `storage_error` | Anything else; `--discard` of data it cannot open or check | Restart with backoff (a create fails; a discard is logged and left) |

A message is for people: never a key, path or SQL. Unknown codes count as
`storage_error`. A missing office secrets key is not a failure: the office opens and
serves its data, and only functions that need a private key report unavailable
(ADR-0016, ENC-1).

The worker never deletes a key, and the host keeps the key after a failed `--create`. If
`office.json` is in place, the office is stored and the host adopts it; otherwise the
host runs `--discard` and deletes the key only after `discarded`. Exit or EOF on the
rendezvous before `ready` without a `failed` event is a crash: restart with backoff. A
satellite whose `office.json` records a terminal join state exits cleanly and is not
restarted.

## 3. What the host does

- **Status per office:** `starting` (spawned, no `ready` yet; at host startup also every
  catalog office not yet spawned) → `active` (`ready`, child alive) → `offline` (exited,
  a restart pending with `next_restart_at`, unless stopped by the owner) →
  `unavailable` (a fatal code, or restarts given up). The `ready` wait is 30 s, then kill
  and backoff. A pending satellite join is `starting` with `join_pending` until the
  worker reports `joined`, after approval and the initial synchronization; the worker
  writes `state: active` first.
- **Status reasons:**

  | What happened | `status_reason` | Status |
  | --- | --- | --- |
  | Rendezvous EOF without a `failed` event | `crashed` | `offline` |
  | No `ready` within 30 s | `start_timeout` | `offline` |
  | `failed: in_use` | `in_use` | `offline` |
  | `failed: storage_error`, or an unknown code | `storage_error` | `offline` |
  | `failed: unsupported_version` | `unsupported_version` | `unavailable` |
  | `failed: invalid_request` | `invalid_request` | `unavailable` |
  | The give-up rule | `restarts_exhausted` | `unavailable` |
  | The owner stopped it | `stopped_by_owner` | `offline`, no restart pending |
  | A satellite join waiting for approval or synchronization | `join_pending` | `starting` |
  | A satellite join ended | `join_expired`, `join_cancelled` or `join_rejected` | `unavailable`, no restart |

  A non-fatal `failed` followed by the ready timeout reports that code, not
  `start_timeout`. `invalid_name` or `already_exists` from a normal run is never
  restarted: it becomes `unavailable` with reason `invalid_request`.

- **Restart:** backoff 1, 2, 4 … 60 s, reset after 60 s of uptime. Fatal codes never
  restart on their own.
- **Give-up rule:** 10 failed runs within 30 minutes, counting the first start, make the
  office `unavailable` with `restarts_exhausted`. A failed run is an exit before 60 s of
  uptime. `office.restart` resets the count.
- **`office.restart`** starts an `offline` or `unavailable` office now and answers
  `conflict` for any other status, `unavailable` for a terminal satellite join, and
  refuses `unsupported_version` and `invalid_request`. It needs the organization's owner or the server's administrator.
- **`office.status_changed`** is level-triggered: each event carries the state at that
  moment, intermediate states may be skipped and duplicates are possible, so clients
  replace their record idempotently.
- **Shutdown** (the service manager's stop signal, or the host exiting): close every
  child's stdin and its rendezvous connection (the worker stops on the rendezvous
  closing), wait up to 10 s, then SIGKILL what is left, and reap.
- **Create:** under the catalog mutex only, check the name and its uniqueness among the
  instance's organizations (ignoring case, counting creates in flight), make the IDs,
  reserve the name and release the mutex. Outside it: create the office secrets key and
  store it, spawn `--create`, and wait up to 30 s for `created` or `failed`. On failure,
  keep the key and run `--discard`, unless `office.json` is present. On success, emit
  `organization.created`.
- **Adopting a stored create:** if a create stored its office but did not report
  `created`, and `office.json` matches the plan, the host adopts it, starts `--office` and
  emits `organization.created`. It never runs `--discard` once `office.json` exists. A
  mismatch is a refused create.
- **Startup:** the host binds its socket before spawning anything, runs the orphan key
  sweep, and answers `hello`, `office.list` and `subscribe` at once. `office.list` shows
  every catalog office as `starting` until it is spawned. Calls that need a settled
  catalog (`organization.create`, `office.restart`) wait for the startup discards for at
  most 30 s, then return `not_ready`.
- **Sockets:** `host.sock` is `0600` in a `0700` directory, and the host also checks the
  peer's user ID. Every office socket path in an instance has the same length, so the
  host checks one sample against the 103-byte macOS limit at startup and refuses to
  start if it does not fit.
- **The web process reads through the office channel.** The host serves the work reads
  the web needs over the office channel, so the browser reads tasks, projects and people
  by the same path as commands.

### The office secrets key

- **What:** one random key per office, used by the office only to wrap its private keys
  (ADR-0016). It never opens data.
- **Where:** in the host's credential provider. On macOS, the Data Protection keychain:
  a generic password with service `org.agentic.office-key` and account the office ID, in
  the access group of the signed host's `keychain-access-groups` entitlement,
  `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`, not synchronizable. On Linux, a
  host-only directory with owner-only permissions, or a mounted secret volume in a
  container. Debug builds and tests use a development file store and never touch a real
  keychain.
- **When:** only a create makes a key, before the office's first `--create`. The host
  loads it before every start and discard. It never re-creates a key for an existing
  office.
- **Deleting:** only after the worker reports `discarded`. An **orphan sweep** at host
  startup (release builds) marks key items whose office has no directory, is not under
  `.refused/` and is named by no `office.json`, in a host-side record of office IDs and
  first-seen times, and deletes an item only if it is still orphaned at a start seven or
  more days later. An item whose directory comes back is unmarked. If anything it must
  look at cannot be read, it deletes nothing and warns at every start. It also deletes
  nothing, with a warning, when the office directories are empty or unreadable while the
  store holds keys, or when more than half of the keys would be orphans at once. A host
  sweeps only its own instance's catalog.
- **Release trust inputs are compiled in:** the team ID, the bundle ID, the Keychain
  access group and the instance rules. A release host refuses development overrides of
  its paths, and at startup refuses to run unless its access group is in its
  entitlements and it has the hardened runtime without `get-task-allow`,
  `allow-dyld-environment-variables` or `disable-library-validation`. A release-profile
  build with debug assertions on fails to compile.
- **Only the admitted worker receives a key,** and only over the authenticated
  rendezvous after the peer check of step 4. On macOS, a same-user process that
  resumes a suspended child before the host checks it cannot receive the key, because the key goes
  only to the connected peer that matches the admitted identity.

## 4. Recovery before launch

Restart and rebuild verify the trusted organization, HQ and CA identity, journal
envelope hashes, contiguous streams and incarnations, the highest authority epoch,
completed snapshot bytes and actor-bound command outcomes. `outcome_unknown` is
reconciled before any side effect repeats. A copied checkpoint cannot promote an office.

Before launching an agent the office restores HQ's placement generation and its
execution journals, binds credentials again and verifies old-process cleanup or an
explicit machine fence. A restored PID is never signalled without a matching ownership
mark, generation and machine, boot and start identity. Deleting a cache does not prove
the old process stopped; missing evidence means recovery is required. The host keeps
the `child` events in memory and, when a worker dies, cleans up those processes before
it restarts the worker; it never races a live worker over its agents.

## 5. Code layout

- `crates/org-office-launch`: the launch contract, `office.json` types and the transfer
  records. It depends only on `org-proto`, `serde` and `serde_json`.
- `crates/org-host`: `catalog` (scan, `office.json`, name uniqueness, create),
  `supervisor` (spawn, the launch protocol, status, backoff, shutdown), `service` (the
  host socket), `keys` and `host_secrets` (credential providers), `spawn` (process
  admission).
- `keys::OfficeSecretsKeys` is the host's own contract for the office secrets keys
  (create add-only, load, delete, list; errors carry codes, never key bytes). Its
  adapters: the macOS Data Protection keychain (`keys/keychain.rs`, service
  `org.agentic.office-key`), owner-only files in a development folder for debug builds
  (`keys/dev.rs`), an in-memory store with fault injection for tests (`keys/memory.rs`),
  and, for every other operating system, an adapter that refuses each call with a
  reported reason (`keys/unsupported.rs`). `keys/platform.rs` holds the only
  `cfg(target_os)` choice and owns the Keychain preconditions (the compiled-in access
  group, its entitlement and the host's own hardening). Another operating system gets the
  refusing adapter before any of them is checked, so its operator sees "this operating
  system has no office secrets key store", never a macOS-only reason. The supervisor and
  provisioning never branch on the platform.
- Storage: every database (host registry, `org.db`, `office.db`) is plain SQLite from
  `rusqlite`'s `bundled` feature, protected by owner-only file permissions; there is no
  SQLCipher dependency, database key or initialization fence (ADR-0016).
- The organization name rule is `org_proto::OrganizationName`: trimmed, Unicode
  lowercase as its key, refusing empty, too long and control characters
  (`invalid_name`).
- The host does not depend on `org-office` or on `agentic-org-office`'s library. Host
  tests use a fake worker binary that follows this contract; it never ships.

## What the code does today

| Increment | Requirement | Code today |
| --- | --- | --- |
| A1 | One launch contract, the verified rendezvous | Done: the rendezvous at the `org-office-launch` crate root (`LAUNCH_VERSION = 1`, `crates/org-office-launch/src/contract.rs`) is the only form; a `"v":2` or `"v":3` first line is refused as `unsupported_version`, and a discard gets no secret. |
| A1, A3 | People are authorized by grants; no application token | The application token is deleted (A1.8): the launch line carries none, `org-proto` defines no token type and no `office.app_token` call, and `org-client` has no app bind. A person or the owner acts only through a host-issued grant that the worker verifies. `msg.bind` binds agents only: it refuses a person or the owner (`not_authorized`) and refuses the retired `app_token` field on presence (`invalid_request`, `org_proto::retired`). The host refuses `office.app_token` like any call it does not serve. The host's `app_auth.rs` (`AppPolicy`) is not a token: it checks the calling process's code signature for `admin` and `recovery` calls, and stays. |
| A3 | The web process starts through the same launch contract as an office worker | The web process has its own rendezvous lines (`GatewayFirstLine`, `GatewayReadyForSecrets` in `crates/org-host-control/src/gateway.rs`), separate from the office worker's. |
| A3 | Seven web channels per office | Done: `MAX_WEB_CHANNELS` is 7 and a test refuses the eighth. |
| A3 | The host serves the web's work reads through the office channel | `host.sock` dispatches only `hello` and `instance.status`. |
| A1 | No database key; the office secrets key instead | The office databases are plain SQLite and open without a key; the launch handoff carries the per-office key as `office_secrets_key` (required on a create, `null` on a start when the host has none, absent on a discard), which the worker uses only to wrap and unwrap the CA private key. Without it, or with a wrong one, the office serves its data and only the CA is unavailable. The `key_missing` status reason and launch code are gone (a `key_missing` from an older server decodes as a retried `storage_error`). The host keeps the keys behind its `OfficeSecretsKeys` contract (§5); an operating system without an adapter refuses. |
| X1a to X6 | macOS and Linux, including a container ([Linux host design](linux-host.md)) | Workers start only on macOS (`crates/org-host/src/spawn.rs`); host secrets use the Keychain. |

`organization.provision.v1` today carries a `home_team_name`, creates no person record
for the creator, and is reached only from tests (implementation plan A4).

The office directory today is `<home>/offices/<office-id>/` under the host home
resolved by `org_proto::location`, holding `org.db`, `office.db` and `workspace/`
besides `office.lock` and `office.json`. Those databases are the source of truth until
the store writes the [storage tree](organization-data/README.md#shared-storage-contract)
(implementation plan B1b). The rendezvous's first line still has an optional
`id_profile: "selected_v1"`; without it the code accepts the older alphanumeric IDs, and
`office.json` version 2 holds UUIDv7 IDs. Every ID becomes UUIDv7 and the profile
switch goes (FC-142). The `joined` event, the `child` and `child_gone` events, the
temp handle and the layout-change notice are not yet part of the rendezvous.
