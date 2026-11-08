# org-office-host ↔ org-office-server: the boundary (phase 1, HQ only)

Status: **approved by ada** 2026-11-08, and **agreed by River** (the host) **and server-dev** (the server), `--discard` included. Updated for ADR-0003 §2.1 (the app token on every start) and the code review of `org-office-launch` (rev 2, 2026-11-08). Amended by ada 2026-11-08 for the boundary review (adopt vs discard, `not_ready` at startup, the give-up count, the status-reason mapping, the stdin and `--discard` wording, `org-supervise`). It implements
`docs/design/offices-hq-and-satellites.md` ("Processes on one Mac") and the `org-proto` host calls.

**Implementation status** (2026-11-08): `org-office-host` implements this design: the catalog, the supervisor, `hello` (with its capabilities), `organization.create` (with `not_ready` at startup and at most 4 creates in flight), `organization.list`, `office.list`, `subscribe`, `office.app_token` behind the peer code-signature check, `office.restart`, `office.status_changed` with `status_reason` / `next_restart_at`, and the give-up rule (River: `river/office-host`, merged at `7c34096`, and `river/host-contract`). Spawning through `org-supervise` comes later; until then the host clears the environment and serializes spawns, binds and accepts under one lock.
The contract crate `crates/org-office-launch` holds the exact shapes. **This design is the reference:**
where this design and `crates/org-office-launch` differ, this design wins until River's next commit
updates the crate.

## 1. Who owns what on disk

`<home>` = the office home, from `org_proto::location` (`$ORG_OFFICE_HOME` or `~/Library/Application Support/agentic.org`).

| Path | Written by | Read by |
| --- | --- | --- |
| `<home>/host.lock` | host: **the single-host lock**, an exclusive `flock` held for the host's whole life. A second host on the same home exits at once, so one host creates every office. Inside it, the **catalog mutex** (in-process) guards only a create's name check and reservation (see 4). | host |
| `<home>/host.sock` | host (0600) | app, `org-agent` (run by hand; office-run sessions compute their socket path, §3) |
| `<home>/run/<handle>.sock` | server (0600) | app, `org-agent` (address from the host's `office.list`, or computed when `ORG_OFFICE` is set, as in office-run sessions, §3) |
| `<home>/offices/<office-id>/` | **server** creates it (0700) on `--create` | host lists the folder names |
| `…/office.lock` | server: `flock` from before it opens anything until it exits | nobody else, ever |
| `…/office.json` | server, **written last** on `--create` (tmp + rename + fsync): this is the "stored" marker | host (the catalog) |
| `…/org.db`, `office.db`, `workspace/` | server | server |

**The catalog** = the folders under `offices/` named by a plain id that hold a valid `office.json`.
The host looks only at folders named by a plain id; any other folder (such as `.refused`) is never
started or discarded.
- **No `office.json`:** an unfinished create. The host never starts it. At startup, or after that create
  failed before `office.json` was in place, it runs `org-office-server --discard <id>`. The server takes
  the lock (same wait), and removes the folder and its Keychain key only if the office never finished
  (with no folder, it takes no lock and only removes a leftover key, see 3):
  - it refuses (`already_exists`) a folder with `office.json`, or with databases that hold a committed
    organization;
  - it deletes only when no database file exists, or when it opens them and they hold no committed
    organization;
  - a database it can't open or check (the key is missing, the Keychain refuses access, SQLCipher
    fails) means it refuses (`storage_error`).

  On a refusal the host logs it and leaves the folder for a person. So a real office that lost its
  `office.json` is never destroyed automatically.
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
role are kept when a server rewrites it (e.g. on an organization rename, later).

## 2. Starting a server

```
org-office-server --create          # create a new office from the pipe message, then serve it
org-office-server --office <id>     # serve an existing office
org-office-server --discard <id>    # remove an unfinished office and its key, then exit
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
- Framing: JSON, one object per line, at most 1 MiB per line (`org_proto::frame`). Every message has `"v":1`.

## 3. The pipe protocol (v1)

**Host → server** (stdin; the first line, on **every** start, matching the flag):
```json
{"v":1,"op":"create","org_id":"01…","org_name":"Acme","office_id":"01…","office_name":"HQ","role":"hq","app_token":"<64 hex>"}
{"v":1,"op":"start","app_token":"<64 hex>"}
```
- `create` with `--create`, `start` with `--office`; `--discard` reads no line. A first line that's
  malformed, an unknown op or for the other flag → `failed: invalid_request`. EOF before the first
  line → exit 0 without a `failed` line (the host is gone).
- After the first line, a v1 server ignores every further stdin line, known ops included, so a
  second `start` can't swap the token mid-life. stdin stays open only so its EOF can stop the server.
- **`app_token`** (ADR-0003 §2.1): the office's ephemeral app token, made by the host for each office
  each time the host starts, kept only in memory, never in argv, env, a file, a log or the Keychain.
  The server keeps it only in memory. The database key never crosses the pipe (ADR-0002).
- v1 creates only HQ offices; any other role is refused.
The host has already checked the name, its uniqueness (ignoring case) among this Mac's organizations,
and that the office id's 12-character handle is unique (ignoring case). The server checks the name again
with the same rule (one implementation, see 6). Lines after the first are ignored; an unknown op on the
first line is `invalid_request`.

**Server → host** (stdout):
```json
{"v":1,"event":"created","office":{…office.json…}}      // --create only: key, databases and
                                                        // office.json are durable
{"v":1,"event":"ready","socket":"/…/run/3RXF2NS0SH83.sock","pid":4242}  // listening, hello works
{"v":1,"event":"discarded"}                                            // --discard only, then exit 0;
                                                                       // also with no folder (idempotent): see below
{"v":1,"event":"failed","code":"key_missing","message":"…"}             // then exits non-zero
```
- `created` comes before `ready`. `organization.create` answers the client as soon as `created`
  arrives (the contract's "succeeds once stored"), with the HQ status `starting`, or `active` if
  `ready` came too.
- `ready` means the socket is bound and accepting: `hello` answers.
- `--discard` with no folder takes no lock and creates nothing: it only checks for the office's
  Keychain key, removes it if present, and reports `discarded`.
- `failed` codes, and what the host does with each:

| code | meaning | host |
| --- | --- | --- |
| `invalid_request` | a bad first line or bad args | fatal: `unavailable`; starts again only with a host restart and a fixed binary |
| `unsupported_version` | `v` unknown | fatal: `unavailable`; starts again only with a host restart and a fixed binary |
| `invalid_name` | name breaks the rule | create fails: `invalid_name` |
| `already_exists` | `--create` but the folder or office.json exists; `--discard` of an office with office.json or a committed organization | create fails: `already_exists`; discard: log, leave it |
| `key_missing` | databases exist, key doesn't (ADR-0002); `--discard` says `storage_error` instead | fatal: `unavailable` (`key_missing`); no automatic restart, but `office.restart` retries it |
| `in_use` | lock not acquired within 10 s | restart with backoff |
| `storage_error` | anything else (I/O, Keychain, SQLCipher); `--discard` of a folder whose databases it can't open or check | restart with backoff (create: fails with `storage_error`; discard: log, leave it) |

- A message is for people: never the key, paths or SQL. Unknown codes count as `storage_error`.
- **On a failed `--create`, the server deletes the Keychain key it made before it exits, but only before
  `office.json` is in place** (the host can't: the item's ACL is tied to the server). Once `office.json`
  is renamed into place, the server never deletes its key, even if it then fails: the office is stored,
  and the host adopts it (see 4). A crash before that point can still leave an orphan item; the host's
  `--discard` then removes it with the folder (rule in 1).
- EOF on stdout with no `failed` line, or exit before `ready`, means a crash: restart with backoff.
- (amended 2026-11-08: agent sessions rev 3.3, ada) **Children, for the watchdog.** After `ready`, the server also sends a line for each agent runtime it spawns, and one when that child is reaped and cleaned up ([`agent-sessions.md`](agent-sessions.md), "Processes and safety"):
  `{"v":1,"event":"child","pgid":4250,"pid":4250,"started_at":"…","mark":"…"}` and `{"v":1,"event":"child_gone","pgid":4250}`.
  The host keeps them in memory only, and when the server dies it runs the orphan cleanup on them before it restarts the server. The mark isn't a secret.
  Office-run agents' tools can't connect to `host.sock` (a fixed deny in `agent-sessions.md`). `org-agent` there always has `ORG_OFFICE`, so it computes its office's socket path (`org_proto::location::office_socket_path`) and never needs `host.sock` or `office.list`.
- (amended 2026-11-08: agent sessions rev 3.3, ada) **A host ignores an event it doesn't know** (it logs the event's name once), so a server can add events without a host change. Only `failed` codes have their own unknown rule (above).

## 4. What the host does
- **Status per office** (`OfficeInfo.status`): `starting` (spawned, no `ready` yet; at host startup,
  also every catalog office not spawned yet, see Startup: this widens `starting`) → `active` (`ready`,
  child alive) → `offline` (exited; a restart is pending, with `next_restart_at`, unless the reason is
  `stopped_by_owner`) → `unavailable` (fatal code, or restarts given up). The `ready` wait is 30 s
  (Keychain and the first SQLCipher open can be slow), then kill + backoff.
- **Status reasons** (`OfficeInfo.status_reason`, set with `offline` and `unavailable`), one for every
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

  `invalid_name` and `already_exists` end a create only; that office never enters the catalog.
- **Restart:** backoff 1, 2, 4 … 60 s, reset after 60 s of uptime. Fatal codes never restart on
  their own. A `key_missing` office starts again only with `office.restart` or a host restart.
  `unsupported_version` and `invalid_request` start again only with a host restart and a fixed binary
  (`office.restart` refuses them).
- **Shutdown** (SIGTERM from launchd at logout, or the host exiting): close every child's stdin,
  wait up to 10 s, then SIGKILL what's left.
- **Create:** under the catalog mutex, and only for this step: check the name and its uniqueness
  (counting names reserved by creates in flight), make the org and office ids (re-roll on a handle
  clash), reserve the name and handle, then release the mutex. Outside it: spawn `--create` and wait for
  `created` (or `failed`) for at most 30 s. On failure, run `--discard` (rule in 1), return the error
  and emit nothing, unless `office.json` is present (adopt, below). On success, emit `organization.created` on `subscribe`. The reservation ends when
  the create fails, or once the new office is in the catalog.
- **Capabilities:** the host's `hello` lists in `capabilities` the optional calls and events it implements (`office.app_token`, `office.restart`, `office.status_changed`; see `org_proto::capability`), so the app checks before relying on one.
- **App-only calls and the exec swap** (ada, 2026-11-08): an app-only call (`office.app_token`, `office.restart`) is accepted only on a connection whose client sent it after reading `hello`'s answer. Bytes already buffered when the host accepted the connection are refused with `not_authorized`, so a client never pipelines an app-only call before `hello`'s answer. The reason is the exec swap: a process can connect, keep the socket in a child, send the call, then `exec` the signed app, and `LOCAL_PEERTOKEN` is the *current* token of the process that last touched the socket. So at accept the host samples the audit token, then the bytes already buffered (`FIONREAD`, N0); on each app-only call it requires the request to start at or after N0 and the current token to equal the accept-time one byte for byte, and runs the code-signature check (`SecCode`) on that token.
- **App-only calls and the inherited-socket swap** (ada, 2026-11-08: host review H1): first thing in the app's main, before any output or threads, the app fstats fds 0-2 and dup2s `/dev/null` over any that is a socket; it never reads or writes an fd it didn't open. After sending an app-only call, the client sends nothing else on that connection until its answer arrives. The reason: a process that inherited a copy of the app's connection (or handed the app one on its stdio) can touch it, and any touch, even a read or a zero-length write, moves `LOCAL_PEERTOKEN` to the toucher. So on each app-only call the host also refuses (`not_authorized`) unless nothing has arrived after the request line: nothing buffered past it in its reader and nothing waiting on the socket (`FIONREAD` 0), checked on the connection's reader before it reads anything more.
- **Host decisions** (ada, 2026-11-08): `office.app_token` gives the token of a `starting` or `active` office. The give-up rule counts 10 failed runs including the first start, and a run that stays up 60 s after `ready` clears the count. `office.status_changed` is level-triggered: each event carries the office's state at that moment, intermediate states may be skipped and duplicates are possible, so clients replace their record idempotently. `office.restart` on an office the host couldn't supervise answers `unavailable`. A non-fatal `failed` followed by the ready timeout reports that code's reason, not `start_timeout`. `invalid_name` and `already_exists` from an `--office` run are never restarted on their own: the office stops `unavailable` with `invalid_request`. `key_missing` follows org-proto: `unavailable`, no automatic restart (so it doesn't count toward the give-up rule), and `office.restart` retries it.
- **host.sock:** 0600 and `run/` 0700. On accept, the host also checks the peer's uid (`getpeereid`) is its own (defence in depth).
- **Fatal status and restart:** `unavailable` (with `status_reason`) lasts until the owner calls `office.restart` from the app, or the host restarts (for `unsupported_version` and `invalid_request`, a host restart with a fixed binary only). `office.restart` starts an `offline` or `unavailable` office now; for any other status it returns `conflict`. It's app-only: the host checks the caller's code signature, as for `office.app_token` (`not_authorized` otherwise). It clears `unavailable`, resets the backoff and the give-up count (the failure count goes to 0), and retries `key_missing` (it goes straight back if the key is still missing); it refuses `unsupported_version` and `invalid_request` (`unavailable`). Every status change is pushed as `office.status_changed` on `subscribe`.
- **Startup:** the host binds `host.sock` before it spawns anything. It answers `hello`, `office.list` and `subscribe` at once. `office.list` shows every catalog office as `starting` from the start, before the host has spawned it (this widens `starting`, see Status per office). Calls that need a settled catalog (`organization.create`, `office.restart`, and `office.app_token` for an office still being discarded) wait for the startup discards, at most 30 s, then return `not_ready` ("host starting"; temporary, retry shortly).
- **A create that stored its office but didn't report `created`** (it died, or sent a `failed` line after `office.json` was in place): if `office.json` matches the plan (id, org_id, org_name, office_name, role), the host adopts it: the create succeeds (status `starting`), it starts `--office`, and it emits `organization.created`. It never runs `--discard` once `office.json` is present. A mismatch is a refused create.
- **Spawning:** `org-supervise` is the single spawn path: every process the host or a server starts goes through it (`posix_spawn` with `POSIX_SPAWN_CLOEXEC_DEFAULT` and explicit file actions for fds 0/1/2), so a child inherits only the fds it's given. That needs libc FFI; the `unsafe` is confined to `org-supervise`. It complements the server's fd swap (see 2), which is the structural guarantee and doesn't depend on it.
- **Give-up rule:** 10 failed restarts within 30 minutes → `unavailable` with `restarts_exhausted` (no more automatic restarts until `office.restart`, which resets the count). A **failed restart** is an exit before 60 s of uptime: a crash, EOF without `ready`, a `failed` line, or the ready timeout. A crash after 60 s of uptime counts as a fresh start, not a failure. While `offline` with a restart pending, `next_restart_at` says when.
- **Socket length:** every office socket path in a home has the same length (12-character handles),
  so the host checks `host.sock` and a sample `run/XXXXXXXXXXXX.sock` against the 103-byte limit **at
  startup** and refuses to start with a clear message. No per-create error code is needed.

## 5. Library layout
- `apps/org-office-host/`: `src/main.rs` (thin: args, location, signals, `Host::run`) and
  `src/lib.rs` with modules `catalog` (scan, office.json, name uniqueness, create), `supervisor`
  (spawn, the pipe protocol, status, backoff, shutdown), and `service` (org-proto handlers: `hello`,
  `organization.create`, `organization.list`, `office.list`, `office.restart`, `office.app_token`,
  `subscribe`).
- The supervisor takes the server program's path as config (default: the `org-office-server` next to
  the host binary in the app bundle). The host's tests use a **fake office server** (a test helper binary
  that follows section 3), so the host is tested without SQLCipher or the Keychain, and doesn't wait for
  server-dev's work to land. The fake must never ship: it's an example or a dev-only bin the bundle never picks up.
- The host doesn't depend on `org-office` (no SQLCipher in the login service) or on
  `org-office-server`'s library (an app doesn't depend on an app).

## 6. Decisions (ada, 2026-11-08)
1. **The pipe protocol and `office.json` types live in a new contract crate, `crates/org-office-launch`**, not `org-proto` (the client contract stays small). It depends only on `org-proto`, `serde` and `serde_json`. ada owns it as a contract. River writes the first version, and ada reviews every commit that touches it. It's additive and versioned (`"v":1`; lines after the first are ignored, and an unknown op on the first line is `invalid_request`; unknown events and fields are ignored; unknown failed codes count as `storage_error`).
2. **The name rule is `org_proto::OrganizationName`** (ada, branch `ada/proto-org-name`): `parse(&str) -> Result<_, NameError>`, `.as_str()` (trimmed), `.key()` (Unicode lowercase of the trimmed name, no normalization). `NameError` is `Empty | TooLong | ControlChar` with `.message()`, mapping to `invalid_name`.
3. **The minimal listener** lives in one module (`service/listener.rs`), uses only `org_proto::frame` and the typed `Call`/`EventKind`, and grows no router. It's swapped for server-dev's shared crate once that's extracted.
4. The design's "`--create --role hq`" (line 35) is wrong. Only `--create` goes on the command line. ada fixes the wording.
5. **`--discard`** (server-dev's proposal, agreed by River and ada 2026-11-08) replaces the host removing unfinished folders itself, so orphan Keychain keys are removed too. It never destroys an office that finished (office.json or a committed organization), or one it can't check.
6. **The app token** (ADR-0003 §2.1, ada 2026-11-08) is ephemeral and goes in the first line of every start.
7. **Refused creates are quarantined**, not deleted: the requested id's folder moves to `offices/.refused/` (from the review of `org-office-launch` @ 26ed861).
