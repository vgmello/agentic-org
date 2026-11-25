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
- `agentic-org setup --non-interactive --config <file> [--name <instance>]`
  Non-interactive setup from a strict JSON file (cannot be combined with `--port` or `--federation-*`; set those in the file).
- `agentic-org serve [--name <instance>]` / `agentic-org serve --setup <file> [--name <instance>]`
  Run the installed service host in the foreground (or initialize and serve a foreground config file).
- `agentic-org service start|stop|status [--name <instance>]`
  Start, stop, or inspect the instance's registered OS service.

### Sign-In, Context & Administration

- `agentic-org sign-in [--name <instance>] [--office <office_id>] [-- <command...>]`
  Start a browser-approved terminal session (`SIGN-40` / `SES-8`) without a terminal password prompt.
- `agentic-org context show|use|clear`
  Manage the saved non-secret default instance and office selection (`--office <office_id> [--name <instance>]`).
- `agentic-org admin reset [--name <instance>] [--user <user>]`
  Issue a one-time administrator registration or password-reset link after local OS confirmation.
- `agentic-org admin tls set --cert <file> --key <file> [--name <instance>]`
  Install or replace the instance's browser HTTPS certificate pair.

### Organizations (`org`)

- `agentic-org org create <name> --slug <slug> --repo <https://...|ssh://...> [--branch <state_branch>] [--login <login>] [--name <instance>] [--no-wait] [--json]`
  Create a new organization and its HQ office on this host (default `--branch main`). Waits for completion unless `--no-wait` is passed.
- `agentic-org org list [--login <login>] [--name <instance>] [--json]`
  List organizations visible to the account and any creates in flight.

### Offices & Multi-Office Federation (`office`)

- `agentic-org office list [--org <org_id>] [--login <login>] [--name <instance>] [--json]`
  List offices visible to the account (plus local satellite offices for the instance administrator on `host.sock`).
- `agentic-org office restart <office_id> [--login <login>] [--name <instance>] [--json]`
  Restart an offline or unavailable office (instance administrator or organization owner).
- `agentic-org office invite <office_name> --address <host:port> [--address <host:port>...] [--org <org_id>] [--ttl <secs>] [--re-enroll-office <office_id>] [--login <login>] [--name <instance>] [--json]`
  Create a one-time satellite invitation token on HQ (owner-only; requires HQ federation listener).
- `agentic-org office invitations [--org <org_id>] [--login <login>] [--name <instance>] [--json]`
  List satellite invitations on HQ with status and expiry (check codes are shown only on the satellite).
- `agentic-org office approve <invitation_id> --code <check_code> [--org <org_id>] [--login <login>] [--name <instance>] [--json]`
  Approve a satellite enrollment on HQ using the satellite's out-of-band check code and issue its Org-CA mTLS certificate.
- `agentic-org office decline <invitation_id> [--org <org_id>] [--login <login>] [--name <instance>] [--json]`
  Decline a satellite invitation on HQ.
- `agentic-org office cancel <invitation_id> [--org <org_id>] [--login <login>] [--name <instance>] [--json]`
  Cancel an active satellite invitation on HQ.
- `agentic-org office remove <office_id> [--org <org_id>] [--login <login>] [--name <instance>] [--json]`
  Remove a satellite office on HQ and revoke its issued Org-CA certificates.
- `agentic-org office sync-status [--org <org_id>] [--office <office_id>] [--login <login>] [--name <instance>] [--json]`
  Show live federation and replication sync status for an HQ or satellite office.
- `agentic-org office join (--invitation-file <path> | --invitation-stdin) --yes [--login <login>] [--name <instance>] [--json]`
  Join this host as a satellite office using an HQ-issued invitation token.
- `agentic-org office remove-pending <office_id> [--login <login>] [--name <instance>] [--json]`
  Cancel and remove a pending satellite join before approval completes.
- `agentic-org office re-enroll <office_id> (--invitation-file <path> | --invitation-stdin) --yes [--login <login>] [--name <instance>] [--json]`
  Re-enroll an existing satellite office onto a replacement HQ after Git import (D25).

### Projects, Repositories, Worktrees & Tasks

- `agentic-org project list|inspect|create`
  List, inspect, or create projects (`agentic-org project --help`).
- `agentic-org repo register|list|attach|status|push`
  Register and manage organization/project Git repositories and push task branches.
- `agentic-org worktree list|prepare`
  List or prepare isolated task Git worktrees.
- `agentic-org task create|list|inspect|update|transition|assign|comment|watch|deps|labels`
  Create, inspect, and manage tasks over an approved office channel.
- `agentic-org checklist add|check|edit|remove|move`
  Manage task checklist items over an approved office channel.
