# Remote Satellite Office Setup (Linux over Tailscale)

This runbook documents the exact, reproducible commands to run a local HQ (macOS or Linux) with one or more remote Linux satellite offices (for example `worker-eu` and `main-eu`) connected over Tailscale mTLS, using prebuilt signed release packages with **no build toolchain required on remote hosts**.

For full flag details on every command, see [CLI Reference](cli-reference.md).

---

## Architecture Overview

```
Local HQ (e.g. hq-host — 100.64.0.10)
  ├── Browser UI / API:  http://127.0.0.1:7420
  ├── Federation mTLS:   0.0.0.0:7443 (SANs: 100.64.0.10, hq-host.tailnet.ts.net)
  └── Hosts local agents (e.g. @backend, @ui, @pm, @poc)

Remote Satellite 1 (worker-eu — 100.64.0.21, Ubuntu x86_64)
  ├── systemd --user service running signed release store
  └── Hosts managed remote agents (e.g. @worker-eu-eng via claude / codex / agy)

Remote Satellite 2 (main-eu — 100.64.0.22, Ubuntu x86_64, no build toolchain)
  ├── systemd --user service running identical prebuilt signed release package
  └── Hosts managed remote agents (e.g. @main-eu-eng via claude / codex / agy)
```

---

## Prerequisites

### On every remote Linux satellite host
- Ubuntu 24.04 / 26.04 (or modern systemd GNU/Linux, `x86_64` or `aarch64`), non-root service user (e.g. `agents`)
- Tailscale connected to the same tailnet as HQ (`tailscale status`)
- `systemd --user` active with lingering enabled (so offices survive SSH logout and reboots):
  ```bash
  loginctl enable-linger "$USER"
  systemctl --user is-system-running
  ```
- Base tools: `curl` (or `wget`), `tar`, `sha256sum`, and OpenSSL 3+ (if using `--pubkey`; no Python or compiler required to install)
- AI agent CLI runtime(s) installed and signed in under the service user (e.g. `claude --version`, `codex --version`, or `agy --version`)
- **Required on Linux for Claude Code sandboxing and subprocess env scrubbing (`CLAUDE_CODE_SUBPROCESS_ENV_SCRUB=1`):**
  ```bash
  sudo apt-get update && sudo apt-get install -y bubblewrap socat
  ```
- **Required on Ubuntu 24.04 / 26.04 (scoped AppArmor profile for `bwrap`):** Ubuntu 24.04+ restricts unprivileged user namespaces host-wide (`kernel.apparmor_restrict_unprivileged_userns = 1`), and Ubuntu's stock `bwrap-userns-restrict` profile strips capabilities inside child processes (`unpriv_bwrap`), blocking Claude Code's `apply-seccomp` helper (`nested userns is capability-restricted`).
  - **Never disable `kernel.apparmor_restrict_unprivileged_userns` globally.** Keeping the sysctl enabled protects every other unprivileged process on the host from kernel user-namespace attack surface.
  - Instead, install the minimal scoped AppArmor profile restricted exclusively to `/{usr/,}bin/bwrap` (`/etc/apparmor.d/agentic-org-bwrap`, canonical template in [`packaging/apparmor/agentic-org-bwrap`](../packaging/apparmor/agentic-org-bwrap)):
    ```bash
    # Recommended: installs /etc/apparmor.d/agentic-org-bwrap (0644), disables
    # Ubuntu's conflicting bwrap-userns-restrict profile if present, and loads
    # the scoped profile via apparmor_parser -r:
    sudo bash scripts/install.sh --install-bwrap-apparmor

    # Verify with agentic-org doctor:
    agentic-org doctor
    ```
  - To inspect or install the profile manually (for example via configuration management), run `bash scripts/install.sh --print-bwrap-apparmor` or install the canonical file directly:
    ```bash
    bash scripts/install.sh --print-bwrap-apparmor \
      | sudo tee /etc/apparmor.d/agentic-org-bwrap >/dev/null
    sudo chmod 0644 /etc/apparmor.d/agentic-org-bwrap
    if [ -f /etc/apparmor.d/bwrap-userns-restrict ]; then
      sudo mkdir -p /etc/apparmor.d/disable
      sudo ln -sf /etc/apparmor.d/bwrap-userns-restrict /etc/apparmor.d/disable/bwrap-userns-restrict
      sudo apparmor_parser -R /etc/apparmor.d/bwrap-userns-restrict || true
    fi
    sudo apparmor_parser -r /etc/apparmor.d/agentic-org-bwrap
    ```
- **Not required on remote hosts:** any build toolchain or compiler.

---

## Step 1: Set Up Local HQ with Tailscale mTLS Federation

### 1a. Find HQ's Tailscale IP and MagicDNS name
```bash
# macOS
/Applications/Tailscale.app/Contents/MacOS/Tailscale ip -4
/Applications/Tailscale.app/Contents/MacOS/Tailscale status --json | python3 -c 'import sys,json; print(json.load(sys.stdin)["Self"]["DNSName"].rstrip("."))'

# Linux
tailscale ip -4
tailscale status --json | python3 -c 'import sys,json; print(json.load(sys.stdin)["Self"]["DNSName"].rstrip("."))'
```

### 1b. Initialize and start HQ
For a signed release build with OS service registration (`launchd` / `systemd --user`):
```bash
agentic-org setup \
  --federation-bind 0.0.0.0:7443 \
  --federation-advertise 100.64.0.10,hq-host.tailnet.ts.net
agentic-org service status
```

For a macOS local development build (or when `~/Library/LaunchAgents` is not writable), run in foreground mode via a strict setup file:
```bash
mkdir -p "$HOME/.config/agentic-org" "$HOME/.local/state/agentic-org-dev-keys/default" \
  "$HOME/Library/Application Support/agentic.org/instances/default"
chmod 0700 "$HOME/.local/state/agentic-org-dev-keys/default" \
  "$HOME/Library/Application Support/agentic.org/instances/default"

cat > "$HOME/.config/agentic-org/hq-setup.json" <<EOF
{
  "version": 1,
  "service": "foreground",
  "root": "${HOME}/Library/Application Support/agentic.org/instances/default",
  "listen": {
    "address": "127.0.0.1",
    "port": 7420,
    "scheme": "http_loopback",
    "origin": "http://127.0.0.1:7420",
    "hsts": "off"
  },
  "federation": {
    "bind": "0.0.0.0:7443",
    "advertise": ["100.64.0.10", "hq-host.tailnet.ts.net"]
  }
}
EOF

ORG_OFFICE_DEV_KEY_DIR="$HOME/.local/state/agentic-org-dev-keys/default" \
  nohup agentic-org serve --setup "$HOME/.config/agentic-org/hq-setup.json" \
  >"$HOME/.local/state/agentic-org/hq-serve.log" 2>&1 </dev/null &
```

### 1c. Create the organization and save default CLI context
```bash
agentic-org org create "Agentic Org" \
  --slug agentic-org \
  --repo https://github.com/vgmello/agentic-org.git \
  --branch v1 \
  --json

# Save default context using the returned HQ office ID
HQ_OFFICE_ID="<hq_office_id_from_above>"
agentic-org context use --office "$HQ_OFFICE_ID"
agentic-org office list
```

Verify HQ is listening on both HTTP (`127.0.0.1:7420`) and mTLS federation (`*:7443`):
```bash
curl -sS -o /dev/null -w "HQ HTTP %{http_code}\n" http://127.0.0.1:7420/
lsof -iTCP:7420 -iTCP:7443 -sTCP:LISTEN
```

---

## Step 2: Install Prebuilt Binaries on Remote Linux Satellites

Run on each remote Linux host (`worker-eu`, `main-eu`, etc.) — **zero local compilation**.

### Option A: Install or upgrade from GitHub Releases
```bash
# Cold install (resolves the latest release automatically when --version is omitted;
# requires out-of-band verification via --sha256 <64-hex|file> or --pubkey <publisher.pub.pem|trust-registry.json>):
curl -fsSL https://raw.githubusercontent.com/vgmello/agentic-org/v1/scripts/install.sh \
  | bash -s -- --sha256 "<expected-archive-sha256>" --bin-dir "$HOME/.local/bin"

# Upgrade an already-installed host to the latest release (reuses the trusted installed binary):
curl -fsSL https://raw.githubusercontent.com/vgmello/agentic-org/v1/scripts/install.sh | bash
```

### Option B: Install from a locally staged `.tar.gz` + `.sha256` bundle
Copy the prebuilt bundle and `scripts/install.sh` to the remote host (e.g. via `scp` over Tailscale):
```bash
scp dist/agentic-org-0.1.0-v5-linux-x86_64.tar.gz \
    dist/agentic-org-0.1.0-v5-linux-x86_64.tar.gz.sha256 \
    scripts/install.sh \
    agents@apps-main-eu:~/dist/
```

Then install on the remote host (cold installs require explicit `--sha256` or `--pubkey`; subsequent upgrades reuse the installed trusted binary):
```bash
ssh agents@apps-main-eu '
  export PATH="$HOME/.local/bin:$PATH"
  bash "$HOME/dist/install.sh" \
    --archive "$HOME/dist/agentic-org-0.1.0-v5-linux-x86_64.tar.gz" \
    --sha256 "$HOME/dist/agentic-org-0.1.0-v5-linux-x86_64.tar.gz.sha256" \
    --bin-dir "$HOME/.local/bin"
  agentic-org release status --json
  agentic-org --version
  agent-os --version
'
```

What `install.sh` does automatically:
- Verifies the archive SHA-256 against the `.sha256` checksum file.
- Tightens Ubuntu User-Private-Group directory permissions (`chmod go-w ~/.local ~/.local/share` and `0700 ~/.local/share/agentic-org`) required for the protected release store.
- Runs `agentic-org release install` to verify the Ed25519 manifest signature, closed inventory, and executable digests, pinning the release atomically into `~/.local/share/agentic-org/release-store`.
- Moves each instance's registered `systemd --user` / `launchd` service to the new release: a running service is stopped for the install and started again on the new release (`--no-restart` leaves it stopped), a stopped one stays stopped, and a unit edited by hand is left untouched with the steps to upgrade it.
- Symlinks `agentic-org` and `agent-os` into `~/.local/bin`.
- On Linux hosts where `kernel.apparmor_restrict_unprivileged_userns = 1` and `bwrap` is present without an active scoped profile, installs `/etc/apparmor.d/agentic-org-bwrap` automatically when run as root or prints the scoped `sudo` command when run unprivileged (without failing the release install or relaxing the sysctl globally).

### Initialize the remote host's `systemd --user` service
```bash
ssh agents@apps-main-eu '
  export PATH="$HOME/.local/bin:$PATH"
  agentic-org setup --port 7420 </dev/null
  agentic-org service status
  curl -sS -o /dev/null -w "HTTP %{http_code}\n" http://127.0.0.1:7420/
'
```

Expected output: `Service registered.`, `Service running.`, `Instance ready.`, `HTTP 200`.

---

## Step 3: Enroll Remote Satellites into HQ (mTLS + Out-of-Band Check Code)

Repeat for each satellite (`apps-worker-eu`, `apps-main-eu`, etc.).

### 3a. Issue a one-time invitation on HQ
```bash
# On HQ (replace 100.64.0.10:7443 with your HQ Tailscale IP:port)
agentic-org office invite "worker-eu" --address 100.64.0.10:7443 --json > /tmp/inv-worker-eu.json
```

### 3b. Start the join on the satellite host (piping the token over stdin)
```bash
TOKEN="$(python3 -c 'import json; print(json.load(open("/tmp/inv-worker-eu.json"))["token"])')"
printf '%s' "$TOKEN" | ssh agents@apps-worker-eu '
  export PATH="$HOME/.local/bin:$PATH"
  agentic-org office join --invitation-stdin --yes --json
'
```

The satellite connects to HQ over Tailscale mTLS and prints its out-of-band verification `check_code` (for example `EVZW-ZAJJ`).

### 3c. Approve the join on HQ with the check code
```bash
INV_ID="$(python3 -c 'import json; print(json.load(open("/tmp/inv-worker-eu.json"))["invitation"]["invitation_id"])')"
agentic-org office approve "$INV_ID" --code "EVZW-ZAJJ" --json
rm -f /tmp/inv-worker-eu.json
```

### 3d. Verify all offices appear live in HQ CLI, Browser UI, and every satellite
```bash
# On HQ: lists HQ + all enrolled remote satellites
agentic-org office list
agentic-org office sync-status --json

# Verify the local Browser UI API returns all offices
curl -sS "http://127.0.0.1:7420/api/v1/organizations/<org_id>/offices" | python3 -m json.tool

# On each satellite: save office context and verify sync convergence
ssh agents@apps-worker-eu '
  export PATH="$HOME/.local/bin:$PATH"
  agentic-org office list
  agentic-org office sync-status --json
'
```

All hosts should report matching `epoch` and `seq`, `initial_snapshot_required: false`, and `status: active`.

---

## Step 4: Configure AI Runtimes on HQ and Satellites

### 4a. Register an org-wide provider connection on HQ
```bash
agentic-org provider create "Claude Code" \
  --key claude-sub \
  --provider anthropic \
  --runtime claude \
  --auth-mode subscription_cli \
  --subscription-id sub:claude-team \
  --max-turns 4 \
  --json
```

### 4b. Bind and refresh the local runtime on HQ and on each headless satellite
Headless satellite administrators can run `provider list`, `provider bind`, and `provider refresh` directly over SSH without browser sign-in:
```bash
CONN_ID="<connection_id_from_step_4a>"

# On HQ (or via ssh on each remote Linux satellite):
CLAUDE_BIN="$(which claude)"
CLAUDE_VER="$(claude --version | awk '{print $1}')"

agentic-org provider bind "$CONN_ID" \
  --auth-ref local-host-oauth \
  --binary "$CLAUDE_BIN" \
  --account-ref sub:claude-team \
  --json

agentic-org provider refresh "$CONN_ID" \
  --driver-version "$CLAUDE_VER" \
  --model "sonnet,opus,haiku,claude-sonnet-4-6,claude-opus-4-6,claude-haiku-4-5" \
  --account-ref sub:claude-team \
  --ttl 300 \
  --json

agentic-org provider bindings
```

Once a local binding is marked `ready`, the office supervisor's background readiness probe automatically re-verifies the runtime binary every 60s and renews the 300s observation window before expiry.

---

## Step 5: Hire Agents Across Offices & Run Cross-Office Messaging

### 5a. Hire managed agents placed on remote satellites (from HQ)
```bash
TEAM_ID="$(agentic-org team list | python3 -c 'import sys,json; print(json.load(sys.stdin)["data"][0]["id"])')"
MODEL_PROFILE_ID="$(agentic-org provider models --json | python3 -c 'import sys,json; print(json.load(sys.stdin)["profiles"][0]["id"])')"
WORKER_OFFICE_ID="<apps-worker-eu office_id>"
MAIN_OFFICE_ID="<apps-main-eu office_id>"

agentic-org hr hire \
  --team "$TEAM_ID" \
  --name "Worker EU Engineer" \
  --handle "worker-eu-eng" \
  --title "Remote Systems Engineer (worker-eu)" \
  --role-text "Remote Linux engineer on apps-worker-eu" \
  --reason "Execute remote engineering tasks on apps-worker-eu" \
  --office-id "$WORKER_OFFICE_ID" \
  --model-profile-id "$MODEL_PROFILE_ID" \
  --model-id "sonnet" \
  --permission-preset "autonomous_sandboxed" \
  --system-prompt-markdown "You are Worker EU Engineer on apps-worker-eu. Reply concisely with agent-os msg reply or agent-os msg send."

agentic-org hr hire \
  --team "$TEAM_ID" \
  --name "Main EU Engineer" \
  --handle "main-eu-eng" \
  --title "Remote Systems Engineer (main-eu)" \
  --role-text "Remote Linux engineer on apps-main-eu" \
  --reason "Execute remote engineering tasks on apps-main-eu" \
  --office-id "$MAIN_OFFICE_ID" \
  --model-profile-id "$MODEL_PROFILE_ID" \
  --model-id "sonnet" \
  --permission-preset "autonomous_sandboxed" \
  --system-prompt-markdown "You are Main EU Engineer on apps-main-eu. Reply concisely with agent-os msg reply or agent-os msg send."
```

You can also inspect or update any existing agent's model profile, model ID, permission preset, or standing system prompt at any time:
```bash
agentic-org agent execution-set \
  --member "<employee_id>" \
  --model-profile-id "$MODEL_PROFILE_ID" \
  --model-id "sonnet" \
  --permission-preset "autonomous_sandboxed" \
  --system-prompt-markdown "You are a remote systems engineer."
agentic-org agent execution-get --member "<employee_id>"
```

### 5b. Send cross-office messages and tasks
```bash
# HQ -> remote satellite agent
agentic-org msg send --to worker-eu-eng --body "Report hostname and check repo status."

# Inspect inbox replies
agentic-org msg inbox
agentic-org msg list --participant worker-eu-eng

# Create a shared task visible across HQ and both EU satellites
agentic-org task create --project-id "<project_id>" --title "Verify remote CI checks" --body "Run checks on apps-worker-eu" --assignee-id "<employee_id>"
```

---

## Step 6: Rolling Upgrades (Zero Re-Enrollment)

Upgrading a running Linux satellite preserves its enrolled mTLS identity, SQLite database, and hired agents. The installer stops the running service for the install, points its unit's `ExecStart` at the new release and starts it again:
```bash
ssh agents@apps-main-eu '
  set -euo pipefail
  export PATH="$HOME/.local/bin:$PATH"
  bash "$HOME/dist/install.sh" \
    --archive "$HOME/dist/agentic-org-<new-release-id>-linux-x86_64.tar.gz" \
    --bin-dir "$HOME/.local/bin"
  agentic-org release status --json
  agentic-org service status
  agentic-org office sync-status --json
'
```

Upgrading from a release whose installed `agentic-org` predates this (its `release install --help` has no `--no-restart`): that CLI cannot install while the service runs, so run `agentic-org service stop --yes` first. `install.sh` then has the newly installed release move the stopped unit to itself, and `agentic-org service start` starts it. Never edit the unit's `ExecStart` by hand: a changed unit is left untouched by every later upgrade.

---

## Troubleshooting

- **`release verification: UnsafeInput` on Ubuntu/Debian:** Ubuntu User Private Groups use `umask 0002`, creating `~/.local` and `~/.local/share` as `0775` (group-writable). Run `chmod go-w ~/.local ~/.local/share` and `chmod 0700 ~/.local/share/agentic-org` (`scripts/install.sh` handles this automatically).
- **`release verification: Busy` on `agentic-org release status` or `release install`:** A running `agentic-org serve` holds `store.lock` exclusively. `release install` stops the services `setup` registered by itself; one it left running (a unit edited by hand, named in the error) is stopped with `agentic-org service stop --yes` before installing again.
- **`service conflict: ... is not a definition this adapter wrote`:** The unit was edited by hand, so `setup` and `release install` never overwrite it. Undo the edit (or remove the unit after `agentic-org service stop --yes`) and run `agentic-org setup` to register the current release again.
- **Claude Code exits before `initialize` on Linux (`bubblewrap is required for subprocess env scrubbing`):** Install `sudo apt-get install -y bubblewrap socat` on the Linux satellite so Claude Code can enforce `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB=1` and OS-level sandboxing (plus `socat` for proxying `AGENT_OS_SOCKET` into the sandbox).
- **`agentic-org doctor` warns on `platform.apparmor_userns` or Claude Code reports `nested userns is capability-restricted` on Ubuntu 24.04+:** Install the minimal scoped `bwrap` AppArmor profile with `sudo bash scripts/install.sh --install-bwrap-apparmor` (writes `/etc/apparmor.d/agentic-org-bwrap` and disables conflicting `bwrap-userns-restrict` while keeping `kernel.apparmor_restrict_unprivileged_userns` enabled host-wide).
- **macOS `cannot write ~/Library/LaunchAgents/...: Permission denied`:** Fix ownership (`sudo chown "$USER":staff ~/Library/LaunchAgents`) or run HQ in foreground mode via `agentic-org serve --setup <config.json>`.
- **Satellite cannot reach HQ port `7443`:** Verify HQ is listening (`lsof -iTCP:7443 -sTCP:LISTEN`) and reachable over Tailscale (`nc -vz 100.64.0.10 7443`). Remember HQ starts its mTLS listener after the first organization is created.

---

## Appendix: Building a Signed Linux Release Package from Source

If you are building your own private signed release bundle (instead of downloading from GitHub Releases), run once on a Linux build host (such as `apps-worker-eu` or CI):

```bash
# 1. Generate a publisher Ed25519 keypair + trust registry (once)
python3 scripts/release_tool.py keygen --out-dir "$HOME/.config/agentic-org/publisher"

# 2. Build static UI (or supply a prebuilt --ui-dir) and compile/sign/package (.tar.gz + .sha256; --deb is for --profile dev only)
./scripts/package-release.sh \
  --profile release \
  --release-id 0.1.0-v1 \
  --sequence 1 \
  --signing-key "$HOME/.config/agentic-org/publisher/publisher.key.pem" \
  --trust-registry "$HOME/.config/agentic-org/publisher/release-trust-registry.json" \
  --ui-dir /path/to/apps/agentic-org-ui/build \
  --out-dir "$HOME/dist"
```

Ship the resulting `dist/agentic-org-<id>-linux-x86_64.tar.gz` and `.sha256` (plus `scripts/install.sh`) to any compiler-free Linux host and follow Step 2.
