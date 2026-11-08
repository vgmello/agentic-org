# org-office-host ↔ org-office-server: the boundary (phase 1, HQ only)

Status: **approved by ada** 2026-11-08 and **agreed by server-dev** (with `--discard`), updated for ADR-0003 §2.1 (the app token on every start) and the code review of `org-office-launch` (rev 2, 2026-11-08). The contract crate `crates/org-office-launch` is the reference for exact shapes. It implements
`docs/design/offices-hq-and-satellites.md` ("Processes on one Mac") and the `org-proto` host calls.

## 1. Who owns what on disk

`<home>` = the office home, from `org_proto::location` (`$ORG_OFFICE_HOME` or `~/Library/Application Support/agentic.org`).

| Path | Written by | Read by |
| --- | --- | --- |
| `<home>/host.lock` | host: **the catalog lock**, an exclusive `flock` held for the host's whole life. A second host on the same home exits at once. Only the host creates offices, so creates are serialized under it (plus an in-process mutex). | host |
| `<home>/host.sock` | host (0600) | app, `org-agent` |
| `<home>/run/<handle>.sock` | server (0600) | app, `org-agent` (address from the host's `office.list`) |
| `<home>/offices/<office-id>/` | **server** creates it (0700) on `--create` | host lists the folder names |
| `…/office.lock` | server: `flock` from before it opens anything until it exits | nobody else, ever |
| `…/office.json` | server, **written last** on `--create` (tmp + rename + fsync): this is the "stored" marker | host (the catalog) |
| `…/org.db`, `office.db`, `workspace/` | server | server |

**The catalog** = the folders under `offices/` named by a plain id that hold a valid `office.json`. A
folder without one is an unfinished create: the host ignores it and, at startup or after that create
failed, runs `org-office-server --discard <id>`. The server takes the lock (same wait), and removes the
folder and its Keychain key only if the office never finished: it refuses (`already_exists`) a folder
with `office.json` **or with databases that hold a committed organization**, so a real office that lost
its `office.json` is never destroyed automatically; the host logs it and leaves it for a person.

**A refused create:** if a server reports a `created` office other than the one asked for (id,
organization, name or role), the host stops it and moves the folder of the id **it asked for** to
`offices/.refused/<id>` (out of the catalog; nothing deleted, the key stays). It never touches a folder
named by a different id the server reported; it only logs it.

**Lock-held-means-running:** an office's `office.lock` is held if and only if a server is running
that office. The server **waits** for the lock at start (blocking, up to 10 s, then `failed: in_use`),
so after a crash or a host restart, a slow-exiting old server and its replacement never overlap. The
host never takes an office's lock and never decides an office is stopped from a lock probe. Status comes from supervision (see 4).

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
- **`app_token`** (ADR-0003 §2.1): the office's ephemeral app token, made by the host for each office
  each time the host starts, kept only in memory, never in argv, env, a file, a log or the Keychain.
  The server keeps it only in memory. The database key never crosses the pipe (ADR-0002).
- v1 creates only HQ offices; any other role is refused.
The host has already checked the name, its uniqueness (ignoring case) among this Mac's organizations,
and that the office id's 12-character handle is unique (ignoring case). The server checks the name again
with the same rule (one implementation, see 6). Later lines are reserved: the server ignores unknown `op`s.

**Server → host** (stdout):
```json
{"v":1,"event":"created","office":{…office.json…}}      // --create only: key, databases and
                                                        // office.json are durable
{"v":1,"event":"ready","socket":"/…/run/3RXF2NS0SH83.sock","pid":4242}  // listening, hello works
{"v":1,"event":"discarded"}                                            // --discard only, then exit 0
{"v":1,"event":"failed","code":"key_missing","message":"…"}             // then exits non-zero
```
- `created` comes before `ready`. `organization.create` answers the client as soon as `created`
  arrives (the contract's "succeeds once stored"), with the HQ status `starting`, or `active` if
  `ready` came too.
- `ready` means the socket is bound and accepting: `hello` answers.
- `failed` codes, and what the host does with each:

| code | meaning | host |
| --- | --- | --- |
| `invalid_request` | a bad first line or bad args | fatal: `unavailable` |
| `unsupported_version` | `v` unknown | fatal: `unavailable` |
| `invalid_name` | name breaks the rule | create fails: `invalid_name` |
| `already_exists` | `--create` but the folder or office.json exists; `--discard` of an office with office.json or a committed organization | create fails: `already_exists`; discard: log, leave it |
| `key_missing` | databases exist, key doesn't (ADR-0002) | fatal: `unavailable`, no restart |
| `in_use` | lock not acquired within 10 s | restart with backoff |
| `storage_error` | anything else (I/O, Keychain, SQLCipher) | restart with backoff (create: fails with `storage_error`) |

- A message is for people: never the key, paths or SQL. Unknown codes count as `storage_error`.
- **On a failed `--create`, the server deletes the Keychain key it made before it exits** (the host can't: the item's ACL is tied to the server). A crash part-way through can still leave an orphan item; that's accepted for now.
- EOF on stdout with no `failed` line, or exit before `ready`, means a crash: restart with backoff.

## 4. What the host does (my side, for reference)
- **Status per office** (`OfficeInfo.status`): `starting` (spawned, no `ready` yet) → `active`
  (`ready`, child alive) → `offline` (exited, restart pending) → `unavailable` (fatal code, or
  restarts given up). The `ready` wait is 30 s (Keychain and the first SQLCipher open can be slow), then kill + backoff.
- **Restart:** backoff 1, 2, 4 … 60 s, reset after 60 s of uptime. Fatal codes never restart, until
  the host restarts.
- **Shutdown** (SIGTERM from launchd at logout, or the host exiting): close every child's stdin,
  wait up to 10 s, then SIGKILL what's left.
- **Create:** under the catalog mutex: check the name and its uniqueness, make the org and office ids
  (re-roll on a handle clash), spawn `--create`, wait for `created` (or `failed`) for at most 30 s.
  On failure, remove the unfinished folder (rule in 1), return the error and emit nothing. On success,
  emit `organization.created` on `subscribe`.
- **host.sock:** 0600 and `run/` 0700. On accept, the host also checks the peer's uid (`getpeereid`) is its own (defence in depth).
- **Fatal status:** `unavailable` lasts until the host restarts (phase 1). A manual "retry" call can come later.
- **Socket length:** every office socket path in a home has the same length (12-character handles),
  so the host checks `host.sock` and a sample `run/XXXXXXXXXXXX.sock` against the 103-byte limit **at
  startup** and refuses to start with a clear message. No per-create error code is needed.

## 5. Library layout
- `apps/org-office-host/`: `src/main.rs` (thin: args, location, signals, `Host::run`) and
  `src/lib.rs` with modules `catalog` (scan, office.json, name uniqueness, create), `supervisor`
  (spawn, the pipe protocol, status, backoff, shutdown), and `service` (org-proto handlers: `hello`,
  `organization.create`, `organization.list`, `office.list`, `subscribe`).
- The supervisor takes the server program's path as config (default: the `org-office-server` next to
  the host binary in the app bundle). The host's tests use a **fake office server** (a test helper binary
  that follows section 3), so the host is tested without SQLCipher or the Keychain, and doesn't wait for
  server-dev's work to land. The fake must never ship: it's an example or a dev-only bin the bundle never picks up.
- The host doesn't depend on `org-office` (no SQLCipher in the login service) or on
  `org-office-server`'s library (an app doesn't depend on an app).

## 6. Decisions (ada, 2026-11-08)
1. **The pipe protocol and `office.json` types live in a new contract crate, `crates/org-office-launch`**, not `org-proto` (the client contract stays small). It depends only on `org-proto` and serde. ada owns it as a contract. River writes the first version, and ada reviews every commit that touches it. It's additive and versioned (`"v":1`; unknown ops, events and fields are ignored; unknown failed codes count as `storage_error`).
2. **The name rule is `org_proto::OrganizationName`** (ada, branch `ada/proto-org-name`): `parse(&str) -> Result<_, NameError>`, `.as_str()` (trimmed), `.key()` (Unicode lowercase of the trimmed name, no normalization). `NameError` is `Empty | TooLong | ControlChar` with `.message()`, mapping to `invalid_name`.
3. **The minimal listener** lives in one module (`service/listener.rs`), uses only `org_proto::frame` and the typed `Call`/`EventKind`, and grows no router. It's swapped for server-dev's shared crate once that's extracted.
4. The design's "`--create --role hq`" (line 35) is wrong. Only `--create` goes on the command line. ada fixes the wording.
5. **`--discard`** (server-dev, agreed 2026-11-08) replaces the host removing unfinished folders itself, so orphan Keychain keys are removed too. It never destroys an office that finished (office.json or a committed organization).
6. **The app token** (ADR-0003 §2.1, ada 2026-11-08) is ephemeral and goes in the first line of every start.
7. **Refused creates are quarantined**, not deleted: the requested id's folder moves to `offices/.refused/` (from the review of `org-office-launch` @ 26ed861).
