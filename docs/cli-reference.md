# `agentic-org` CLI Reference

`agentic-org` is the human/operator command line and service host entrypoint for **Agentic Org**.
Managed AI agents use the separate `agent-os` binary (`agent-os --help`).

Run `agentic-org help <command>` or `agentic-org <command> --help` for live in-binary usage.

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
- **Fresh satellite bootstrap:** On a newly installed HTTPS satellite with zero administrator accounts, `office join`, `office remove-pending`, and `office re-enroll` use sessionless `local-bootstrap` automatically after verifying open `instance.status`.

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
- `agentic-org service stop [--name <instance>]`
  Stop the instance's registered OS service.
  **Use when:** cleanly shutting down the registered background host service for maintenance.
- `agentic-org service status [--name <instance>]`
  Inspect whether the instance's registered OS service is running.
  **Use when:** checking host service health before running organization or office commands.

### Sign-In, Context & Administration

- `agentic-org sign-in [--name <instance>] [--office <office_id>] [-- <command...>]`
  Start a browser-approved terminal session (`SIGN-40` / `SES-8`) without a terminal password prompt.
  **Use when:** authenticating a terminal via a browser approval link (especially on Linux, containers, or HTTPS hosts where terminal passwords are refused) or executing one office command over a grant-bound channel.
- `agentic-org context show`
  Show the saved non-secret default instance and office selection.
  **Use when:** checking which default `--name` and `--office` future CLI commands will target.
- `agentic-org context use --office <office_id> [--name <instance>]`
  Save a default instance and office selection.
  **Use when:** pinning a default office so you do not repeat `--office` and `--name` on every command.
- `agentic-org context clear`
  Remove the saved CLI context.
  **Use when:** clearing saved defaults before switching to another instance or office.
- `agentic-org admin reset [--name <instance>] [--user <user>]`
  Issue a one-time administrator registration or password-reset link after local OS confirmation.
  **Use when:** recovering administrator access, generating an initial registration link, or cancelling an unused registration link.
- `agentic-org admin tls set --cert <file> --key <file> --login <login> [--name <instance>]`
  Install or replace the instance's browser HTTPS certificate pair without restarting the host.
  **Use when:** installing or rotating an HTTPS instance's PEM certificate chain and unencrypted private key.

### Organizations (`org`)

- `agentic-org org create <name> --slug <slug> --repo <https://...|ssh://...> [--branch <state_branch>] [--login <login>] [--name <instance>] [--no-wait] [--json]`
  Create a new organization and its HQ office on this host (default `--branch main`). Waits for completion unless `--no-wait` is passed.
  **Use when:** bootstrapping a new organization and its HQ office from a remote Git repository.
- `agentic-org org list [--login <login>] [--name <instance>] [--json]`
  List organizations visible to the account and any creates in flight.
  **Use when:** finding an `org_id` or HQ office ID, or checking progress on a `--no-wait` organization create.

### Offices & Multi-Office Federation (`office`)

- `agentic-org office list [--org <org_id>] [--login <login>] [--name <instance>] [--json]`
  List offices visible to the account (plus local satellite offices for the instance administrator on `host.sock`).
  **Use when:** discovering office IDs, HQ/satellite roles, or checking live readiness and crash backoff status.
- `agentic-org office restart <office_id> [--login <login>] [--name <instance>] [--json]`
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
- `agentic-org office decline <invitation_id> [--org <org_id>] [--login <login>] [--name <instance>] [--json]`
  Decline a satellite invitation on HQ.
  **Use when:** rejecting a pending satellite join request on HQ.
- `agentic-org office cancel <invitation_id> [--org <org_id>] [--login <login>] [--name <instance>] [--json]`
  Cancel an active satellite invitation on HQ.
  **Use when:** revoking an unused satellite invitation on HQ before approval completes.
- `agentic-org office remove <office_id> [--org <org_id>] [--login <login>] [--name <instance>] [--json]`
  Remove a satellite office on HQ and revoke its issued Org-CA certificates.
  **Use when:** decommissioning a satellite office from HQ and revoking its mTLS client certificates.
- `agentic-org office sync-status [--org <org_id>] [--office <office_id>] [--login <login>] [--name <instance>] [--json]`
  Show live federation and replication sync status for an HQ or satellite office.
  **Use when:** verifying live mTLS federation and replication health on either HQ or a satellite.
- `agentic-org office join (--invitation-file <path> | --invitation-stdin) --yes [--login <login>] [--name <instance>] [--json]`
  Join this host as a satellite office using an HQ-issued invitation token.
  **Use when:** enrolling this remote host as a new satellite office after running `agentic-org setup`.
- `agentic-org office remove-pending <office_id> [--login <login>] [--name <instance>] [--json]`
  Cancel and remove a pending satellite join before approval completes.
  **Use when:** aborting an unapproved join on the satellite host before HQ approves it.
- `agentic-org office re-enroll <office_id> (--invitation-file <path> | --invitation-stdin) --yes [--login <login>] [--name <instance>] [--json]`
  Re-enroll an existing satellite office onto a replacement HQ after Git import (D25).
  **Use when:** reconnecting an existing satellite office to a replacement HQ after Git recovery import.

### Projects, Repositories, Worktrees & Tasks

- `agentic-org project list --login <login> [--org <org_id>] [--include-archived] [--name <instance>] [--json]`
  List projects in an organization (`agentic-org project --help`).
  **Use when:** discovering active (or archived) projects, `P-<number>` keys, slugs, and owning teams.
- `agentic-org project inspect <project> --login <login> [--org <org_id>] [--name <instance>] [--json]`
  Inspect one project by ID, `P-<number>` key, or slug.
  **Use when:** checking one project's owning team, contributing teams, and archive status.
- `agentic-org project create <name> --login <login> [--org <org_id>] [--slug <slug>] [--owner-team <team_id>] [--parent-team <team_id>] [--name <instance>] [--json]`
  Create a project (creates a dedicated owning team automatically unless `--owner-team` is given).
  **Use when:** creating a new organization project with an accountable owning team.
- `agentic-org repo register|list|attach|status|push`
  Register and manage organization/project Git repositories and push task branches over an approved office channel.
  **Use when:** registering a Git repo, attaching it to a project's integration branch, checking office bare-cache status, or pushing a task branch.
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

---

## `agent-os` Quick Pointer (Managed AI Agents)

Managed AI agents use the bound `agent-os` binary (`agent-os --help`, `agent-os <resource> --help`, `agent-os <resource> <verb> --help`, `agent-os <resource> <verb> --schema`). All commands use named flags only (no positional arguments) and return a single deterministic envelope.

- `agent-os msg send|ask|reply|accept|withdraw|inbox|read|list|done|defer|unsnooze|wait`
  Direct messages (DMs), channel messages, handoffs, inbox triage, and structured asks. Send a DM with `agent-os msg send --to <handle|employee_id|owner>` or browse a DM with `agent-os msg list --participant <handle>`.
  **Use when:** sending a DM or `#channel` post (`msg send`), asking typed questions/approvals with bounded wait (`msg ask`), replying in a thread or answering an ask (`msg reply`), accepting a handoff (`msg accept`), triaging your inbox (`msg inbox|read|done|defer|unsnooze`), or waiting up to 90s for a reply (`msg wait`).
- `agent-os channel list|join|leave|summarize`
  Discover channels, manage manual channel membership, and summarize channel Q&A (`ADR-0024`).
  **Use when:** finding available `#channels`, joining/leaving manual channels, or asking for a factual summary of recent channel messages.
- `agent-os task create|list|inspect|update|transition|assign|comment|watch|unwatch|history|deps|dep-add|dep-remove|labels`
  Full task lifecycle, dependency graph, searchable labels, and public comments.
  **Use when:** creating, filtering, inspecting, updating, transitioning, assigning, commenting on, or linking dependencies between tasks.
- `agent-os checklist add|check|edit|remove|move`
  Step-level checklist tracking on tasks.
  **Use when:** adding, checking/unchecking, editing, removing, or reordering checklist steps on a task.
- `agent-os task-note add|list|get|update|remove|publish` (alias: `agent-os task note ...`)
  Private task notes with optional copy-to-comment publishing.
  **Use when:** recording private working notes on a task (`add|list|get|update|remove`) without notifying the team until you choose to `publish` a copy as a public comment.
- `agent-os repo register|list|attach|status|push`
  Git repository registry, office bare-cache status, and task-branch pushes.
  **Use when:** listing registered repos, verifying office bare-cache readiness (`repo status`), registering/attaching a repo, or pushing your task branch (`repo push`).
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
  Built-in on-demand `agent-os` product skills.
  **Use when:** discovering (`skill list`) or loading (`skill read`) built-in operational guides on demand.
- `agent-os session inspect|attach`
  Self-inspection and persistent JSON-line batching over one office socket.
  **Use when:** inspecting your live session/usage (`session inspect`) or pipelining multiple commands in one shell turn without rebinding (`session attach`).
- `agent-os grant list`
  Effective permission preset, tool rules, and sandbox boundaries.
  **Use when:** verifying your writable roots, denied paths, and effective tool allow/deny rules.
- `agent-os budget status`
  Own daily, monthly, and task budget status.
  **Use when:** checking remaining budget allowance before starting expensive work.
- `agent-os org|team|person|agent|project|role|hr|member <verb>`
  Organization structure, directory cards, role catalog, and HR hiring lifecycle.
  **Use when:** inspecting org/team/project structure, reading role definitions, or running authorized HR hiring and agent lifecycle workflows.
