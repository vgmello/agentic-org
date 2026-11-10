# Runbook: a real HQ office on this Mac (debug build)

**Naming amendment (2026-11-10):** standalone v1 renames the server app/package/binary
from `apps/org-office-server` / `org-office-server` to `apps/org-server` / `org-server`,
and the retained browser UI to `apps/org-ui` / `org-ui`. The legacy debug commands
below describe the inspected source. Follow [project structure](../design/project-structure.md)
for implementation; this docs PR does not rename source or change process boundaries.

**V1 direction changed 2026-11-09:** the target is a standalone Rust server
on macOS/Linux serving the browser application from HQ. Tauri is deferred to v2.
See the [server design](../design/standalone-server.md) and [deletion inventory](../design/standalone-server-removal-inventory.md).
This documentation change specifies the pivot; the source described below is the
inspected pre-pivot implementation. Its Tauri/bundle commands are historical
development instructions, not installation instructions for the new v1. Do not
claim the new CLI, HTTP server or remote sync exists until implemented. No migration
of the undeployed desktop app or its data is required.

Status: draft (River, 2026-11-08), for ada's switchover from the mock to the real `org-office-host` and an HQ
`org-office-server`, from **debug builds**, and an end-to-end check. Nothing here is signed or bundled. Each claim
not checked against a run is marked "(unverified: …)".

## 0. Before you start: the tree must hold both halves of key custody

The host side (the tip of `river/host-keys`) and the server side (`server-dev/server-key-custody`, `da54892`) are
**not in `origin/v1` yet** (2026-11-08). They merge cleanly (`git merge-tree`), but that merge hasn't been built.
With the host side alone, `org-office-server` still has its own key store: a default build reads and writes the
**login Keychain** (service `org.agentic.office`) and ignores the key on the launch pipe, and a `--features dev-keys`
build refuses to start, because the host passes no `ORG_OFFICE_DEV_KEY_DIR` to its servers. Check first:

```sh
export REPO=/path/to/checkout   # holds both branches, e.g. v1 after both merges
cd "$REPO"
git merge-base --is-ancestor origin/river/host-keys HEAD && git merge-base --is-ancestor da54892 HEAD && echo "both halves: ok"
test ! -e apps/org-office-server/src/keys.rs && echo "server: keys only from the launch pipe"
```

Stop if either line prints nothing.

## 1. One folder for everything dev, apart from a real install

A future real install uses the default home `~/Library/Application Support/agentic.org`
(`org_proto::location::default_office_home`), the Keychain (service `org.agentic.office-key`) and the launchd label
`org.agentic.office-host`. The dev setup uses none of them:

```sh
export DEV="$HOME/agentic-org-dev"
export ORG_OFFICE_HOME="$DEV/home"          # the host, its servers, the app and org-client all find host.sock here
export ORG_OFFICE_DEV_KEY_DIR="$DEV/keys"   # debug host only: office DB keys as plain files
mkdir -p -m 700 "$DEV" "$DEV/bin" "$DEV/logs" "$DEV/keys"
```

- The office home rule: `ORG_OFFICE_HOME` if it is an absolute path, otherwise `$HOME/Library/Application Support/agentic.org`.
  A debug host also takes `--home <abs path>`, which wins over the variable, but the app and clients only read the
  variable, so **use the variable only** and export it in every terminal that runs the host, the app or a check.
- `ORG_OFFICE_DEV_KEY_DIR` (`apps/org-office-host/src/keys/mod.rs`, `dev_store`): required by a debug host (it
  refuses to start without it, so it never writes a Keychain item); absolute (a relative one is resolved against
  the host's working directory); **not inside the office home** (checked lexically and canonically). Keys are
  `<dir>/<office id>.key`, 64 hex digits, mode 0600, and the folder is made 0700. Don't share one key folder between
  two homes. Release builds ignore it.
- Socket paths must fit 103 bytes: `$ORG_OFFICE_HOME/run/<12 chars>.sock`. The host checks this at startup and
  refuses to start, so a deep home path (e.g. one under a long project folder) fails: keep `$DEV` short.
- The home must be a real folder owned by you, **not a symlink** (the host refuses to start on one, as on any home
  it can't make owner-only).
- Shared with a future real install: the app's data folder for `com.ellosoft.agentic-org` (remembered
  connections under `real/`, the login-item "off" marker) (unverified: Tauri's `app_data_dir`,
  `~/Library/Application Support/com.ellosoft.agentic-org`).

## 2. Build (debug)

The host takes the dev app's executable path **at compile time** (`option_env!("ORG_DEV_APP_PATH")`, main.rs). Cargo
records it as a build dependency, so changing it rebuilds the host. `tauri dev` runs the **unbundled** binary
`agentic-org` (package `agentic-org`, `src-tauri/src/main.rs`) from the workspace target folder, so that file is
what `proc_pidpath` reports (unverified: check with step 6's `ps` line; with `CARGO_TARGET_DIR` set it is under that).

```sh
cd "$REPO"
export ORG_DEV_APP_PATH="$REPO/target/debug/agentic-org"
cargo build -p org-office-host -p org-office-server        # no features: none is needed or wanted
cp target/debug/org-office-host target/debug/org-office-server "$DEV/bin/"
strings "$DEV/bin/org-office-host" | grep -F "$ORG_DEV_APP_PATH"   # the path is compiled in
```

- Binaries land in `$REPO/target/debug/`. Copy them out: any later `cargo build`/`cargo test` in `$REPO` without
  `ORG_DEV_APP_PATH` rewrites `target/debug/org-office-host` without the fallback.
- `org-office-host`'s other binary, `fake-office-server`, needs the `test-support` feature, which only tests turn on.
- `--app-team-id` (debug only) and `ORG_APP_TEAM_ID` (build time) are for a signed app. Leave both unset: the host
  logs `WARNING: no app team id`, which is expected here.

The desktop app (Tauri 2 + SvelteKit, bun). `bun run tauri dev` starts Vite on `http://localhost:1420`
(`beforeDevCommand`), builds the Rust shell in debug and runs it. Build the binary once now, so it exists when the
host starts:

```sh
cd "$REPO/apps/desktop" && bun install
cd "$REPO" && cargo build -p agentic-org     # or: run `bun run tauri dev` once and quit it
test -x "$ORG_DEV_APP_PATH" && echo "dev app: ok"
```

(unverified: a plain `cargo build -p agentic-org` writes the same file `tauri dev` runs.)

## 3. Start the host

The app does **not** start the host in-process. When `hello` to `host.sock` fails it asks `SMAppService` for the
bundled login item (`src-tauri/src/login_item.rs`, plist `src-tauri/launch-agents/org.agentic.office-host.plist`).
That only works from a bundle, and the bundled host is a release build that can't start yet (section 9). So for dev,
start the host yourself **before** the app. It then answers `hello` and the app never touches `SMAppService`.

**Recommended: A, in a terminal.** Logs are on screen and Ctrl-C stops everything.

```sh
"$DEV/bin/org-office-host" --server-bin "$DEV/bin/org-office-server" 2>&1 | tee -a "$DEV/logs/host.log"
```

Expect the lines `org-office-host: DEVELOPMENT FALLBACK (debug build only): office.app_token and office.restart also
accept <path> …` and `no orphan-key sweep for …`. If you see `the development fallback is off: …`, the app binary was
missing or isn't a file: build it, then restart the host. Stop with Ctrl-C (SIGINT, or SIGTERM): the host closes
every server's stdin, kills any still running after 10 s, removes `host.sock` and releases `host.lock`.

**B: a dev LaunchAgent**, if the host must outlive the terminal. It uses its own label and stays out of
`~/Library/LaunchAgents`, so it is never loaded at login (unverified: `launchctl bootstrap` of a plist kept elsewhere).

```sh
cat > "$DEV/org.agentic.office-host.dev.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>org.agentic.office-host.dev</string>
  <key>ProgramArguments</key><array>
    <string>$DEV/bin/org-office-host</string><string>--server-bin</string><string>$DEV/bin/org-office-server</string>
  </array>
  <key>EnvironmentVariables</key><dict>
    <key>ORG_OFFICE_HOME</key><string>$DEV/home</string>
    <key>ORG_OFFICE_DEV_KEY_DIR</key><string>$DEV/keys</string>
  </dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>10</integer>
  <key>ExitTimeOut</key><integer>20</integer>
  <key>ProcessType</key><string>Interactive</string>
  <key>StandardErrorPath</key><string>$DEV/logs/host.log</string>
  <key>StandardOutPath</key><string>$DEV/logs/host.log</string>
</dict></plist>
EOF
plutil -lint "$DEV/org.agentic.office-host.dev.plist"
launchctl bootstrap gui/$(id -u) "$DEV/org.agentic.office-host.dev.plist"
launchctl print gui/$(id -u)/org.agentic.office-host.dev | grep -E "state|pid"
```

Stop and unload: `launchctl bootout gui/$(id -u)/org.agentic.office-host.dev` (SIGTERM; with `KeepAlive`, a plain
`kill` only restarts it). Run only one host per home: a second one exits with `another org-office-host is already
running for …`.

## 4. A command-line client for host.sock

The wire format is one compact JSON object per line (`crates/org-proto/src/frame.rs`): request `{"id","method","params"}`,
response `{"id","result"}` or `{"id","error"}`. No CLI in the repo sends host calls: `org-agent` only uses the local
spool, and `org-client` is a library. The host answers every request on the connection. This script reads one response line
for its one request, so it doesn't depend on the host closing the connection after a half-close (hosts before the
fix of 2026-11-08 never did, so a read-to-EOF client hung):

```sh
cat > "$DEV/hostcall.py" <<'EOF'
import json, socket, sys
path, method = sys.argv[1], sys.argv[2]
params = json.loads(sys.argv[3]) if len(sys.argv) > 3 else {}
s = socket.socket(socket.AF_UNIX); s.settimeout(45); s.connect(path)
s.sendall((json.dumps({"id": 1, "method": method, "params": params}, separators=(",", ":")) + "\n").encode())
f = s.makefile("rb")
for line in f:  # events may come first (after `subscribe`): print up to the answer
    message = json.loads(line); print(json.dumps(message, indent=2))
    if message.get("id") == 1: break
EOF
python3 "$DEV/hostcall.py" "$ORG_OFFICE_HOME/host.sock" hello
```

`hello` answers `api_version` 1 and the capabilities `office.app_token`, `office.restart`, `office.status_changed`.
(unverified: `/usr/bin/python3` needs the Xcode command line tools.)

## 5. Create "Agentic Consulting"

**From the app (the real check):**

```sh
cd "$REPO/apps/desktop"
AGENTIC_ORG_OFFICE=real bun run tauri dev     # ORG_OFFICE_HOME must be exported in this terminal
```

`AGENTIC_ORG_OFFICE=real` selects the real office on this branch. On `v1` it is already the default, and only
`=mock` picks the mock. In the app: Welcome → **Create organization** → *Organization name* `Agentic Consulting`
→ **Create organization**. The steps "Start the office on This Mac" (`hello`) and "Create Agentic Consulting" send
`organization.create` (`src/lib/create-flow.ts`, `RealOffice::create_organization`). The host holds a create up to 30 s.

**Fallback without the app:** `organization.create` isn't app-only, so any process of this user can send it:

```sh
python3 "$DEV/hostcall.py" "$ORG_OFFICE_HOME/host.sock" organization.create '{"name":"Agentic Consulting"}'
```

The org then shows under **Organizations on this Mac** on the Welcome screen. Send a create once only: a second
one answers `already_exists`.

## 6. Check it works

```sh
python3 "$DEV/hostcall.py" "$ORG_OFFICE_HOME/host.sock" office.list         # the HQ: "role": "hq", "status": "active"
python3 "$DEV/hostcall.py" "$ORG_OFFICE_HOME/host.sock" organization.list   # Agentic Consulting with its HQ
ls "$ORG_OFFICE_HOME/offices" "$ORG_OFFICE_HOME/run" "$ORG_OFFICE_DEV_KEY_DIR"   # <id>/, <handle>.sock, <id>.key
ps -axo pid,comm | grep -E "org-office|agentic-org"   # the app's path must equal ORG_DEV_APP_PATH
```

- The office is active: `status` is `active` with no `status_reason`. `starting` means wait a moment.
  `unavailable` with `key_missing` means the host has no key for it (section 8).
- The app shows the org: it opens into the shell, the org chart loads, and **Offices** shows HQ as active. Opening
  needs `office.app_token` (the office refuses `org.snapshot` without the app's bind), so the host log must show
  `DEVELOPMENT FALLBACK: an app-only call was allowed for the unsigned <path>`. A `not_authorized` there means the
  app's executable isn't exactly `ORG_DEV_APP_PATH`: fix the path, rebuild the host (step 2), copy it, restart it.

## 7. Logs

| What | Where |
| --- | --- |
| Host | its stderr, lines starting `org-office-host:`: the terminal (A) or `$DEV/logs/host.log` (A with `tee`, B) |
| Each office server | `$ORG_OFFICE_HOME/logs/<handle>.log` (the last 12 characters of the office id), rotated at 10 MiB to `.log.1`. The file appears only once that server first writes to its stderr |
| App (Rust shell) | stderr of the `bun run tauri dev` terminal (no log plugin) |
| App (UI) | the WebView's console (unverified: right-click → Inspect in a debug build) |

## 8. Stop and clean up

1. Quit the app (closing the window only hides it: quit from the menu bar, or Ctrl-C in its terminal).
2. Stop the host: Ctrl-C (A), or `launchctl bootout gui/$(id -u)/org.agentic.office-host.dev` (B). Its office
   servers stop with it. Check: `pgrep -fl org-office` prints nothing.
3. Remove the dev setup:

```sh
rm -f "$DEV/org.agentic.office-host.dev.plist"
rm -rf "$DEV/home" "$DEV/keys"     # or the whole "$DEV"
```

- **Deleting a key makes its office unreadable forever.** The databases are SQLCipher, and the key is only in
  `$DEV/keys`. Delete the keys only together with the home they belong to.
- **Dev offices made before key custody** were keyed by the server, in the login Keychain (service
  `org.agentic.office`, account = office id). The host never reads those, so such offices show `unavailable` /
  `key_missing` and must be created again. Removing the old items is optional. List the ids first, then per id
  (not run while writing this runbook):

```sh
security delete-generic-password -s org.agentic.office -a <office id>
```

## 9. Known gaps for the switchover

1. **Unmerged:** the host and server halves of key custody (section 0) aren't on `v1`, and their merge hasn't been
   built or tested.
2. **No signing:** no Developer ID, no `ORG_APP_TEAM_ID`, no `keychain-access-groups` entitlement. A **release** host
   refuses to start without `ORG_HOST_KEYCHAIN_GROUP` and the matching entitlement, so `bun run install:dev`
   (release host, ad-hoc signed, registered through `SMAppService`) builds a login service that can't start today.
   `stage-office.sh` and `INSTALLING-THE-OFFICE.md` still mention `river/office-host`. A release host also gives an
   office's key only to a server signed as `com.ellosoft.agentic-org.org-office-server` with the team's Developer
   ID and the hardened runtime (the host itself is `com.ellosoft.agentic-org.org-office-host`), and with no
   `ORG_APP_TEAM_ID` compiled in it starts no server at all; the bundle's signing doesn't set those identifiers
   yet. **On the first signed bundle, check** that the release host admits its own hardened Developer ID server
   (the host's log says nothing like `refused office server`, and the office becomes active): the check takes
   the suspended child's audit token through `task_name_for_pid`, which the tests could exercise only on Apple's
   own binaries from an unsandboxed test process. If it's refused, every office stays offline.
3. **The app can't start a dev host:** the registration (`SMAppService.agent`) works only from a bundle and runs the
   bundled release host. A dev run relies on the host from section 3 (unverified: what `SMAppService` reports
   to an unbundled `tauri dev` binary; expect "The office isn't installed" if the host is down).
4. **Dev-only fallback:** the app-only calls trust an unsigned app by its exact path, a debug-build-only check fixed
   at compile time. Every change of the path, or of the target folder, needs a host rebuild and restart.
5. **Orphan-key sweep is off in debug builds** (logged at startup): removing an office folder by hand leaves its
   `<id>.key` in `$DEV/keys`. Release builds sweep at startup, on the account's own office home only: an orphaned
   key is marked first (`<home>/orphan-keys.json`) and deleted only if it's still orphaned at a start 7 or more
   days later.
6. **The host's own log is stderr only.** The bundled plist has no `StandardErrorPath`, so under launchd it is
   lost (unverified: launchd discards it). The draft in River's notes says the host logs under `<home>/logs/`, but
   only the office servers do.
7. **`org-agent` doesn't talk to the host yet** (spool only), and there's no CLI for host calls besides
   `hostcall.py` above.
8. The app's data folder is shared with a future real install (section 1).
