# C1 · Sign-in, private home and sub-agent isolation spike

Status: **recorded 2026-11-19** (revised 2026-11-19 after review; **macOS 27.2.1 arm64 only**; Claude Code **2.1.311**; OpenAI Codex CLI **0.169.0**; **Linux unverified** pending a Linux run).
Satisfies the C1 sign-in and sub-agent isolation record in the
[implementation plan](../implementation-plan.md#c1--the-sign-in-spike), informs
[ADR-0015](../adr/0015-agent-isolation-modes.md) decisions 7, 10 and 11,
[ADR-0025](../adr/0025-agents-start-sub-agents.md) decision 10, and motivates
[ADR-0032](../adr/0032-browser-driven-cli-signin-and-remote-token-distribution.md).

Companion POC code and automated tests: [`pocs/console-attach/`](../../pocs/console-attach/README.md).
Earlier standalone Claude and Codex protocol spikes: [`spikes/claude/README.md`](../../spikes/claude/README.md),
[`spikes/codex/README.md`](../../spikes/codex/README.md).

## 1. Summary & first adapter choice

- **First adapter chosen for C3:** **`org-agent-runtime-claude`** (Claude Code `2.1.311`, `claude -p --input-format stream-json --output-format stream-json --verbose --forward-subagent-text`).
  - Portable 1-year `CLAUDE_CODE_OAUTH_TOKEN` (`claude setup-token`) avoids macOS Keychain and `HOME` conflicts (Linux runtime behavior remains unverified until exercised on Linux).
  - Private home (`CLAUDE_CONFIG_DIR`), `--resume`, approval bridge (`--permission-prompt-tool stdio`), persistent background tasks + concurrent follow-up messaging, and sub-agent isolation were exercised on macOS `27.2.1` (see evidence classification in §2 and §3).
- **Second adapter:** **`org-agent-runtime-codex`** (Codex CLI `0.169.0`, `codex app-server` over stdio).
  - Multi-office Codex pools default to HQ-centralized single-lock refresh (`chatgptAuthTokens`, recorded exception to "Works alone" in [architecture-non-negotiables.md](../architecture-non-negotiables.md) and [ADR-0032](../adr/0032-browser-driven-cli-signin-and-remote-token-distribution.md) decision 6), with per-office `codex login --device-auth` available for extended offline operation.
  - A **synthetic-token protocol probe** against Codex `0.169.0` confirmed `codex app-server` accepts `account/login/start` (`type: "chatgptAuthTokens"`) with **zero `auth.json` written to disk** and emits `account/chatgptAuthTokens/refresh` on an unauthorized token. A live model turn with real tokens and a vendor-supported refresh path outside the Codex CLI remain **unverified** and are required before shipping HQ-centralized refresh (see §2.2).
- **Still pending before `dedicated-user` becomes the macOS default (ADR-0015 decision 7 / ISO-6):** running both vendors + an Xcode build under a second macOS OS account. `same-user` remains the default in C3 until that macOS dedicated-account check passes.
- **Linux status:** all runs in this record were executed on macOS `27.2.1` arm64. Headless Linux credential storage (such as `<CLAUDE_CONFIG_DIR>/.credentials.json` `0600`) and Linux private-home isolation are **unverified** here and must be run on Linux before claiming Linux support.

## 2. Findings and evidence classification

Evidence labels used below:
- **Automated real-binary test:** committed test in this repo that spawns the real installed vendor CLI.
- **Interactive macOS run (no committed transcript):** observed live on macOS `27.2.1` on 2026-11-19 during this spike, without a committed raw transcript.
- **Synthetic protocol probe:** run against the real installed vendor binary using synthetic/non-production credentials (no real API/model turn).
- **Mock test:** committed test exercising protocol handling against a local mock script, not the real vendor binary.
- **Unverified / documented only:** inferred from vendor docs or prior spikes; not yet executed in this environment.

### 2.1 Claude Code (`2.1.311`, macOS only)

| Question | Evidence class | Observed behaviour | Product impact |
| --- | --- | --- | --- |
| **Private home (`CLAUDE_CONFIG_DIR`)** | Interactive macOS run + `spikes/claude/README.md` | Session transcripts (`projects/<encoded-cwd>/<session-id>.jsonl`), sub-agent transcripts, `.claude.json`, settings, and auto-memory (`projects/<encoded-cwd>/memory/`) stay inside `CLAUDE_CONFIG_DIR`. **Known caveat (`spikes/claude/README.md`):** when `HOME` is left at the user's real home, Claude CLI still creates empty folders under real `~/.claude` even with `CLAUDE_CONFIG_DIR` set. | Matches ADR-0015 decision 10 (`durable/local/offices/<office-id>/agents/<employee-id>/home/`). Disable auto-memory explicitly via `--settings '{"autoMemoryEnabled":false}'`. Redirecting both `HOME` and `CLAUDE_CONFIG_DIR` (possible with `CLAUDE_CODE_OAUTH_TOKEN`) avoids touching the real `~/.claude`. |
| **Overriding `HOME` on macOS with Keychain auth** | Interactive macOS run | Setting `HOME` to the private directory **breaks macOS Keychain lookup** (`"Not logged in · Please run /login"`), because macOS resolves `login.keychain-db` from `HOME`. | With `CLAUDE_CODE_OAUTH_TOKEN`, both `HOME` and `CLAUDE_CONFIG_DIR` can point into the private home on macOS. When falling back to macOS Keychain auth, keep `HOME` as the OS account home and redirect only `CLAUDE_CONFIG_DIR` (with `CLAUDE_SECURESTORAGE_CONFIG_DIR=""` or a connection-scoped path). |
| **Relocated Keychain lookup on macOS; Linux credential file** | Interactive macOS run + `spikes/claude/README.md` (**Linux unverified**) | On macOS, Keychain item name is `"Claude Code-credentials"` when `CLAUDE_CONFIG_DIR` is unset or `CLAUDE_SECURESTORAGE_CONFIG_DIR=""`, and `"Claude Code-credentials-<sha256(dir)[:8]>"` when `CLAUDE_SECURESTORAGE_CONFIG_DIR` is non-empty. On Linux, `<CLAUDE_CONFIG_DIR>/.credentials.json` (`0600`) is documented in vendor/prior notes but **unverified on Linux** in this spike. | Explained in `spikes/claude/README.md`; Linux credential storage must be verified on a real Linux host. |
| **Headless browser-less subscription enrollment** | Automated real-binary test (`auth_login.rs`) + interactive macOS run | Running `claude auth login --claudeai` or `claude setup-token` inside a wide (`512x24`) PTY with `BROWSER=/usr/bin/true`, `NO_BROWSER=1`, and `SSH_TTY=/dev/pts/0` suppresses local browser launch and emits `https://claude.com/cai/oauth/authorize?code=true&...&redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback...` followed by `Paste code here if prompted >`. End-to-end token extraction and paste-back code submission were tested against a mock in `auth_login.rs` and observed interactively on macOS (`claude setup-token` minting a 1-year `sk-ant-oat01-...`, `scope=user:inference`). | Enables browser-driven enrollment from `agentic-org-ui` under ADR-0004 decision 3's narrow exception (ADR-0032), minting one `setup-token` per office binding inside a wiped `0700` temp config dir. |
| **Session resume from private home** | `spikes/claude/item04_resume.py` (prior spike) | `--resume <session-id>` resumes the same transcript inside `CLAUDE_CONFIG_DIR`. | Confirmed on macOS. |
| **Background tasks & sub-agents across turns (ADR-0025 #6, #10)** | Interactive macOS run + mock test (`claude_stream.rs`) | **(a) Default (background enabled — owner choice 2026-11-19):** observed interactively on macOS that keeping `stdin` open lets Turn 1 start a background Bash job or background sub-agent (`system/task_started`), return `result`, accept and answer Turn 2 while background work runs, and wake on `system/task_notification` when it finishes. The committed test `persistent_stream_json_supports_background_work_approvals_followup_and_interrupt` exercises this protocol flow against a **mock**, not the real binary. **(b) Foreground-only option:** setting `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1` forced synchronous completion (`started_in_background: 0`, `completed: 1` before `result`) in the interactive probe. | `org-agent-runtime-claude` leaves background tasks **enabled** by default; reactive wakes record `started_by_kind = 'background_wake'` (doc 11); Pause/Stop/Offboard/stall reaps all remaining background work (`SUB-5`). |
| **Sub-agent inheritance, approval bridge, and transcript path** | Interactive macOS run (no committed transcript) | Observed in an interactive foreground probe (`CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1`, `--permission-mode manual --permission-prompt-tool stdio`): (1) sub-agent's `Bash` call arrived on the parent's `can_use_tool` stdio approval bridge with `"agent_id": "a5d37e112f953af70"`; (2) sub-agent inherited custom env vars (`C1_PROBE_MARKER`) and `CLAUDE_CONFIG_DIR`; (3) 4 sub-agent events streamed with `parent_tool_use_id`; (4) sub-agent transcript was written inside the private home at `projects/<encoded-cwd>/<session-id>/subagents/agent-<agent-id>.jsonl` and `.meta.json`; (5) `result.subagent_stats` and `total_cost_usd` accounted for the sub-agent. | Informs ADR-0025 `SUB-1`–`SUB-4` and C4 (`SUB-6`); automated conformance/opt-in live tests should lock this in during C3. |

### 2.2 OpenAI Codex CLI (`0.169.0`, macOS only)

| Question | Evidence class | Observed behaviour | Product impact |
| --- | --- | --- | --- |
| **Private home (`CODEX_HOME`)** | Interactive macOS run (no committed transcript) + `spikes/codex/README.md` | Pointing `CODEX_HOME` at a private directory keeps Codex SQLite state (`state_5.sqlite`, `logs_2.sqlite`, `memories_1.sqlite`, `queue_1.sqlite`, `goals_1.sqlite`) and sessions inside that directory. | Matches ADR-0015 decision 10. |
| **Headless device-code enrollment** | Automated real-binary test (`auth_login.rs`) | Running `codex login --device-auth` in a PTY prints `https://auth.openai.com/codex/device` and a one-time user code (valid 15 minutes) without opening a server browser. | Enables browser-driven Codex sign-in from `agentic-org-ui` at HQ (default centralized pool) or per office for extended offline operation (ADR-0032). |
| **Refresh token rotation hazard (ADR-0015 #11)** | Documented / prior observations (**not re-tested live with rotating real tokens here**) | Codex OAuth refresh tokens are single-use / rotating (commonly ~30-day sliding lifetime, ~8-day proactive refresh, ~1-hour access token). Copying one `auth.json` across concurrent offices risks `refresh_token_already_used` / `refresh_token_revoked`. | Confirms ADR-0015 #11's rule: never copy one rotating `auth.json` across offices or concurrent processes. Motivates HQ-centralized single-lock refresh as the multi-office Codex default (ADR-0032 decision 6). |
| **External-auth mode (`chatgptAuthTokens`) in `codex app-server`** | **Synthetic protocol probe** (real Codex `0.169.0` binary, synthetic tokens `acct_synthetic_test`; **no real model turn; no live refresh outside Codex CLI**) | With `capabilities: { experimentalApi: true }` and empty `CODEX_HOME`: (1) `account/login/start` with `{"type":"chatgptAuthTokens","accessToken":<synthetic jwt>,"idToken":<synthetic jwt>,"chatgptAccountId":"acct_synthetic_test",...}` succeeded (`{"result":{"type":"chatgptAuthTokens"}}`); (2) **zero `auth.json` was written to disk**; (3) on unauthorized token, `app-server` emitted `{"method":"account/chatgptAuthTokens/refresh","id":0,"params":{"reason":"unauthorized","previousAccountId":"acct_synthetic_test"}}`. **What was NOT verified:** a live model turn with real ChatGPT tokens, and whether HQ can refresh tokens outside the Codex CLI (doing so directly would require calling an undocumented OpenAI OAuth client/endpoint, which carries terms-of-service and stability risk). | Shows the `app-server` RPC wire contract exists with no on-disk `auth.json`, **not** yet a complete production HQ refresher. HQ-centralized refresh (the multi-office Codex default in ADR-0032 decision 6) is therefore gated before shipping on verifying both a live model turn with real tokens and a vendor-supported refresh path. `experimentalApi: true` must also be re-verified on Codex upgrades. |
| **Codex sub-agents under isolation switches** | Pending (**unverified**) | Pending live model turn under `codex app-server` + `shell_environment_policy` (to be verified when `org-agent-runtime-codex` is implemented). | Recorded openly; does not block starting C3 with `org-agent-runtime-claude`. |

## 3. Reproducible automated tests in this repository

- **Real-binary automated tests** (`pocs/console-attach/tests/`):
  - `auth_login.rs`: `real_claude_login_extracts_live_oauth_url_in_isolated_dir`
  - `auth_login.rs`: `real_claude_setup_token_extracts_live_oauth_url`
  - `auth_login.rs`: `real_codex_device_login_extracts_live_url_and_code_in_isolated_dir`
  - `claude_stream.rs`: `real_claude_binary_completes_stream_json_initialize_without_model_call`
- **Mock-based automated tests** (`pocs/console-attach/tests/`):
  - `auth_login.rs`: `mock_claude_paste_code_flow_authenticates_end_to_end`
  - `auth_login.rs`: `mock_claude_setup_token_extracts_and_redacts_oauth_token`
  - `claude_stream.rs`: `persistent_stream_json_supports_background_work_approvals_followup_and_interrupt`
- Interactive probes and synthetic protocol probes were run on 2026-11-19 on macOS `27.2.1` arm64 against `/opt/homebrew/bin/claude` (`2.1.311`) and `/Users/vgmello/.local/bin/codex` (`0.169.0`) in isolated `/tmp` directories.
