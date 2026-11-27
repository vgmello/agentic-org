# `agentic-org` CLI Reference

`agentic-org` is the human/operator command line and service host entrypoint for **Agentic Org**.
Managed AI agents use the separate `agent-os` binary (`agent-os --help`).

Run `agentic-org help <command>` or `agentic-org <command> --help` for live in-binary usage; every
command's help ends with copy-paste `Examples:`.

---

## Quickstart: Create an Organization & Enroll a Remote Satellite Office

### 1. Set up HQ with mTLS federation enabled

```bash
agentic-org setup \
  --name hq \
  --federation-bind 0.0.0.0:7443 \
  --federation-advertise hq.example.com
```

For headless or container setups, pass `--non-interactive --config <file>` with an optional `"federation"` JSON object:

```json
{
  "version": 1,
  "service": "foreground",
  "root": "/var/lib/agentic-org/hq",
  "listen": {
    "address": "127.0.0.1",
    "port": 7420,
    "scheme": "http_loopback",
    "origin": "http://127.0.0.1:7420",
    "hsts": "off"
  },
  "federation": {
    "bind": "0.0.0.0:7443",
    "advertise": ["hq.example.com"]
  }
}
```

### 2. Create the organization on HQ

```bash
agentic-org org create "Acme" \
  --slug acme \
  --repo https://git.example/acme.git \
  --branch main \
  --name hq \
  --json

agentic-org org list --name hq --json
agentic-org office list --name hq --json
```

### 3. Invite and enroll a remote satellite office

1. **On HQ**, create a one-time invitation token:
   ```bash
   agentic-org office invite "Berlin" \
     --address hq.example.com:7443 \
     --name hq \
     --json
   ```
2. **On the remote satellite machine**, set up the instance (if not already set up) and join using the token via stdin or a `0600` file, noting the printed `check_code`:
   ```bash
   agentic-org setup --name sat
   printf '%s' "$INVITATION_TOKEN" | agentic-org office join --invitation-stdin --yes --name sat --json
   ```
3. **Back on HQ**, verify the pending invitation (`status: "awaiting_approval"`; HQ intentionally omits `check_code` so it must be read out-of-band from the satellite) and approve it:
   ```bash
   agentic-org office invitations --name hq --json
   agentic-org office approve <invitation_id> --code <check_code> --name hq --json
   ```
4. **Verify live mTLS replication sync** on both satellite and HQ:
   ```bash
   agentic-org office sync-status --name sat --json
   agentic-org office sync-status --name hq --json
   ```

---

## Authentication Modes

- **Local authless HTTP (default local HQ):** Omit `--login`; commands use `session.local` over `host.sock` automatically with no password or browser step. `session.local` fails closed immediately whenever authentication is enabled or HTTPS is active.
- **Browser-approved sign-in (auth enabled / HTTPS / Linux):** Omit `--login`; commands print a one-time 10-minute approval link to `stderr` (keeping `--json` on `stdout` clean) and continue once approved in a signed-in browser.
- **Explicit password sign-in (`--login <user>`):** Verifies the host binary on macOS and prompts for the password on the terminal. Refused on Linux/containers where peers are identified by UID alone.
- **Fresh satellite bootstrap:** On a newly installed satellite with zero administrator accounts where `session.local` is unavailable (such as an opt-in HTTPS or foreground/published HTTP instance), `office join`, `office remove-pending`, and `office re-enroll` use sessionless `local-bootstrap` automatically after verifying open `instance.status`.

---

## Global Options, Output and Exit Codes

Every command takes these flags (before or after the command):

- `--json` — bare switch (no value), accepted before or after any command (including office commands such as `agentic-org directory list --json` and `agentic-org --json directory list`). Prints the result as JSON on `stdout` (indented on a human terminal, one compact line in a pipe or agent-harness PTY); an error as `{"error": {"code", "message", "help", "exit_code"}}` on `stderr`, with an empty `stdout`. Implies no color and no prompts.
- `--format json|compact|yaml|tsv|table` — explicit output format override (before or after any command; conflicts with `--json` and `-q`).
- `-q, --quiet` — only the primary IDs, one per line (nothing for an empty list or a `--dry-run`). Cannot be combined with `--json` or `--format` (exit `2`).
- `--no-input` (or `AGENTIC_ORG_NO_INPUT=1`) — never prompt; a missing value or confirmation fails instead.
- `--dry-run` — show what would change (`[DRY-RUN] ...` on `stderr`, or `{"dry_run": true, ...}` under `--json`) and change nothing. Office commands (`task`, `msg`, ...) always receive it: a write is previewed, and a command that changes nothing refuses it (exit `2`) instead of running as if it were not given. A `--dry-run` that is a flag's value (`--body --dry-run`) or follows `--` is text, not the flag.
- `--color auto|always|never` — `auto` honours `NO_COLOR`, `FORCE_COLOR`/`CLICOLOR_FORCE`, `CLICOLOR=0`, `TERM=dumb` and whether the stream is a terminal.

Shared output-format environment override & precedence (applies across both `agentic-org` and `agent-os`):

- `AGENT_OS_FORMAT` — sets the default output format when no explicit CLI output flag is passed (`json`, `compact`, `yaml`, `tsv`, `table` for `agentic-org`; `json`, `compact`, `yaml` for `agent-os`; case-insensitive and trimmed; invalid values fail closed with exit `2`). Managed agent runtimes automatically receive `AGENT_OS_FORMAT=json`.
- Precedence order: explicit CLI flags (`--json`, `-q`, `--format <fmt>`) > `AGENT_OS_FORMAT` > agent-harness PTY / non-TTY pipe > human TTY.
- When `AGENT_OS_FORMAT` is unset on a TTY inside a known agent harness (`AGENT_OS_SESSION_TOKEN`, `ANTIGRAVITY_AGENT`, `CLAUDECODE`, `CODEX_CI`), `agentic-org` emits untruncated compact JSON instead of a narrow human table; piped non-TTY stdout preserves headerless TSV for shell scripts.
- On narrow human terminals (such as 80-column TTYs), borderless tables protect high-signal distinguishing columns (`HANDLE`, `NAME`, `KEY`, `SLUG`, `EMAIL`, `TITLE`, `STATUS`, `ROLE`) by shrinking and eliding low-priority metadata columns (`ID`, `*_ID`, timestamps, `NOTES`, `OFFICE`) first so identifiers such as `@backend` and `@backend-2` stay distinguishable.

Directories (each an absolute path; an empty variable is unset; a relative one is refused with exit `1`, `cli::config::invalid`, and never read against the working directory):

- `AGENTIC_ORG_CONFIG_DIR` — where the context saved by `agentic-org context use` lives (`cli-context.json`, owner-only). Default: this account's own folder.
- `AGENTIC_ORG_CACHE_DIR` — the completion cache (`completion/`, created owner-only `0700`). Default: an absolute `XDG_CACHE_HOME` joined with `agentic-org`, else `~/Library/Caches/agentic.org/cli-cache` on macOS and `~/.cache/agentic-org/cli-cache` on Linux — never a shared temporary folder.

Output is shaped for the reader: on a terminal, lists are borderless tables and records aligned cards; in a pipe, lists are header-less tab-separated lines with bare values and no color; notices (empty lists, progress, approval links, `✓` confirmations) go to `stderr` only. Every value that came from a host, an office or a file is escaped before it reaches a terminal (control characters, `U+2028`/`U+2029` and bidirectional overrides are printed as `\u{..}`), and DEL, C1 controls, `U+2028`/`U+2029` and bidirectional controls are escaped (`\uXXXX`) inside every JSON document, the `--json` error envelope included.

Exit codes: `0` success (also when the reader of `stdout` went away, e.g. `| head -1`); `1` a refusal or failure; `2` a command line that cannot be used, or a destructive command without `--yes` where no one can be asked; `4` not signed in or not allowed; `130` cancelled (Ctrl+C at a prompt, or a declined confirmation). Office commands (`task`, `msg`, ...) keep their own `0`/`1`/`2`.

Destructive commands (`office restart|decline|cancel|remove|remove-pending|join|re-enroll`, `context clear`, `admin tls set`, `service stop`) ask for confirmation on a terminal and require `-y, --yes` when no terminal can be asked (a pipe, `--no-input`, `--json`, `-q`). `admin reset` shows its link on a terminal only and keeps its own local confirmation (Touch ID or the login password on a Mac, root on Linux); it also asks a person at the terminal, and needs `--yes` under `--no-input`, `--json` or `-q`. `serve`, `setup` and `recovery` refuse `--json`, `-q`, `--format` and `--dry-run` (exit `2`; `setup` also when they follow its own flags), and `sign-in` refuses `--dry-run`, instead of ignoring them. `setup` asks for its port only when someone can be asked: never under `--no-input` or `AGENTIC_ORG_NO_INPUT=1`, even on a terminal.

### Selecting the Instance and Office

Host commands act on one instance and, where it matters, one office. Each is chosen in this order: the flag (`--name`/`--instance`, `--office`); the environment (`AGENTIC_ORG_INSTANCE`, `AGENTIC_ORG_OFFICE`; empty means unset); the nearest `agentic-org.toml` in the working directory or above it; the context saved by `agentic-org context use`; then the built-in default (the `default` instance, and the office the host picks). `agentic-org.toml` may hold only these two non-secret names:

```toml
instance = "lab"
office = "0193a5c0-0000-7000-8000-000000000001"
```

Any other key, a control character or an over-long value makes it invalid (exit `1`, `cli::config::invalid`); the error names the line and column and never repeats the file's text. A saved context whose instance name is not an instance name (or holds a control or bidirectional character), or whose office is not an ID, or that is a symbolic link or not a regular file, is refused the same way until `agentic-org context clear` removes it. An `agentic-org.toml`, a saved context or an `--invitation-file` is read only as a regular file of at most 64 KiB, never through a symbolic link (pipe an invitation with `--invitation-stdin` instead). `agentic-org status` shows what was chosen and where it came from.

### Shell Completion

- `agentic-org completion <bash|zsh|fish|elvish|powershell> [--dynamic]`
  Print the shell's completion script; `--dynamic` (or `COMPLETE=<shell> agentic-org`) asks the binary on every `<TAB>`, completing instance names, organizations, offices and projects from the cache the list commands fill, without contacting a host.
  **Use when:** enabling `<TAB>` completion of commands, flags and the names this host knows.

---

## Commands

### Instance Setup & Lifecycle

- `agentic-org setup [--name <instance>] [--port <port>] [--federation-bind <IP:port>] [--federation-advertise <host,...>]`
  Set up an instance and register its per-user service (`launchd`/`systemd`). `--federation-bind` enables HQ mTLS federation; `--federation-advertise` adds comma-separated ASCII DNS hostnames or bare IP SANs (without ports).
  **Use when:** initializing a new host instance on this computer, registering its OS background service, or enabling HQ mTLS federation for remote satellites (run `setup` first and `office join` second when setting up a satellite).
- `agentic-org setup --non-interactive --config <file> [--name <instance>]`
  Non-interactive setup from a strict JSON file (cannot be combined with `--port` or `--federation-*`; set those in the file).
  **Use when:** automating headless, CI, or container instance setup from a checked-in or generated JSON config file.
- `agentic-org serve [--name <instance>]` / `agentic-org serve --setup <file> [--name <instance>]`
  Run the installed service host in the foreground (or initialize and serve a foreground config file).
  **Use when:** running the host directly in a container, terminal, or service unit instead of via `service start`.
- `agentic-org service start [--name <instance>]`
  Start the instance's registered OS service.
  **Use when:** launching a stopped background host service registered with `launchd`/`systemd`.
- `agentic-org service stop [--name <instance>] [--yes]`
  Stop the instance's registered OS service (asks first on a terminal; `--yes` where no one can be asked).
  **Use when:** cleanly shutting down the registered background host service for maintenance.
- `agentic-org service status [--name <instance>]`
  Inspect whether the instance's registered OS service is running.
  **Use when:** checking host service health before running organization or office commands.
- `agentic-org instance rebind [<name>] [--name <instance>] [--yes] [--dry-run] [--json]`
  Verify and rebind a stopped instance's recorded directory identity (`device`/`inode`) in `instance.json` and `locator/instances.json` after copying or restoring its root onto a new inode. Refuses while `host.lock` or any `office.lock` is held, requires explicit confirmation (`--yes` when non-interactive), and fails closed if configuration matches neither the recorded nor actual filesystem identity.
  **Use when:** host startup reports `RootUnavailable` after restoring or moving an instance root directory onto a new inode.
- `agentic-org status [--name <instance>] [--office <office_id>]`
  Show the selected instance and office (and which tier chose them), the instance's service state and the saved context.
  **Use when:** checking which instance and office commands act on before running them.
- `agentic-org logs [--instance <instance>] [--office <office_id>] [--lines <N>] [--json] [-q]`
  Show bounded, symlink-safe, secret-redacted host and office stderr logs (`1..=2000` lines; every line scrubbed through `org-redact` plus PEM/token-URL/recovery-code passes).
  **Use when:** inspecting recent operational host or office logs safely without exposing Bearer tokens, cookies, API keys, vault credentials, or PEM private keys.
- `agentic-org doctor [--instance <instance>] [--office <office_id>] [--org-dir <path>] [--json] [-q]`
  Run read-only host layout, owner-only permissions, `host-registry.created` consistency, socket, release-pin, AppArmor/userns, and durable organization-state integrity checks (never reading `private/` or `protected/`). Exits `0` when all checks pass and `1` if any check fails.
  **Use when:** diagnosing host permissions, stale `host-registry.created` markers, sockets, AppArmor confinement, or durable organization-state integrity.
- `agentic-org support [--instance <instance>] [--output <path>] [--lines <N>] [--json]`
  Generate a deterministic, 100% secret-redacted JSON diagnostic bundle (`agentic-org-support-v1`) combining instance summary, `doctor`, and redacted `logs`; writes atomically with mode `0600` when `--output <path>` is supplied (refusing symlinks) and never reads `host-registry.db`, `private/`, `protected/`, or private keys.
  **Use when:** collecting a shareable troubleshooting bundle without leaking credentials, keys, or private databases.
- `agentic-org uninstall [--instance <instance>] [--yes] [--dry-run] [--json]`
  Cleanly uninstall a stopped instance's runtime, sockets, logs, setup records, locator entry, saved CLI context, and host registry metadata — explicitly clearing `host-registry.created` so a fresh setup can initialize cleanly while leaving non-installed files untouched. Fails closed without `--yes` or while `registry.lock`, `host.lock`, any `office.lock`, or a registered service is active.
  **Use when:** cleanly tearing down a stopped instance and clearing `host-registry.created` before a fresh setup.

### Sign-In, Context & Administration

- `agentic-org sign-in [--name <instance>] [--office <office_id>] [-- <command...>]`
  Start a browser-approved terminal session without a terminal password prompt.
  **Use when:** authenticating a terminal via a browser approval link (especially on Linux, containers, or HTTPS hosts where terminal passwords are refused) or executing one office command over a grant-bound channel.
- `agentic-org context show`
  Show the saved non-secret default instance and office selection.
  **Use when:** checking which default `--name` and `--office` future CLI commands will target.
- `agentic-org context use --office <office_id> [--name <instance>]`
  Save a default instance and office selection.
  **Use when:** pinning a default office so you do not repeat `--office` and `--name` on every command.
- `agentic-org context clear [--yes]`
  Remove the saved CLI context.
  **Use when:** clearing saved defaults before switching to another instance or office.
- `agentic-org admin reset [--name <instance>] [--user <user>] [--yes]`
  Issue a one-time administrator registration or password-reset link after local OS confirmation.
  **Use when:** recovering administrator access, generating an initial registration link, or cancelling an unused registration link.
- `agentic-org admin tls set --cert <file> --key <file> --login <login> [--name <instance>] [--yes]`
  Install or replace the instance's browser HTTPS certificate pair without restarting the host.
  **Use when:** installing or rotating a supplied PEM certificate chain and unencrypted private key on an HTTPS instance (instances start on plain HTTP by default; enable opt-in HTTPS in-app via `POST /api/v1/instance/tls` or via `"scheme": "https"` in setup config per ADR-0033).
- `agentic-org recovery status|release [--name <instance>] [--org <name> | --org-id <id>] [--login <login>] [--redeliver] [--package <file>]`
  The protected organization recovery client; recovery codes are read from the controlling terminal only.
  **Use when:** checking or releasing an organization's recovery package as its administrator.

### Organizations (`org`)

- `agentic-org org create <name> --slug <slug> --repo <https://...|ssh://...> [--branch <state_branch>] [--login <login>] [--name <instance>] [--no-wait] [--json]`
  Create a new organization and its HQ office on this host (default `--branch main`). Waits for completion unless `--no-wait` is passed.
  **Use when:** bootstrapping a new organization and its HQ office from a remote Git repository.
- `agentic-org org list [--login <login>] [--name <instance>] [--json]`
  List organizations visible to the account and any creates in flight.
  **Use when:** finding an `org_id` or HQ office ID, or checking progress on a `--no-wait` organization create.
- `agentic-org org inspect [--format json|yaml|compact]`
  Read the organization's record from its office (the office command `org inspect`).
  **Use when:** checking the organization's name, identifiers and settings from the terminal.

### Backup & Restore (`backup`, `restore`)

- `agentic-org backup <org_id> --out <path> [--office <office_id>] [--encrypt] [--passphrase-file <path> | --passphrase-fd <fd>] [--name <instance>] [--json]`
  Write one consistent, deterministic `.tar.gz` of the office's `<data-root>/<org_id>/durable/` folder (Org Git `state/`, `messaging/`, `recovery/` with the acceptance journal, `authority/`, `accounting/`, `budgets/`, `checkpoints/`, `blobs/` when present, `local/offices/<id>/` less `enroll-tmp/`, the unattached uploads in `attachment-staging/` and each agent's `runtime/` and `tmp/`), with a versioned `manifest.json` (org, office, journal incarnation and head, `created_at`, and every file's length and SHA-256). `private/task-notes.db` and HQ's `private/user-space.db` are each carried as a consistent SQLite `VACUUM INTO` copy (never their `-wal`, `-shm` or `-journal` files); the disposable `runtime/` folder never is. The office must be stopped (a held `host.lock` or `office.lock` is refused), and the archive is written owner-only and atomically and never replaces a file. `--encrypt` seals it with a passphrase (Argon2id 64 MiB, t=3, p=4 and AES-256-GCM) read from the terminal, `--passphrase-file` (owner-only) or `--passphrase-fd`, never from the command line.
  **Use when:** taking a local safety-net backup of an organization before maintenance, an upgrade or risky work, with the office stopped (`agentic-org service stop`).
- `agentic-org restore --verify <archive> [--to <dir>] [--work-dir <dir>] [--passphrase-file <path> | --passphrase-fd <fd>] [--json]`
  Verify an archive and change nothing: the encryption tag (detected from the archive's first bytes), every member against the manifest (no `..`, absolute paths, links, special, unlisted, duplicated or oversized members), SQLite's `integrity_check` on the notes copy and on the private file space copy, the journal to its recorded head, the replication state and replicated HQ owner when present, and a full rebuild of the office's projections from it in an owner-only disposable folder (beside `--to` or the archive, or in `--work-dir`; never the shared temporary folder). Paths that differ only in letter case, and other spellings of a private database or of a path a backup never carries, are refused. `--to` also extracts the verified archive into a new or empty folder.
  **Use when:** proving a backup actually restores before you rely on it, or inspecting its files without touching the organization.
- `agentic-org restore <archive> --org <org_id> [--office <office_id>] [--yes] [--passphrase-file <path> | --passphrase-fd <fd>] [--name <instance>] [--json]`
  Verify the archive in full, then replace the stopped office's durable folder with it (the previous one is kept as `restore-backups/before-*` in the owner-only `restore-backups` folder beside it, where the restore also stages and checks the archive; agent sandboxes never see it) and discard the rebuildable projections. It never resets the host's writer custody: an archive of another incarnation, behind the office's journal or off its chain is refused. Host-local custody, the host registry and accounts, and the office secrets key in the Keychain or credential store are not in a backup; moving an office to another machine is not supported in v1.
  **Use when:** putting an organization's durable data back on this computer after it was lost or damaged, with the office stopped.

### Offices & Multi-Office Federation (`office`)

- `agentic-org office list [--org <org_id>] [--login <login>] [--name <instance>] [--json]`
  List offices visible to the account (plus local satellite offices for the instance administrator on `host.sock`).
  **Use when:** discovering office IDs, HQ/satellite roles, or checking live readiness and crash backoff status.
- `agentic-org office restart <office_id> [--yes] [--login <login>] [--name <instance>] [--json]`
  Restart an offline or unavailable office (instance administrator or organization owner).
  **Use when:** recovering an offline or crashed office without restarting the entire host.
- `agentic-org office invite <office_name> --address <host:port> [--address <host:port>...] [--org <org_id>] [--ttl <secs>] [--re-enroll-office <office_id>] [--login <login>] [--name <instance>] [--json]`
  Create a one-time satellite invitation token on HQ (owner-only; requires HQ federation listener).
  **Use when:** initiating satellite enrollment from HQ (or generating a re-enrollment token with `--re-enroll-office`).
- `agentic-org office invitations [--org <org_id>] [--login <login>] [--name <instance>] [--json]`
  List satellite invitations on HQ with status and expiry (check codes are shown only on the satellite).
  **Use when:** viewing pending or expired satellite invitations on HQ before approving, declining, or cancelling.
- `agentic-org office approve <invitation_id> --code <check_code> [--org <org_id>] [--login <login>] [--name <instance>] [--json]`
  Approve a satellite enrollment on HQ using the satellite's out-of-band check code and issue its Org-CA mTLS certificate.
  **Use when:** completing satellite enrollment on HQ after reading the `check_code` printed on the satellite.
- `agentic-org office decline <invitation_id> [--org <org_id>] [--yes] [--login <login>] [--name <instance>] [--json]`
  Decline a satellite invitation on HQ.
  **Use when:** rejecting a pending satellite join request on HQ.
- `agentic-org office cancel <invitation_id> [--org <org_id>] [--yes] [--login <login>] [--name <instance>] [--json]`
  Cancel an active satellite invitation on HQ.
  **Use when:** revoking an unused satellite invitation on HQ before approval completes.
- `agentic-org office remove <office_id> [--org <org_id>] [--yes] [--login <login>] [--name <instance>] [--json]`
  Remove a satellite office on HQ and revoke its issued Org-CA certificates.
  **Use when:** decommissioning a satellite office from HQ and revoking its mTLS client certificates.
- `agentic-org office sync-status [--org <org_id>] [--office <office_id>] [--login <login>] [--name <instance>] [--json]`
  Show live federation and replication sync status for an HQ or satellite office.
  **Use when:** verifying live mTLS federation and replication health on either HQ or a satellite.
- `agentic-org office join (--invitation-file <path> | --invitation-stdin) [--yes] [--login <login>] [--name <instance>] [--json]`
  Join this host as a satellite office using an HQ-issued invitation token.
  **Use when:** enrolling this remote host as a new satellite office after running `agentic-org setup`.
- `agentic-org office remove-pending <office_id> [--yes] [--login <login>] [--name <instance>] [--json]`
  Cancel and remove a pending satellite join before approval completes.
  **Use when:** aborting an unapproved join on the satellite host before HQ approves it.
- `agentic-org office re-enroll <office_id> (--invitation-file <path> | --invitation-stdin) [--yes] [--login <login>] [--name <instance>] [--json]`
  Re-enroll an existing satellite office onto a replacement HQ after Git import.
  **Use when:** reconnecting an existing satellite office to a replacement HQ after Git recovery import.

### Projects, Repositories, Worktrees & Tasks

- `agentic-org project list [--login <login>] [--org <org_id>] [--include-archived] [--name <instance>] [--json]`
  List projects in an organization (`agentic-org project --help`).
  **Use when:** discovering active (or archived) projects, `P-<number>` keys, slugs, and owning teams.
- `agentic-org project inspect <project> [--login <login>] [--org <org_id>] [--name <instance>] [--json]`
  Inspect one project by ID, `P-<number>` key, or slug.
  **Use when:** checking one project's owning team, contributing teams, and archive status.
- `agentic-org project create <name> [--login <login>] [--org <org_id>] [--slug <slug>] [--owner-team <team_id>] [--parent-team <team_id>] [--name <instance>] [--json]`
  Create a project (creates a dedicated owning team automatically unless `--owner-team` is given).
  **Use when:** creating a new organization project with an accountable owning team.

### Providers, Releases, Messaging & Agents

- `agentic-org provider create <name> --key <key> --provider <type> --runtime <claude|codex|agy> [--auth-mode <subscription_cli|api_key|cloud_iam>] [--subscription-id <id>] [--max-turns <n>] [--org <org_id>] [--login <login>] [--name <instance>] [--json]`
  Create an org-wide provider connection and auto-generated model profile on HQ.
  **Use when:** enabling Claude, Codex, or Agy for agents across an organization.
- `agentic-org provider list [--include-archived] [--org <org_id>] [--login <login>] [--name <instance>] [--json]`
  List org-wide provider connections.
  **Use when:** finding a provider `connection_id` before binding it on an HQ or satellite office.
- `agentic-org provider bind <connection_id> --auth-ref <ref> [--binary <path>] [--account-ref <ref>] [--office <office_id>] [--org <org_id>] [--login <login>] [--name <instance>] [--json]`
  Bind a provider connection to the local HQ or satellite office.
  **Use when:** configuring a headless Linux satellite or HQ office to run local `claude`, `codex`, or `agy` turns.
- `agentic-org provider refresh <connection_id> --driver-version <ver> --model <id[:driver_id]> [--account-ref <ref>] [--ttl <secs>] [--office <office_id>] [--org <org_id>] [--login <login>] [--name <instance>] [--json]`
  Record a fresh local readiness and discovered-model observation for a bound provider connection.
  **Use when:** marking a headless office's local runtime binding ready for agent session routing (accepts comma-separated `--model` entries).
- `agentic-org provider bindings [--connection <id>] [--office <office_id>] [--org <org_id>] [--login <login>] [--name <instance>] [--json]`
  List provider bindings and local readiness on this office.
  **Use when:** verifying an office has a local binding for a provider connection.
- `agentic-org provider models [--org <org_id>] [--office <office_id>] [--login <login>] [--name <instance>] [--json]`
  List model profiles available for hiring.
  **Use when:** finding a `model_profile_id` before hiring an agent with `agentic-org hr hire`.
- `agentic-org release verify <bundle.tar.gz | bundle-dir> [--store <dir>] [--json]`
  Verify a signed release bundle's Ed25519 signature, closed inventory, and SHA-256 payload hashes offline without mutating the release store or touching running services (refusing symlinks, path traversal, and untrusted keys).
  **Use when:** verifying a prebuilt offline release archive or directory before installing or activating it.
- `agentic-org release install <bundle.tar.gz | bundle-dir> [--store <dir>] [--no-restart] [--json]`
  Verify Ed25519 signature, closed inventory, and executable hashes, then atomically pin a signed release into this host's release store. Each instance's registered service that `setup` wrote then moves to the new release (its `launchd` `ProgramArguments` / `systemd` `ExecStart`; arguments and environment unchanged): a running service holds the store, so it is stopped for the install and started again on the new release (`--no-restart` leaves it stopped); a stopped service stays stopped (a stopped `launchd` agent loads again at the next login or with `agentic-org setup`); a failed install starts again what it stopped. A service definition changed by hand, or one running a program outside this store, is left untouched and the install prints the steps to upgrade it. `--json` adds `services` (`instance`, `outcome` `refreshed|unchanged|refused|skipped|failed`, `was_running`, `running`, `program`, `note`), and `services_warning` when the registered services could not be read (the install goes on without them; standard error stays empty or holds only an error's envelope); `--dry-run` lists what would happen to each service (`action`, `running`, and `restart`: whether it would be started again).
  **Use when:** installing or upgrading prebuilt signed binaries on macOS or Linux without compiling from source (see [Remote Satellite Office Setup](remote-office-setup.md)).
- `agentic-org release status [--store <dir>] [--json]`
  Show the active pinned release ID, sequence, manifest SHA-256, and installed executable paths.
  **Use when:** verifying which signed release is currently active on this host.
- `agentic-org msg send|inbox|read|reply|ask|accept|withdraw|list|wait`
  Send direct messages, channel posts, asks, replies, or handoffs over an approved office channel.
  **Use when:** messaging local or remote office agents from your terminal without running a managed agent session.
- `agentic-org channel list|join|leave|mark-read|summarize`
  Discover and manage organization messaging channels over an approved office channel.
  **Use when:** finding `#channels` and what is new in them, joining, marking read or summarizing shared channels across offices.
- `agentic-org hr hire|list|inspect`
  Hire and manage agents across HQ and satellite offices over an approved office channel (confirm or withdraw pending proposals with `hr hire --id <proposal_id> [--withdraw]`). Pass `--bind-workspace <dir>` on `hr hire` (when hiring directly or confirming a draft) to hire the agent and bind a local directory or Git repository outside Git in one step so `agent-os` authenticates immediately without a separate approval code.
  **Use when:** hiring an agent onto HQ or a remote satellite office from the terminal, optionally binding a BYOA workspace inline.
- `agentic-org agent inspect|update|move|pause|resume|offboard|execution-get|execution-set|bind|approve|renew|sessions|revoke`
  Inspect or manage hired agents and their execution configuration (`model_profile_id`, `model_id`, `permission_preset`, optional `effort`, `system_prompt_markdown`) over an approved office channel. `execution-set` is a full replacement: `--effort low|medium|high|xhigh|max` sets the agent's reasoning effort (any other value is refused), and leaving it out returns the agent to the runtime's default. Claude Code receives the level as `--effort`; Codex receives it as `model_reasoning_effort`, with `max` running as Codex's highest level, `xhigh`. A changed effort recycles the agent's warm session. `hr hire --effort <level>` sets it at hire.
  **Use when:** checking agent status, placement, lifecycle state, or configuring an agent's model profile, permission preset, reasoning effort, and system prompt.
- `agentic-org agent bind --member <handle|employee_id> --workspace <dir> [--ttl <duration>] [--label <text>] [--takeover] [--json]`
  Bind a local workspace directly to an organization agent in one operator step without an approval code: reuses any existing workspace Ed25519 keypair (or generates a fresh one), authorizes an external lease through the office's single-active-lease rules (`member.external_bind`; supersedes any prior external lease, refuses a live managed turn unless `--takeover` parks it, refuses the Chief of Staff, and requires owner, hiring, manager, or team-lead authority), and writes `key.ed25519.pkcs8` and `binding.v1.json` (`0600`) inside `<application base>/agent-os/workspaces/<sha256>/` (`0700`, strictly outside Git). Subsequent `agent-os` commands run inside `<dir>` authenticate immediately via Ed25519 challenge-exchange.
  **Use when:** authorizing a local folder or Git repository to run `agent-os` as a BYOA agent in one operator command without `agent-os login`.
- `agentic-org agent approve <code> [--ttl <duration>] [--takeover]` (also `--code <code>`)
  Approve an external ("bring your own agent") workspace that ran `agent-os login --as @<handle>`: the code is the one `agent-os login` printed (`ABCD-EFGH`, valid 10 minutes, locked after 3 wrong attempts by people who may approve it; a person without authority over the agent never spends an attempt). The agent gets one active external lease (default `--ttl 24h`, 5 minutes to 7 days); approving supersedes the agent's previous external session and closes its connections. A live managed turn is refused unless `--takeover` parks it first; while the lease is active, the agent's wakes stay queued instead of starting managed runs. Only the owner, a person who may hire, or the agent's manager or team lead may approve; an agent can never approve itself.
  **Use when:** letting a terminal, IDE or external agent harness act as an organization agent after it asked to sign in.
- `agentic-org agent renew --member <handle|employee_id> [--ttl <duration>] [--session <session_id>] [--json]` (also positional `@<handle>`)
  Extend an agent's active external workspace lease by up to 7 days (default `--ttl 7d`, 5 minutes to 7 days) without requiring a new login or approval code (`member.external_renew`). Atomically updates the lease expiry on disk and records a `renewed` audit event; refuses expired or revoked leases fail-closed. Human-only (grant-bound operator with authority over the agent; agents and Tess are always refused).
  **Use when:** extending an active external workspace session before its lease expires so work continues uninterrupted.
- `agentic-org agent sessions [--member <handle|employee_id>] [--pending] [--limit <n>] [--json]` (also positional `@<handle>`, `--history-limit <n>`)
  With `--pending`: list all pending external workspace sign-in requests awaiting approval across the office (or filtered to `--member @<handle>`), showing `request_id`, `@handle`, masked `code_prefix` (`XXXX-••••`, never the secret half), client `hostname`, `workspace_path`, `workspace_label`, `pubkey_fingerprint`, `managed_turn_active`, expiry, and server-computed `workspace_familiarity` (`first_time`, `returning_new_key`, or `returning_known_key` with `last_approved_by` / `last_approved_at`). Human-only (`member.external_pending`). Without `--pending`: show an agent's active external lease (workspace, key fingerprint, approver, expiry) and its recent sign-in history, the last `--limit` entries (default 20, at most 200): lifecycle and security events only (`approved`, `renewed`, `superseded`, `token_issued`, `revoked`, `logout`, `offboarded`, `paused`). Commands an external session runs are counted in the office's memory, never written per command.
  **Use when:** reviewing pending external workspace sign-in requests (`--pending`) or checking which external workspace an agent is signed in from and until when.
- `agentic-org agent revoke @<handle> [<session_id>]` (also `--member`, `--session`)
  End an agent's external lease at once: its fast tokens stop working, its connections close, and its held wakes run again as managed turns. Pausing or offboarding the agent revokes it too.
  **Use when:** cutting off an external agent session immediately (a lost laptop, a finished pairing session).
- `agentic-org team list|inspect|create|update|move|archive|set-limit|request-limit|limit-requests|decide-limit`
  Discover, inspect, create, or manage organization teams and member-cap limits over an approved office channel. Team leads can request a `max_members` increase (`team request-limit --team <id> --max-members <n> --reason "..."`) routed to their manager, who lists (`team limit-requests`) and approves or denies it (`team decide-limit --request <id> --approve|--deny --if-match <rev>`).
  **Use when:** finding a `team_id`, creating a sub-team, or raising/requesting a team's `max_members` limit before hiring or moving agents.
- `agentic-org role list|inspect|create|update`
  Discover, inspect, or create role catalog entries over an approved office channel.
  **Use when:** inspecting or authoring reusable role definitions before hiring agents.
- `agentic-org who`
  List people and agents with live presence and `@handles` over an approved office channel.
  **Use when:** checking active handles and online presence across HQ and satellite offices.
- `agentic-org repo register|list|attach|status|fetch|push|set-credential|credential-status|revoke-credential`
  Register and manage organization/project Git repositories, store encrypted per-repository HTTPS tokens in an office vault, refresh an office's cache of the origin, and push task branches over an approved office channel.
  **Use when:** registering a Git repo, attaching it to a project's integration branch, checking office bare-cache status, fetching newly pushed origin branches (`repo fetch --repo <id-or-key>`), pushing a task branch, or managing scoped per-repo vault credentials.
- `agentic-org repo set-credential --repo <id-or-key> [--username <user>] [--token-file <path> | --token-stdin] [--office <office_id>] [--json]`
  Store an encrypted per-repository HTTPS token inside the target office's credential vault (`<office>/git-credentials/repos/<repository_id>/credential.v1.json`, `0700`/`0600`, denied in every agent sandbox) so worker offices never depend on personal host `gh` or `~/.git-credentials`. Tokens are read from `--token-file` (regular file only), `--token-stdin`, or an echo-off terminal prompt — never CLI flags.
  **Use when:** authorizing an HQ or satellite office to clone, fetch, or push a private HTTPS repository with a scoped fine-grained token.
- `agentic-org repo credential-status --repo <id-or-key> [--office <office_id>] [--json]`
  Show whether an office holds an encrypted per-repository HTTPS token (or host helper) for a repository, its canonical scope URL, and audit metadata without exposing secrets.
  **Use when:** verifying whether an HQ or satellite office has a scoped HTTPS token configured for a repository.
- `agentic-org repo revoke-credential --repo <id-or-key> [--office <office_id>] [--json]`
  Delete an office's stored per-repository HTTPS token from its credential vault.
  **Use when:** rotating or revoking a scoped repository HTTPS token on an office.
- `agentic-org worktree list|prepare`
  List or prepare isolated task Git worktrees over an approved office channel.
  **Use when:** preparing a clean per-task Git worktree before editing code or inspecting active worktrees.
- `agentic-org task create|list|inspect|update|transition|assign|comment|watch|unwatch|history|deps|dep-add|dep-remove|labels`
  Create, inspect, and manage tasks over an approved office channel.
  **Use when:** creating, filtering, updating, transitioning, assigning, commenting on, or tracking task dependencies from the terminal.
- `agentic-org checklist add|check|edit|remove|move`
  Manage task checklist items over an approved office channel.
  **Use when:** adding, checking/unchecking, editing, removing, or reordering task checklist steps.
- `agentic-org task-note add|list|get|update|remove|publish` (alias: `agentic-org task note ...`)
  Manage private task notes over an approved office channel.
  **Use when:** keeping private working notes on a task or publishing a copy as a shared task comment.
- `agentic-org directory|person|member|session|grant|process|skill|budget <verb>`
  The remaining office resources, over the same approved office channel as `task` and `msg`.
  **Use when:** browsing the directory, people and membership, sessions and grants, organization, team and project processes and standards, agent skills or budgets from the terminal.

Office commands (`msg`, `task`, `who`, `team`, ...) run the same command table as `agent-os`. On a terminal their answers are shown as tables and cards (or untruncated JSON inside an agent-harness PTY); in a pipe as headerless tab-separated lines (`tsv`); `--json` (before or after the command) prints the table's JSON envelope; `--format json|yaml|compact|tsv|table` prints exactly that format (and overrides `AGENT_OS_FORMAT`); `-q` prints IDs; structured JSON request bodies use `--input-json <object>`. `<resource> <verb> --help` (or `-h`) ends with a copy-paste `Example:`.

---

## `agent-os` Quick Pointer (Managed AI Agents)

`agent-os <resource> <verb> --help` (or `-h`) lists a command's flags and ends with a mechanical, copy-paste `Example:` built from the command's contract example.

Managed AI agents use the bound `agent-os` binary (`agent-os --help`, `agent-os <resource> --help`, `agent-os <resource> <verb> --help`, `agent-os <resource> <verb> --schema`). All commands use named flags only (no positional arguments) and return a single deterministic envelope.

`agent-os --help` opens with a four-line quickstart for the shell's binding state: an unbound shell points to `agent-os login --as @<handle>` and `agent-os context --offline`; a bound one to `agent-os context` (managed sessions already receive the rules on turn 1 and `agent-os login` returns them in `data.context`, so re-run it only after context compaction).

Contract rules every command follows:

- **Output format & structured JSON input.** `--json` (bare switch, before or after the command) selects JSON output; `--format json|yaml|compact` picks an explicit format; `AGENT_OS_FORMAT` sets the default when neither flag is given (managed agent runtimes receive `AGENT_OS_FORMAT=json`). Structured JSON request bodies use `--input-json <object>` (or `--input-file` / `--stdin` with `--input-format json|yaml`).
- **Lists are pages.** Every list takes `--limit` (1 to 200, default 20) and `--cursor`; `meta.next_cursor` names the next page and, on counted lists, `meta.total` counts every match (`msg inbox` reports no total, and `msg list` reports one only for a `--since-read` or `--after-id` read, whose `meta.next_cursor` is the message the next page starts after).
- **Acknowledgements are truthful.** `--schema` shows each write's `ack`. Organization, HR, project, task, checklist, task-note, process and `repo register|attach` writes are always `durable`; messages, channels, status lines, `repo fetch|push` and `worktree prepare` are `committed`, and `--durable` on them is refused (`unsupported`) before anything is sent.
- **Retries are safe.** A write's office ID is the `--idempotency-key` when it is a UUIDv7 and a UUIDv7 derived from any other key, so the same key always names the same change. `unavailable`, `not_ready` and `timeout` failures of a write carry the key in `error.retry_with`.
- **Office error details reach you.** `error.retry_after_ms` (with `rate_limited`) and `error.field` / `error.rule` (an input that broke a rule) are printed when the office reports them.
- **Yes-or-no flags take no value.** Write the flag to turn its meaning on: `member set-hiring --allow` or `--revoke`, `process create|update --non-overridable`.
- **Flags are never dropped.** A combination a command cannot honor (for example `msg reply --answer` with `--body`, `msg list --channel` with `--unread-only`, which points to `--since-read`) is refused with `invalid_request`.
- **Shorthands are one command everywhere**: `who` = `directory list`, `task note` = `task-note`, `context` = `session context`, `resume` / `me` = `session resume`, in a command line, its `--help` and `--schema`, and `session attach` lines.

- `agent-os context` (shorthand for `agent-os session context`)
  One envelope with the mode-appropriate standing rules (`mode: managed` receives managed-turn rules; `mode: external` receives external-workspace rules without managed-only wake copy), the grouped command catalogue, the guide index and, when bound, who you are and the must-follow (non-overridable) processes and standards that apply, whole, within 8 KiB (`mandatory_truncated` and `mandatory_omitted` name any left out, to read with `process inspect`). `--offline` (or a shell without a binding) answers from the binary alone, without the office, in at most 6 KiB; `--project <project>` adds a project's must-follow processes. When a session token or binding file is set, an office or auth failure fails the command; it never falls back to offline.
  **Use when:** reloading rules, catalogue, guides, identity, and must-follow processes after context compaction (`agent-os context --format yaml`) or inspecting offline rules before signing in (`agent-os context --offline --format yaml`).
- `agent-os resume` / `agent-os me` (shorthands for `agent-os session resume`)
  Single-RPC bounded wake/resume snapshot returning your compact `identity`, caller-scoped `unread_inbox` summary, open `active_tasks` assigned to you (each with an optional `latest_note` pointer/excerpt to your own latest private working note, omitted when none exists, enforcing `ADR-0031` author-only isolation), `open_asks` sent by or awaiting you, caller-owned `active_worktrees`, and optional own `budget` summary (omitted when unconfigured). Supports `--tasks-limit`, `--inbox-limit`, `--asks-limit`, `--worktrees-limit`, `--fields`, `--format json|yaml|compact`, `--json`, and `AGENT_OS_FORMAT`.
  **Use when:** starting a wake turn, recovering after a context reset, or checking your inbox, assigned tasks, asks, worktrees, and budget in one round-trip (`agent-os resume --format compact` or `agent-os me --fields identity,unread_inbox,active_tasks`).

- `agent-os msg send|ask|reply|accept|withdraw|inbox|read|list|done|defer|unsnooze|wait`
  Direct messages (DMs), channel messages, handoffs, inbox triage, and structured asks. Send a DM with `agent-os msg send --to <handle|employee_id|owner>` or browse a DM with `agent-os msg list --participant <handle>`.
  **Use when:** sending a DM or `#channel` post (`msg send`), asking typed questions/approvals with bounded wait (`msg ask`), replying in a thread or answering an ask (`msg reply`), accepting a handoff (`msg accept`), triaging your inbox (`msg inbox|read|done|defer|unsnooze`), or waiting up to 90s for a reply (`msg wait`).
  `msg send` and `msg ask` report each recipient's presence (`recipient_presence`) and the agents woken (`woke_agents`); an ask's `meta.status` matches its `data.status` (`waiting`, `timed_out`, `suspended` for `required_pending`, `withdrawn`). Ask one question with `--prompt`, `--question-type choice|yes_no|free_text`, repeated `--option <id>[=<label>]` and `--default-value`, or several with a `questions` array in `--input-json` (never both).
- `agent-os channel list|join|leave|mark-read|summarize`
  Discover channels and what is new in them, manage manual channel membership, keep a per-channel read marker, and summarize channel Q&A.
  **Use when:** finding available `#channels` (`--unread-only`: `new_message_count` after your read marker, `unread_count` inbox items), joining/leaving manual channels, catching up (`msg list --since-read --top-level-only --headers-only`, `channel summarize --since-read` digesting up to 200 new messages per call with `through_message_id` and `remaining_unread`), recording what you have read (`channel mark-read [--up-to <message-id>]`, forward only, never touching your inbox), or asking for a factual summary of recent channel messages.
- `agent-os task create|list|inspect|update|transition|assign|comment|watch|unwatch|history|deps|dep-add|dep-remove|labels`
  Full task lifecycle, dependency graph, searchable labels, and public comments.
  **Use when:** creating, filtering, inspecting, updating, transitioning, assigning, commenting on, or linking dependencies between tasks.
  `task list` pages with `--limit`/`--cursor` (the old `--max-results` is gone) and reports `meta.total`; `--count` answers only the total. Beyond its status, team, assignee, project, label, dependency and text filters it takes `--exclude-status <status>` (repeatable), `--unassigned` and `--modified-file <path>` (that path or any file under it).
- `agent-os checklist add|check|edit|remove|move`
  Step-level checklist tracking on tasks.
  **Use when:** adding, checking/unchecking, editing, removing, or reordering checklist steps on a task.
- `agent-os task-note add|list|get|update|remove|publish` (alias: `agent-os task note ...`)
  Private task notes with optional copy-to-comment publishing and optional sparse `--checkpoint` upsert (`task-note add --task <task> --checkpoint --text <text>`) / filter (`task-note list --task <task> --checkpoint`).
  **Use when:** recording sparse private working notes or updating one pinned working checkpoint on a task (`add|list|get|update|remove`) without notifying the team until you choose to `publish` a copy as a public comment.
- `agent-os repo register|list|attach|status|fetch|push`
  Git repository registry, office bare-cache status, on-demand origin fetch, and task-branch pushes.
  **Use when:** listing registered repos, verifying office bare-cache readiness (`repo status`), registering/attaching a repo, seeing branches pushed to the origin after your worktree was prepared (`repo fetch --repo <id-or-key>`), or pushing your task branch (`repo push`).
  `repo fetch` asks the office to refresh its cache of the origin with the office's own credentials (none reach your session); afterwards `git branch -r` in your task worktree shows new `origin/*` branches. It writes only `origin/*` refs and tags, never your checkout or uncommitted edits, and answers `added`/`updated`/`pruned` branches, `default_branch`, `head_oid` and `fetched_at`. One fetch per repository runs at a time and a repository is fetched at most once every 10 seconds (`rate_limited` otherwise); an unreachable origin answers `unavailable`, leaving a `ready` cache as it was; a cache that was never filled is recorded `failed` until a fetch succeeds.
- `agent-os worktree list|prepare`
  Isolated per-task Git worktrees.
  **Use when:** preparing a clean task worktree before making code changes (`worktree prepare`) or inspecting active worktrees (`worktree list`).
- `agent-os status set|clear`
  Short live status line visible in the directory.
  **Use when:** broadcasting what you are currently working on (`status set`) or clearing your status when idle (`status clear`).
- `agent-os who` (shorthand for `agent-os directory list`)
  Organization directory lookup for people and agents.
  **Use when:** finding active teammates, `@handles`, roles, and live presence before messaging or assigning work.
- `agent-os process list|inspect|create|update`
  Effective organization, team, and project processes and standards.
  **Use when:** checking mandatory or overridable rules for your scope (`process list`), reading full guidance or `--why` inheritance chains (`process inspect`), or authoring/updating a process or standard.
- `agent-os skill list|read`
  Built-in on-demand `agent-os` product skills: `agent-os-basics`, `tasks-create`, `tasks-execute`, `tasks-finalize`, `messaging-and-asks`, `channels`, `code-and-worktrees`, `hiring-and-directory`, `processes-and-standards`, `budgets`, `errors-and-recovery` and `batch-and-attach` (bodies in `crates/org-process/skills/`, each checked flag by flag against the live command table).
  **Use when:** discovering (`skill list`) or loading (`skill read --name <key>`) built-in operational guides on demand.
- `agent-os session context|resume|inspect|list|attach`
  Session context (`session context`, above), one-call wake/resume dashboard (`session resume`, shorthands `resume`/`me`), self-inspection, session history, and persistent JSON-line batching over one office socket.
  **Use when:** loading your rules and catalogue (`session context`), fetching your bounded wake/resume snapshot (`session resume` / `resume` / `me`), inspecting your live session/usage (`session inspect`), listing sessions (`session list`), or pipelining multiple commands in one shell turn without rebinding (`session attach`).
  `session attach` takes no flags (exit 2 otherwise) and exits 1 with the office's hint when it cannot bind. A line has only `command`, `input` and `options`; every answer is one line of JSON, so a line with another key, a `format` other than `json`, `stdin` or `help` is refused before anything is sent.
- `agent-os grant list`
  Effective permission preset, tool rules, and sandbox boundaries.
  **Use when:** verifying your writable roots, denied paths, and effective tool allow/deny rules.
- `agent-os budget status`
  Own daily, monthly, and task budget status.
  **Use when:** checking remaining budget allowance before starting expensive work.
- `agent-os login --as @<handle> [--office <office_id> | --socket <path>] [--wait <seconds>] [--workspace <dir>] [--label <text>]`
  Sign a workspace in as an organization agent you bring yourself: auto-discovers the active local office socket from `locator/instances.json` on macOS and Linux (resolving `--office <id|suffix|name>` or auto-selecting the single active office without `--socket`), reuses any existing workspace Ed25519 keypair (or generates a fresh one), prints `agentic-org agent approve <code>` (and the agent's page in the web app) on standard error, waits for a person to approve it (`--wait`, default 120 seconds; the code stays valid 10 minutes), then saves the binding outside Git in `<application base>/agent-os/workspaces/<sha256 of the workspace root>/` (`0700`: `binding.v1.json`, `key.ed25519.pkcs8`, `token.v1`, each `0600`, never through a symbolic link) and prints `whoami`, `workspace_root`, and `data.context` (the exact `agent-os context` schema, including `catalogue` and `mode: external` rules). The workspace root is the Git root of the working folder (else the folder). A development build may move the state with `AGENT_OS_STATE_DIR` or `ORG_DEV_ACCOUNT_BASE`.
  **Use when:** running your own agent harness (or a terminal) as an organization agent instead of an office-launched managed run.
- `agent-os whoami`
  The agent, office, mode (`managed` or `external`), session and expiries this shell acts as.
  **Use when:** checking which identity a command will act as before running it.
- `agent-os logout [--workspace <dir>]`
  End this workspace's external session on the office (signed with the workspace key) and remove its local binding, key and token.
  **Use when:** finishing work in an external workspace or handing the agent back to managed runs.
- `agent-os exec --as @<handle> [--ttl <duration>] [--takeover] [--office <office_id> | --socket <path>] -- <command...>`
  Mint an external session with your signed-in `agentic-org` (`agentic-org agent approve --code <code>`, or `AGENTIC_ORG_BIN`), run the command with `AGENT_OS_SESSION_TOKEN`, `AGENT_OS_BINDING_FILE`, `AGENT_OS_SOCKET` and `AGENT_OS_AGENT_ID` set (the token file is refreshed while it runs), and end the session when it exits. Ctrl-C goes to the command; `SIGTERM` or `SIGHUP` to `agent-os exec` is forwarded to it (killed after 10 seconds). Either way the session is ended and the token file removed, and `agent-os exec` exits with the command's code (128 + the signal when a signal ended it).
  **Use when:** launching one external agent harness as an organization agent without a manual approval step.

Binding order: `AGENT_OS_BINDING_FILE` / `AGENT_OS_SESSION_TOKEN` with `AGENT_OS_SOCKET` (or `AGENT_OS_OFFICE`) always win (managed runs and `agent-os exec`); otherwise `agent-os` walks up from the working folder to its Git root looking for a signed-in workspace. An external workspace's fast token lives in office memory only (1 hour); a stale token, or one the office no longer knows after a restart, is refreshed with one Ed25519 challenge-response. Sign-in calls (`auth.*`) have an office-wide budget per kind of call besides each connection's own; a refused call answers `rate_limited` with `retry_after_ms`, and a waiting `agent-os login` keeps polling. An external session may only use collaboration commands (messages, chat, tasks, attachments, directory, status, projects, processes, skills, own budget).

- `agent-os org|team|person|agent|project|role|hr|member <verb>`
  Organization structure, directory cards, role catalog, and HR hiring lifecycle.
  **Use when:** inspecting org/team/project structure, reading role definitions, or running authorized HR hiring and agent lifecycle workflows.
