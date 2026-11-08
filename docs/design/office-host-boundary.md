# org-office-host ↔ org-office-server: the boundary (phase 1, HQ only)

Status: **agreed by River and server-dev; approved by ada (2026-11-08)**. It implements
`offices-hq-and-satellites.md` ("Processes on one Mac") and the `org-proto` host calls. The exact shapes are in `crates/org-office-launch` (River is writing it; it isn't on `v1` yet).

## 1. Who owns what on disk

`<home>` = the office home, from `org_proto::location` (`$ORG_OFFICE_HOME` or `~/Library/Application Support/agentic.org`).

| Path | Written by | Read by |
| --- | --- | --- |
| `<home>/host.lock` | host: **the single-host lock**, an exclusive `flock` held for the host's whole life. A second host on the same home exits at once. Only the host creates offices. Inside it, the **catalog mutex** (in-process) serializes only the name check and reservation (see 4). | host |
| `<home>/host.sock` | host (0600) | app, `org-agent` |
| `<home>/run/<handle>.sock` | server (0600) | app, `org-agent` (address from the host's `office.list`) |
| `<home>/offices/<office-id>/` | **server** creates it (0700) on `--create`, and removes an unfinished one on `--discard` | host lists the folder names |
| `…/office.lock` | server: `flock` from before it opens anything until it exits | nobody else, ever |
| `…/office.json` | server, **written last** on `--create` (tmp + rename + fsync): this is the "stored" marker. The host only renames it, on a refused create (see 4) | host (the catalog) |
| `…/org.db`, `office.db`, `workspace/` | server | server |

**The catalog** = the folders under `offices/` that hold a valid `office.json`. A folder without one is
an unfinished create: the host ignores it. At startup, and after that create failed, the host runs
`org-office-server --discard <office-id>`. That server takes the office lock first (so it never removes a
live server's folder), refuses a folder that holds `office.json` (`already_exists`), deletes the office's
orphan Keychain key if there is one, removes the folder, reports `discarded` and exits 0. The host
doesn't delete folders itself.

**Lock-held-means-running:** an office's `office.lock` is held if and only if a server is running
that office. The server **waits** for the lock at start (blocking, up to 10 s, then `failed: in_use`),
so after a crash or a host restart, a slow-exiting old server and its replacement never overlap. The
host never takes an office's lock, and never decides an office is
stopped from a lock probe. Status comes from supervision (see 4).

### `office.json` (schema v1, the server writes it, the host reads it)
```json
{"v":1,"office_id":"01…","office_name":"HQ","role":"hq",
 "org_id":"01…","org_name":"Acme","created_at":"2026-11-08T02:10:00.000Z"}
```
No secrets. Readers ignore unknown fields (phase 2 adds `hq_address`). The server rewrites it when it
applies an organization rename (later).

## 2. Starting a server

```
org-office-server --create          # create a new office from the pipe message, then serve it
org-office-server --office <id>     # serve an existing office
org-office-server --discard <id>    # remove an unfinished office (no office.json), then exit
```
- The host always sets `ORG_OFFICE_HOME` explicitly on the child, so both use the same home.
- No details on the command line beyond the office id (the design: details travel through the pipe).
- **stdin is the control pipe from the host.** Its first line matches the flag: `create` with `--create`,
  `start` with `--office`. `--discard` reads no line. EOF on stdin = the host is gone, so the server shuts
  down (also before the first line, then without a `failed` line): it stops accepting, finishes
  in-flight writes, removes its socket, releases the lock and
  exits 0. (The design's "exits when the pipe to the host closes".) The child also stays in the host's
  process group.
- **The app token travels only in that first line, on every start.** The host makes a new one for each
  office each time the host starts and keeps it only in memory (ADR-0003 §2.1). The server keeps it only
  in memory and never logs it. **The database key never crosses the pipe:** each server creates and keeps
  its own (ADR-0002).
- **stdout carries the server's control lines to the host, and nothing else.** Logs go to stderr,
  which the host sends to `<home>/logs/<handle>.log` (`logs/` 0700, files 0600; rotate at 10 MiB, keep one old file, so a crash loop can't fill the disk).
- Framing: JSON, one object per line, at most 1 MiB per line (`org_proto::frame`). Every message has `"v":1`.

## 3. The pipe protocol (v1)

**Host → server** (stdin; the first line, on every start except `--discard`):
```json
{"v":1,"op":"create","org_id":"01…","org_name":"Acme","office_id":"01…","office_name":"HQ","role":"hq","app_token":"…"}
{"v":1,"op":"start","app_token":"…"}
```
The host has already checked the name, its uniqueness (ignoring case) among this Mac's organizations,
and that the office id's 12-character handle is unique (ignoring case). The server checks the name again
with the same rule (one implementation, see 6). Later lines are reserved: the server ignores unknown `op`s.

**Server → host** (stdout):
```json
{"v":1,"event":"created","office":{…office.json…}}      // --create only: key, databases and
                                                        // office.json are durable
{"v":1,"event":"ready","socket":"/…/run/3RXF2NS0SH83.sock","pid":4242}  // listening, hello works
{"v":1,"event":"discarded"}                                             // --discard only: key and folder
                                                                         // are gone; then exits 0
{"v":1,"event":"failed","code":"key_missing","message":"…"}             // then exits non-zero
```
- `created` comes before `ready`. `organization.create` answers the client as soon as `created`
  arrives (the contract's "succeeds once stored"), with the HQ status `starting`, or `active` if
  `ready` came too.
- `ready` means the socket is bound and accepting: `hello` answers.
- `failed` codes, and what the host does with each:

| code | meaning | host |
| --- | --- | --- |
| `invalid_request` | bad or missing args, or a first line that's malformed or doesn't match the flag | fatal: `unavailable` |
| `unsupported_version` | `v` unknown | fatal: `unavailable` |
| `invalid_name` | name breaks the rule | create fails: `invalid_name` |
| `already_exists` | `--create` but the folder or office.json exists, or `--discard` of an office that has office.json | `--create`: create fails: `already_exists`. `--discard`: the office is complete, so the host keeps it and never treats it as unfinished |
| `key_missing` | databases exist, key doesn't (ADR-0002) | fatal: `unavailable`, no restart |
| `in_use` | lock not acquired within 10 s | restart with backoff |
| `storage_error` | anything else (I/O, Keychain, SQLCipher) | restart with backoff (create: fails with `storage_error`) |

- A message is for people: never the key, paths or SQL. Unknown codes count as `storage_error`.
- **On a failed `--create`, the server deletes the Keychain key it made before it exits** (the host can't: the item's ACL is tied to the server). A crash part-way through can still leave an orphan item; the host's `--discard` removes it with the folder.
- For `--create` and `--office`, EOF on stdout with no `failed` line, or exit before `ready`, means a
  crash: restart with backoff. A `--create` that crashes before `office.json` is durable is never
  restarted: the host discards it. A `--discard` that exits without `discarded` leaves the folder
  unfinished, and the host tries again at its next start.

## 4. What the host does (River's side, for reference)
- **Status per office** (`OfficeInfo.status`): `starting` (spawned, no `ready` yet) → `active`
  (`ready`, child alive) → `offline` (exited, restart pending) → `unavailable` (fatal code, or
  restarts given up). The `ready` wait is 30 s (Keychain and the first SQLCipher open can be slow), then kill + backoff.
- **Restart:** backoff 1, 2, 4 … 60 s, reset after 60 s of uptime. Fatal codes never restart, until
  the host restarts.
- **Shutdown** (SIGTERM from launchd at logout, or the host exiting): close every child's stdin,
  wait up to 10 s, then SIGKILL what's left.
- **Create:** under the catalog mutex, check the name against this Mac's organizations and the names
  reserved by creates still in flight, reserve it, and release the mutex. Then make the org and office ids
  (re-roll on a handle clash), spawn `--create`, wait for `created` (or `failed`) for at most 30 s.
  On `failed`, run `--discard` for the office (rule in 1), return the error and emit nothing. If the
  server exits or times out without `created`, the host stops it, waits for it to exit, and checks the
  office's folder. An `office.json` that matches the request means the create succeeded (status
  `starting`, later starts use `--office`). One that doesn't match is a refused create (below). With no
  `office.json`, the host runs `--discard`. On success, emit `organization.created` on `subscribe`.
  The reservation ends when the create succeeds or fails.
- **A refused create:** if the office reported by `created`, or found in the folder after a crash or
  timeout, doesn't match the create request (another id, organization, name or role), the host doesn't
  adopt it. It stops the server, waits for it to exit, renames
  `offices/<requested id>/office.json` (if present) to `office.json.refused` (so the folder counts as
  unfinished again), runs `--discard <requested id>`, and fails the create. **The host acts only on the
  folder of the office id it requested, never on an id the server reported.** It never touches any other
  office, whatever id the server reported.
- **Open (River and server-dev):**
  - If the rename to `office.json.refused` fails, `--discard` refuses the folder (`already_exists`), and
    the next host start would adopt it. The host needs a durable record of the refusal.
  - A `--create` that fails after `office.json` is in place (e.g. the folder fsync) reports `failed` and
    deletes its key, so the next host start would adopt an office with no key (`key_missing`). `failed`
    and the key deletion should be limited to errors before `office.json` is in place.
- **host.sock:** 0600 and `run/` 0700. It also serves `office.app_token` (only after the peer
  code-signature check, ADR-0003 §2.1). On accept, the host also checks the peer's uid (`getpeereid`) is its own (defence in depth).
- **Fatal status:** `unavailable` lasts until the host restarts (phase 1). A manual "retry" call can come later.
- **Socket length:** every office socket path in a home has the same length (12-character handles),
  so the host checks `host.sock` and a sample `run/XXXXXXXXXXXX.sock` against the 103-byte limit **at
  startup** and refuses to start with a clear message. No per-create error code is needed.

## 5. Library layout
- `apps/org-office-host/`: `src/main.rs` (thin: args, location, signals, `Host::run`) and
  `src/lib.rs` with modules `catalog` (scan, office.json, name uniqueness, create), `supervisor`
  (spawn, the pipe protocol, status, backoff, shutdown), and `service` (org-proto handlers: `hello`,
  `organization.create`, `organization.list`, `office.list`, `office.app_token`, `subscribe`).
- The supervisor takes the server program's path as config (default: the `org-office-server` next to
  the host binary in the app bundle). The host's tests use a **fake office server** (a test helper binary
  that follows section 3), so the host is tested without SQLCipher or the Keychain, and doesn't wait for
  server-dev's work to land. The fake must never ship: it's an example or a dev-only bin the bundle never picks up.
- The host doesn't depend on `org-office` (no SQLCipher in the login service) or on
  `org-office-server`'s library (an app doesn't depend on an app).

## 6. Decisions (ada, 2026-11-08)
1. **The pipe protocol and `office.json` types live in a new contract crate, `crates/org-office-launch`**, not `org-proto` (the client contract stays small). It depends only on `org-proto`, serde and serde_json. ada owns it as a contract. River writes the first version, and ada reviews every commit that touches it. It's additive and versioned (`"v":1`; unknown ops, events and fields are ignored; unknown failed codes count as `storage_error`).
2. **The name rule is `org_proto::OrganizationName`** (ada, on `v1`): `parse(&str) -> Result<_, NameError>`, `.as_str()` (trimmed), `.key()` (Unicode lowercase of the trimmed name, no normalization). `NameError` is `Empty | TooLong | ControlChar | InvisibleChar` with `.message()`, mapping to `invalid_name`.
3. **The minimal listener** lives in one module (`service/listener.rs`), uses only `org_proto::frame` and the typed `Call`/`EventKind`, and grows no router. It's swapped for server-dev's shared crate once that's extracted.
4. The design's "`--create --role hq`" was wrong. Only `--create` goes on the command line. The offices design now says so.
