# `console-attach` — Rust POC: Persistent Claude Code (`stream-json`) + Unix PTY Proxy

This standalone Rust POC implements two complementary ways to drive terminal programs and coding agents from the background over a local Unix domain socket:

1. **Structured persistent agent daemon (`claude ...`)** — runs **one long-lived `claude -p --input-format stream-json --output-format stream-json --verbose`** process with `stdin` kept open. Unlike one-shot `claude -p "prompt"`, the session stays alive across turns so background bash commands and subagents keep running while you send new messages, approve tool calls, interrupt mid-turn, or stream structured JSON events over a Unix socket.
2. **Unix PTY proxy (`run` / `send` / `screen` / `demo-vi`)** — allocates a real pseudo-terminal (`openpty` + `TIOCSCTTY`) so any interactive TUI (`vi`, shells, etc.) can be driven by translated keystrokes (`vi:save` -> `ESC` + `:w\r`) while attached to a terminal or running headless.

---

## Mode 1: Persistent Claude Code (`stream-json`)

### Why this solves the `claude -p` background-command problem
- Plain `claude -p "prompt"` exits after one turn and has no open input channel while work is running.
- Keeping `stdin` open with `--input-format stream-json --output-format stream-json --verbose --forward-subagent-text` keeps **one persistent session alive**:
  - `send` writes a `{"type":"user",...}` frame **non-blocking** — you can send follow-up messages even while a background bash job or subagent is still running.
  - `interrupt` sends a `control_request` (`interrupt`) mid-turn without killing the session.
  - `approve` / `deny` answer `can_use_tool` permission prompts over the socket (when started with `--stdio-approvals`).
  - `events` / `follow` return clean structured JSON or human-formatted lines (including `subagent_text`, `tool_use`, `tool_result`, and `result`).

### Live walkthrough (2 terminals)

**Terminal 1** — start a persistent Claude Code daemon:
```bash
cd pocs/console-attach
cargo run -- claude start \
  --socket /tmp/claude-agent.sock \
  --model haiku \
  --permission-mode acceptEdits \
  --max-budget-usd 0.50
```
*(Or add `--stdio-approvals` to approve/deny tool calls manually via `claude approve <id>`, or `--auto-approve` to auto-allow them.)*

**Terminal 2** — send messages anytime (even while background commands/subagents are running):
```bash
cd pocs/console-attach

# 1. Ask Claude to start a long background bash job (non-blocking send):
cargo run -- claude send --socket /tmp/claude-agent.sock \
  "Start a background bash command that sleeps 20 seconds and writes done to /tmp/bg-done.txt, then reply immediately."

# 2. Check status or stream live events (including subagent output):
cargo run -- claude status --socket /tmp/claude-agent.sock
cargo run -- claude follow --socket /tmp/claude-agent.sock

# 3. Send a second message while the background job is still running:
cargo run -- claude send --wait --socket /tmp/claude-agent.sock \
  "While that runs in the background, what is 2 + 2?"

# 4. Mid-turn interrupt (without killing the session) or clean shutdown:
cargo run -- claude interrupt --socket /tmp/claude-agent.sock
cargo run -- claude stop --socket /tmp/claude-agent.sock
```

---

## Mode 2: PTY Proxy for Interactive TUIs (`vi`, shells, etc.)

### Built-in `vi` demo
```bash
cd pocs/console-attach
cargo run -- demo-vi
```

### Interactive two-terminal walkthrough (watch `vi` respond live)

**Terminal 1:**
```bash
cd pocs/console-attach
cargo run -- run --socket /tmp/vi-demo.sock -- vi /tmp/hello.txt
```

**Terminal 2:**
```bash
cd pocs/console-attach
cargo run -- send --socket /tmp/vi-demo.sock vi:insert "Hello from Terminal 2!"
cargo run -- send --socket /tmp/vi-demo.sock vi:append-line "Driven via Rust PTY proxy."
cargo run -- send --socket /tmp/vi-demo.sock vi:save
cargo run -- screen --socket /tmp/vi-demo.sock
cargo run -- send --socket /tmp/vi-demo.sock vi:save-quit
```

---

## Mode 3: SSO-Like Login Web UI + Rust API (`auth-ui`)

Drives one-time subscription login headlessly inside a Rust PTY so a web UI can offer **"Sign in with Claude"** (OAuth URL + paste-back code) and **"Sign in with Codex"** (OAuth device code):

```bash
cd pocs/console-attach
cargo run -- auth-ui --port 8787
# Open http://127.0.0.1:8787
```

By default, credentials are stored in an isolated directory (`/tmp/ca-sso-demo`) so your real `~/.claude` and `~/.codex` are untouched. Pass `--use-default-config` to write to your real CLI config.

HTTP endpoints:
- `GET  /api/presets` -> returns user-friendly sandbox & permission presets for Advanced Settings
- `POST /api/auth/start` `{ "provider": "claude_token" | "claude" | "codex" }` -> returns `{ login_id, flow, state, url, user_code }`
- `POST /api/auth/submit` `{ "login_id": "...", "code": "..." }` -> writes pasted code into Claude's PTY stdin; for `claude_token` (`claude setup-token`), returns the minted 1-year `oauth_token` (`sk-ant-oat01-...`) once authenticated (redacted from `output_preview`) so it can be distributed to remote offices via `CLAUDE_CODE_OAUTH_TOKEN` / `--oauth-token`
- `GET  /api/auth/status?id=<login_id>` -> polls state (`awaiting_code` / `awaiting_device_auth` -> `authenticated`)
- `POST /api/auth/cancel` `{ "login_id": "..." }` -> cancels the pending login process

---

## State Isolation (`--isolated-dir` / `--config-dir`) vs. Workspace Sandbox

**Important distinction:** `--isolated-dir` / `--config-dir` does **not** restrict which project files an agent can read or edit — it relocates the CLI's own **config, credentials, sessions, and memory** (`CLAUDE_CONFIG_DIR` / `CODEX_HOME`) so multiple agents or users on the same machine stay isolated from each other and from `~/.claude` / `~/.codex`.

### Where sessions, memory, and credentials are stored

| Data | Claude Code (`$CLAUDE_CONFIG_DIR`, default `~/.claude/`) | Codex (`$CODEX_HOME`, default `~/.codex/`) |
| --- | --- | --- |
| **Session store (`--resume`)** | `<CLAUDE_CONFIG_DIR>/projects/<encoded-cwd>/<session-id>.jsonl` | `<CODEX_HOME>/sessions/` (and state DB files under `<CODEX_HOME>/`) |
| **Auto-memory (`MEMORY.md`)** | `<CLAUDE_CONFIG_DIR>/projects/<encoded-cwd>/memory/` | `<CODEX_HOME>/memories/` (if enabled) |
| **Settings / skills / plugins** | `<CLAUDE_CONFIG_DIR>/settings.json`, `.claude.json`, `skills/` | `<CODEX_HOME>/config.toml`, `skills/` |
| **Credentials (macOS)** | macOS Keychain item (`Claude Code-credentials`, or `Claude Code-credentials-<hash>` when `CLAUDE_SECURESTORAGE_CONFIG_DIR` is set) | `<CODEX_HOME>/auth.json` (or OS keyring if configured) |
| **Credentials (headless Linux)** | `<CLAUDE_CONFIG_DIR>/.credentials.json` (`0600` file) | `<CODEX_HOME>/auth.json` (`0600` file) |

*(Note: Claude auto-memory loads from `<CLAUDE_CONFIG_DIR>/projects/.../memory/` even with `--setting-sources ""` unless disabled with `--settings '{"autoMemoryEnabled": false}'`.)*

---

## Secure-by-Default Sandbox & Friendly Permission Presets (Advanced Settings)

File access and command execution are controlled separately by the **workspace directory (`--cwd`)**, the **OS sandbox** (macOS Seatbelt / Linux bubblewrap), and the **permission preset**.

`console-attach claude start` defaults to **`safe_ask` with OS sandbox enabled**, and exposes friendly presets under Advanced Settings (`--preset`, `--sandbox on|off`, and `GET /api/presets` in `auth-ui`):

| Friendly Preset (`--preset`) | UI Label | OS Sandbox Default | Claude Code Mapping | Codex Mapping |
| --- | --- | --- | --- | --- |
| `safe_ask` *(default)* | Ask before actions (Recommended) | **On** | `--permission-mode manual` + `--permission-prompt-tool stdio` + sandbox enabled | `-s workspace-write -a on-request` |
| `read_only` | Read-only / Plan | **On** | `--permission-mode plan` + sandbox enabled | `-s read-only -a on-request` |
| `workspace_edits` | Auto-approve edits, ask for shell | **On** | `--permission-mode acceptEdits` + `--permission-prompt-tool stdio` + sandbox enabled | `-s workspace-write --approve-for-me` |
| `autonomous_sandboxed` | Autonomous (sandboxed) | **On** | `--permission-mode auto` + sandbox enabled (`autoAllowBashIfSandboxed: true`) | `-s workspace-write -a never` |
| `unrestricted` *(High Risk)* | Unrestricted (no sandbox, no prompts) | **Off** | `--permission-mode bypassPermissions` | `--dangerously-bypass-approvals-and-sandbox` |

Example reusing an isolated SSO login with the secure default preset:
```bash
# 1. Sign in via browser UI into /tmp/ca-sso-demo
cargo run -- auth-ui --port 8787 --isolated-dir /tmp/ca-sso-demo

# 2. Launch a persistent Claude session using that isolated login + secure default preset
cargo run -- claude start \
  --config-dir /tmp/ca-sso-demo/claude \
  --cwd /path/to/project \
  --preset safe_ask
```

---

## Remote-Office Authentication, Centralized Refresh & Encryption Architecture

When running multiple offices (e.g., an HQ Mac + remote headless Linux offices), credentials must **never** travel in plaintext or be stored unencrypted in Git/DBs.

### 1. Per-provider token lifecycle across offices

| Provider | Master credential at HQ | Lifetime & rotation | What remote offices receive | Offline from HQ ("Works alone") |
| --- | --- | --- | --- | --- |
| **Claude Code** | 1-year OAuth token (`sk-ant-oat01-...`) minted via `claude setup-token` (`provider: "claude_token"` in `auth-ui`) | 1 year (`scope=user:inference`), machine-independent | `CLAUDE_CODE_OAUTH_TOKEN` injected into the `claude` process env (never via `argv` in production) | Works up to 1 year once wrapped locally under the remote host key |
| **Codex (centralized refresh)** | Single rotating OAuth `refresh_token` minted once at HQ via `codex login --device-auth` | Refresh token: **30 days sliding** (single-use rotation, ~8-day proactive refresh); access token: **~1 hour** | Short-lived ~1h `access_token` only — passed to `codex app-server` via `account/login/start` (`type: "chatgptAuthTokens"`); refresh token **never leaves HQ** | Works for the current ~1h `access_token` window; needs HQ reachable to refresh (for multi-day HQ-offline operation, run an independent `codex login --device-auth` on that office) |

Why Codex refresh is centralized at HQ (or per-office, never copied):
- OpenAI refresh tokens are **single-use**. Copying one `auth.json` to two offices causes the first refresh to revoke the other (`refresh_token_already_used`).
- With `chatgptAuthTokens` mode, `codex app-server` installs an external-auth bridge and emits `account/chatgptAuthTokens/refresh` on `401`/expiry. The remote host requests a fresh `access_token` from HQ; HQ serializes refreshes behind a single lock and replies without dropping the live session. *(Note: `chatgptAuthTokens` is marked experimental in Codex `app-server` and should be probed against the target Codex release before production reliance.)*

### 2. Zero-plaintext transport & key ownership (HQ <-> Remote Office)

- **Asymmetric office identity:** Each remote office generates its own key pair locally; its **private key never leaves that machine** (wrapped under the remote host key / Keychain, aligned with `crates/org-pki` and ADR-0016). HQ stores only the remote office's **public key / client certificate** bound to `office_id` so compromised or retired offices can be revoked individually.
- **In transit:** All HQ <-> remote office token exchanges run over **mTLS (TLS 1.3)** (mutual authentication + transport encryption). When traffic crosses any relay or proxy, payloads are additionally **envelope-encrypted (AEAD) to the remote office's public key** so intermediaries cannot read them.
- **At rest & runtime isolation:**
  - HQ stores master tokens wrapped under HQ's host key — never in Git, logs, or plaintext DB columns.
  - Remote offices hold Codex `access_token`s in host memory only (no `auth.json` on disk) and inject Claude/Codex credentials strictly into the agent runtime process (never exposed to Git, org data, or tool subshells).

---

## Checks

```bash
cd pocs/console-attach
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The test suite covers:
- Unit tests for ANSI stripping, rolling buffers, and key/`vi:*` translation.
- End-to-end PTY integration tests against real `/usr/bin/vi` and `/bin/sh`.
- End-to-end `claude` `stream-json` daemon tests covering `initialize`, `can_use_tool` approvals, concurrent background/subagent events, follow-up turns, `interrupt`, and clean `stop` — plus a live `initialize` handshake check against the locally installed `claude` binary (zero model turns).
