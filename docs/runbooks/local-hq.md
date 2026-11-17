# Runbook: a development host on this machine

What can be built and run from this checkout today, and how to do it without touching
a real installation. Checked against the source on 2026-11-17; recheck against the
commit you work on. [Executable migration](../design/executable-migration.md) is the
reference for what each executable does now.

## What works and what does not

| Step | Today |
| --- | --- |
| Build the four executables | macOS: CI builds the whole workspace there. Linux: all four build. The host refuses to start an office worker there: the check that a started worker is the intended program exists only for macOS, and the Linux version refuses rather than use a weaker check (`crates/org-host/src/supervisor/channels.rs`, `crates/org-host/src/spawn.rs`). Increment X2 brings the host to Linux. |
| `--help` and `--version` of each executable | Work. |
| `agentic-org serve` (debug build) against a disposable home | macOS: starts the host in the foreground (from the source; not run for this runbook). Its socket answers only `hello` and `instance.status`, and refuses every call while no instance is registered. |
| `agentic-org setup` | Refuses and changes nothing. Increment A2 makes it work. |
| `agentic-org-web` | Refuses to open a listener. Increment A3 makes it work. |
| Create an organization, sign in, open the browser application against a server | Not possible yet (increments A3 and A4). |
| The host starting an office worker | macOS only today (`crates/org-host/src/spawn.rs`); increment X2 adds Linux. |
| The browser application on its own | `bun run dev` in `apps/agentic-org-ui`; areas without a server adapter show as unavailable. |

## 1. A disposable home

Use an empty, owner-private directory, never a real office home. In a terminal at the
repository root:

```sh
export HQ_REPO="$(pwd -P)"
export HQ_DEV="$(mktemp -d /tmp/ao-hq.XXXXXX)"
chmod 700 "$HQ_DEV"
mkdir -m 700 "$HQ_DEV/home" "$HQ_DEV/keys"
export ORG_OFFICE_HOME="$HQ_DEV/home"
export ORG_OFFICE_DEV_KEY_DIR="$HQ_DEV/keys"
```

A debug host refuses to start without `ORG_OFFICE_DEV_KEY_DIR`, an absolute folder
outside the office home where it keeps office database keys as plain files instead of
in the Keychain. Those keys exist only because the office database is encrypted today;
increment A1 removes that encryption. Keep socket paths short: macOS limits a socket
path to 103 bytes.

## 2. Build

```sh
cargo build --locked -p agentic-org -p agentic-org-office -p agentic-org-web -p agent-os
./target/debug/agentic-org --help
./target/debug/agentic-org serve --help
./target/debug/agentic-org-office --help
./target/debug/agentic-org-web --help
./target/debug/agent-os --help
```

There is no separate host executable: host code is the library `crates/org-host`,
composed by `agentic-org serve`.

## 3. Start the host

```sh
./target/debug/agentic-org serve \
  --home "$ORG_OFFICE_HOME" \
  --server-bin "$HQ_REPO/target/debug/agentic-org-office"
```

It stays in the foreground until Ctrl-C or SIGTERM. `--home`, `--server-bin` and
`--app-team-id` are accepted by debug builds only; a release build refuses them. A
fresh home has no registered instance, so every call to the host socket other than
`hello` is refused. Do not edit registry, database or marker files by hand to get
further: setup (A2) is what registers an instance.

## 4. Logs

The host logs to stderr. Each office worker it starts logs to
`<home>/logs/<handle>.log`, owner-only, rotated at 10 MiB (`crates/org-host/src/logs.rs`).
Never paste key bytes, passwords, session proofs or recovery codes into a log or a report.

## 5. Stop and clean up

Send Ctrl-C to the foreground host, or SIGTERM to its process, and wait for it to exit.
Then remove exactly the directory created in step 1:

```sh
rm -rf "$HQ_DEV"
```

Never run that against a real office home.
