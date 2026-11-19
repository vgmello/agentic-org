# C1 · Sign-in, private home and sub-agent isolation spike

Status: **recorded 2026-11-19** (macOS 27.2.1 arm64; Claude Code **2.1.311**; OpenAI Codex CLI **0.169.0**).
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
  - Portable 1-year `CLAUDE_CODE_OAUTH_TOKEN` (`claude setup-token`) works cleanly across macOS and headless Linux without Keychain or `HOME` conflicts.
  - Private home (`CLAUDE_CONFIG_DIR`), `--resume`, approval bridge (`--permission-prompt-tool stdio`), persistent background tasks + concurrent follow-up messaging, and sub-agent isolation are all live-verified.
- **Second adapter:** **`org-agent-runtime-codex`** (Codex CLI `0.169.0`, `codex app-server` over stdio).
  - Live-verified `chatgptAuthTokens` external-auth mode + `account/chatgptAuthTokens/refresh` callback with **zero `auth.json` written to disk**, enabling HQ-centralized single-lock refresh across offices.
- **Still pending before `dedicated-user` becomes the macOS default (ADR-0015 decision 7 / ISO-6):** running both vendors + an Xcode build under a second macOS OS account. `same-user` remains the default in C3 until that macOS dedicated-account check passes.

## 2. Live-verified findings

### 2.1 Claude Code (`2.1.311`)

| Question | Observed behaviour (verified live 2026-11-19) | Product impact |
| --- | --- | --- |
| **Private home (`CLAUDE_CONFIG_DIR`)** | All session transcripts (`projects/<encoded-cwd>/<session-id>.jsonl`), sub-agent transcripts, `.claude.json`, settings, and auto-memory (`projects/<encoded-cwd>/memory/`) stay inside `CLAUDE_CONFIG_DIR`. | Matches ADR-0015 decision 10 (`durable/local/offices/<office-id>/agents/<employee-id>/home/`). Disable auto-memory explicitly via `--settings '{"autoMemoryEnabled":false}'` when required. |
| **Overriding `HOME` on macOS with Keychain auth** | Setting `HOME` to the private directory **breaks macOS Keychain lookup** (`"Not logged in · Please run /login"`), because macOS resolves `login.keychain-db` from `HOME`. | With `CLAUDE_CODE_OAUTH_TOKEN`, both `HOME` and `CLAUDE_CONFIG_DIR` can point into the private home on macOS and Linux. When falling back to macOS Keychain auth, keep `HOME` as the OS account home and redirect only `CLAUDE_CONFIG_DIR` (with `CLAUDE_SECURESTORAGE_CONFIG_DIR=""` or a connection-scoped path). |
| **Relocated Keychain lookup on macOS** | Keychain item name is `"Claude Code-credentials"` when `CLAUDE_CONFIG_DIR` is unset or `CLAUDE_SECURESTORAGE_CONFIG_DIR=""`, and `"Claude Code-credentials-<sha256(dir)[:8]>"` when `CLAUDE_SECURESTORAGE_CONFIG_DIR` is non-empty. On headless Linux, credentials are stored in `<CLAUDE_CONFIG_DIR>/.credentials.json` (`0600`). | Explained in `spikes/claude/README.md` and exercised in `pocs/console-attach/tests/auth_login.rs`. |
| **Headless browser-less subscription enrollment** | Running `claude auth login --claudeai` or `claude setup-token` inside a wide (`512x24`) PTY with `BROWSER=/usr/bin/true`, `NO_BROWSER=1`, and `SSH_TTY=/dev/pts/0` suppresses local browser launch and emits `https://claude.com/cai/oauth/authorize?code=true&...&redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback...` followed by `Paste code here if prompted >`. | Enables an SSO-like browser login flow from `agentic-org-ui` without a terminal in the browser (ADR-0032). `claude setup-token` mints a portable 1-year `sk-ant-oat01-...` (`scope=user:inference`) token suitable for remote offices. |
| **Session resume from private home** | Verified in `spikes/claude/item04_resume.py`: `--resume <session-id>` resumes the same transcript inside `CLAUDE_CONFIG_DIR`. | Confirmed. |
| **Background tasks & sub-agents across turns (ADR-0025 #6, #10)** | Live-verified in both modes on Claude `2.1.311`: **(a) Default (background enabled — owner choice 2026-11-19):** keeping `stdin` open lets Turn 1 start a background Bash job or background sub-agent (`system/task_started`), return `result` immediately, accept and answer Turn 2 while the background work is still running, and automatically wake on `system/task_notification` when the background work finishes. **(b) Foreground-only option:** setting `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1` forces synchronous completion (`started_in_background: 0`, `completed: 1` before `result`). | `org-agent-runtime-claude` leaves background tasks **enabled** by default so the agent stays conversational while background commands/sub-agents run; the office tracks active background work (`task_started`/`task_notification`/`background_tasks_changed`), attributes reactive `system/init` wakeups to the run, and reaps all background processes on Pause/Stop/Offboard/stall (`SUB-5`). |
| **Sub-agent inheritance, approval bridge, and transcript path** | Live foreground probe (`CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1`, `--permission-mode manual --permission-prompt-tool stdio`): (1) sub-agent's `Bash` call arrived on the parent's `can_use_tool` stdio approval bridge with `"agent_id": "a5d37e112f953af70"`; (2) sub-agent inherited custom env vars (`C1_PROBE_MARKER`) and `CLAUDE_CONFIG_DIR`; (3) 4 sub-agent events streamed with `parent_tool_use_id`; (4) sub-agent transcript was written inside the private home at `projects/<encoded-cwd>/<session-id>/subagents/agent-<agent-id>.jsonl` and `.meta.json`; (5) `result.subagent_stats` and `total_cost_usd` accounted for the sub-agent. | Satisfies ADR-0025 `SUB-1` through `SUB-4` and gives C4 (`SUB-6`) the exact file layout to parse for sub-agent session views. |

### 2.2 OpenAI Codex CLI (`0.169.0`)

| Question | Observed behaviour (verified live 2026-11-19) | Product impact |
| --- | --- | --- |
| **Private home (`CODEX_HOME`)** | Pointing `CODEX_HOME` at a private directory keeps all SQLite state (`state_5.sqlite`, `logs_2.sqlite`, `memories_1.sqlite`, `queue_1.sqlite`, `goals_1.sqlite`) and sessions isolated there. | Matches ADR-0015 decision 10. |
| **Headless device-code enrollment** | Running `codex login --device-auth` in a PTY prints `https://auth.openai.com/codex/device` and a one-time user code (e.g. `QGM9-KSR16`, valid 15 minutes) without opening a server browser. | Enables browser-driven Codex sign-in from `agentic-org-ui` (ADR-0032). |
| **Refresh token rotation hazard (ADR-0015 #11)** | Codex OAuth refresh tokens have a 30-day sliding lifetime and are **single-use / rotating** (~8-day proactive refresh; ~1-hour access token). Copying one `auth.json` across two agents or offices causes the second refresher to fail with `refresh_token_already_used` / `refresh_token_revoked`. | Confirms ADR-0015 #11's warning: offices/agents must **never** concurrently refresh copies of the same Codex `auth.json`. |
| **External-auth mode (`chatgptAuthTokens`) in `codex app-server`** | Live probe against Codex `0.169.0` with `capabilities: { experimentalApi: true }` and empty `CODEX_HOME`: (1) `account/login/start` with `{"type":"chatgptAuthTokens","accessToken":<jwt>,"idToken":<jwt>,"chatgptAccountId":...,"chatgptPlanType":...}` succeeded (`{"result":{"type":"chatgptAuthTokens"}}`); (2) **zero `auth.json` was written to disk**; (3) on unauthorized token, `app-server` immediately emitted `{"method":"account/chatgptAuthTokens/refresh","id":0,"params":{"reason":"unauthorized","previousAccountId":"acct_synthetic_test"}}`. | Proves HQ can own the single mutex-guarded Codex `refresh_token` and serve short-lived ~1h `access_token`s to remote offices over Organization-CA mTLS (ADR-0032). Note: `chatgptAuthTokens` requires `experimentalApi: true` and must be re-verified on Codex upgrades. |
| **Codex sub-agents under isolation switches** | Pending live model turn under `codex app-server` + `shell_environment_policy` (to be verified when `org-agent-runtime-codex` starts after C3). | Recorded openly; does not block starting C3 with `org-agent-runtime-claude`. |

## 3. Reproducible evidence in this repository

- `pocs/console-attach/tests/auth_login.rs`:
  - `real_claude_login_extracts_live_oauth_url_in_isolated_dir`
  - `real_claude_setup_token_extracts_live_oauth_url`
  - `real_codex_device_login_extracts_live_url_and_code_in_isolated_dir`
  - `mock_claude_paste_code_flow_authenticates_end_to_end`
  - `mock_claude_setup_token_extracts_and_redacts_oauth_token`
- `pocs/console-attach/tests/claude_stream.rs`:
  - `real_claude_binary_completes_stream_json_initialize_without_model_call`
  - `persistent_stream_json_supports_background_work_approvals_followup_and_interrupt`
- Live probes executed 2026-11-19 against `/opt/homebrew/bin/claude` (`2.1.311`) and `/Users/vgmello/.local/bin/codex` (`0.169.0`) in isolated `/tmp` directories.
