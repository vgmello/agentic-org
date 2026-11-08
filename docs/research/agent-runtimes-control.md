# Controlling agent runtimes from the office server: Claude Code, Codex, Antigravity

Research for Agentic Org's office server (`org-office-server`, Rust). It answers how the office can start, prompt, observe, resume, stop and meter agents on three runtimes, and recommends one session abstraction. Written 2026-11-08.

## Evidence legend

- **[ran]**: run on this Mac today, output captured.
- **[src]**: read in the runtime's own source at a pinned release tag.
- **[doc]**: official documentation (URLs under Sources).
- **[3p]**: third-party write-up. Treat as a lead, not a fact.
- **[unverified]**: not confirmed. Each one says why.

## Versions

| Runtime | Version used | How |
| --- | --- | --- |
| Claude Code | **2.1.311** (`/opt/homebrew/bin/claude`) | Installed. `claude --help`, subcommand help and live `-p` runs **[ran]** |
| OpenAI Codex CLI | **rust-v0.167.0** (latest release, 2026-11-06) | **Not installed** (`codex: command not found`). Flags come from the source at that tag **[src]** and from learn.chatgpt.com docs **[doc]** |
| Google Antigravity CLI (`agy`) | **1.5.3** (latest release, 2026-11-07) | **Not installed** (`agy: command not found`). Facts come from antigravity.google docs **[doc]**, the official CHANGELOG **[doc]** and third-party probes **[3p]** |

I didn't install Codex or agy. Both need a sign-in, and installing them was outside a write-no-code research brief. Everything about them still needs a hands-on spike (see the last section).

---

## 0. TL;DR

1. **All three have a long-lived, stdin/stdout JSON control mode.** One driver shape fits all three: one child process with JSON lines in and JSON lines out.
   - **Claude Code:** `claude -p --input-format stream-json --output-format stream-json --verbose`. Verified here with two turns in one process, an in-band interrupt, and a clean exit on stdin EOF.
   - **Codex:** `codex app-server`, which speaks JSON-RPC over stdio JSONL or over a Unix socket. It's multi-thread and has first-class approvals and status notifications. `codex exec --json` is one turn per process.
   - **Antigravity:** `agy -p --input-format stream-json --output-format stream-json`, or `agy acp`, an Agent Client Protocol server over stdio.
2. **"Turn finished" is explicit everywhere:**
   - Claude: the `result` message.
   - Codex: `turn/completed`, or `turn.completed` from `exec`. The app-server also sends `thread/status/changed` → `idle`.
   - agy: a `result` event per turn, or the `session/prompt` response under ACP.
3. **Resume by id works everywhere:** `claude --resume <uuid>`, `thread/resume {threadId}` or `codex exec resume <id>`, and `agy --conversation <id>` or ACP `session/resume`.
4. **Only Claude reports money** (`total_cost_usd`, a client-side estimate). Codex and agy report tokens only, so the office prices tokens itself or shows quota percentages.
5. **Human attach differs a lot:**
   - **Codex:** best. The TUI can connect to a running app-server with `codex --remote unix://PATH` **[src]**.
   - **Claude:** a headless `-p` process can't be attached to. The `--bg` daemon + `claude attach` path (spike S2) can, but it has no programmatic prompt channel and no structured event stream.
   - **agy:** take-over only (stop headless, reopen interactively).
6. **Main risks:**
   - The Claude path choice conflicts with ADR-0004's `--bg` + attach design.
   - Protocol stability: Claude's stdin control messages are semi-documented, Codex app-server is marked "experimental", and agy is closed-source with weekly releases.
   - **Auth and ToS for a third-party product** driving subscription logins. Anthropic explicitly disallows offering claude.ai login in third-party products without approval.
   - Approvals in headless mode, which all three deny by default.

---

## 1. What exists locally today

### 1.1 `agents/bin/agent` (how agents start today)

`bin/agent` is a thin wrapper: `exec python3 hr/hr.py session "$@"`. `hr.py session_command()` builds:

```
claude --agent <agent-id> --name <agent-id> [--plugin-dir mods/<mod> ...] [extra...]      # new
claude --agent <agent-id> --resume <agent-id> [--plugin-dir ...] [extra...]               # --resume
```

`cmd_session` then `chdir`s to the repository root and `execvp`s `claude`.

- Every agent runs **interactively in the user's terminal**, from the repo root. Identity comes from `--agent`, which points at `.claude/agents/<agent>.md` (the generated registration that tells the agent to read its home `CLAUDE.md`).
- `--name` makes the session resumable by name.
- Mods (`mods/goals`) load with `--plugin-dir`.
- There's no headless or programmatic control. The agent's instructions are a subagent definition plus `CLAUDE.md` files, not a system-prompt flag.

### 1.2 App repo `origin/spike/s1-s2` (S1 presence, S2 background sessions and the attach panel)

Both spikes ran against Claude Code 2.1.311 (the version here).

**S2 (`spikes/s2-sessions/README.md`)** answers ADR-0004 §6. Its verdict: "keep the chosen design (Claude Code background sessions + `claude attach` in a PTY)". Facts that matter here:

- **Start:** `claude --bg` returns in 0.57–0.80 s, because a per-user daemon (`claude daemon run`) hands over a pre-started spare.
  - It prints `backgrounded · <id8> · <name>`.
  - `--bg` **ignores `--session-id`**. The full uuid comes from `claude agents --json`.
- **Ids:** `attach`, `stop` and `logs` take only the 8-hex short id. `--resume` takes the uuid. Store both.
- **Environment:** the caller's env doesn't reach a bg session. Only cwd and `PATH` do, and `PATH` comes from whichever client last started or woke it. Everything else comes from the daemon's env. Pass variables with `--settings '{"env":{…}}'`.
- **Reopen:** `claude attach <id8>` wakes a stopped session with its saved `respawnFlags`.
  - `--bg --resume <uuid>` with any flags **forks a copy**.
  - Right after `stop`, it usually forks a copy that has no mods (5 of 7 trials).
- **`--bg` setup:**
  - It needs a trusted workspace.
  - It appends stdin to the prompt, so spawn it with stdin from `/dev/null`.
  - It needs `"worktree": {"bgIsolation": "none"}`, or Edit and Write are blocked in the main checkout.
- **Liveness:** `claude agents --json` gives `status` (`busy`, `idle`, `waiting` + `waitingFor`) and `state`. Liveness means having a `pid`.
- **`claude logs <id8>`** is raw terminal bytes, not a transcript.
- **pty-host:** a Rust crate that runs `claude attach <id8>` in a PTY behind seq-numbered frames.

**S1 (`spikes/s1-status/README.md`)** proved status from a presence mod through hooks, a Unix socket, the Rust receiver and a macOS notification. Amendments that matter here:

- Undecided `PermissionRequest` in headless `-p` gives a false "Needs you" (amendment 3).
- Use `Stop.background_tasks` (subagents only) for "still working".
- The presence mod must be the **first** `--plugin-dir`.
- `AC_AGENT` must go through `--settings env` for `--bg`.

### 1.3 ADRs on `origin/v1`

- **ADR-0001** ("the app is a client; the office server holds the organization"):
  - One `org-office-server` process per office, supervised by `org-office-host` (a per-user SMAppService login item).
  - Each office owns its **runtime side: sessions, live status, local queues**.
  - The office server is also the message hub (decision 4). `org-agent` talks to its own office over the office's local socket.
  - `crates/` are libraries and `apps/` are runnables.
- **ADR-0003** (messaging, office-first):
  - Agents "run across multiple CLIs (Claude Code, Antigravity, Codex)".
  - Each runtime gets a **thin (~30-line) adapter** around `org-agent msg notify`, run from the runtime's **`Stop` hook**.
  - Presence is `busy` while executing a turn, `idle` while holding `msg.wait` or between turns, and `offline` when no session is active.
  - So the ADRs already assume **every runtime has a Stop-like hook**. All three do (Claude: `Stop`; Codex: `Stop` **[src]**; agy: `Stop` with `fullyIdle` **[doc]**).
- **ADR-0004** (sessions) is referenced by the spikes but isn't on `v1` yet.

---

## 2. Claude Code (2.1.311)

### 2.1 Headless and programmatic modes

**A. One-shot:** `claude -p "<prompt>" --output-format json|stream-json`.

**B. Long-lived streaming session over stdin/stdout** **[ran]**:

```
claude -p --input-format stream-json --output-format stream-json --verbose \
  [--include-partial-messages] [--include-hook-events] [--replay-user-messages] \
  --model <m> --session-id <uuid> --append-system-prompt "<text>" \
  --permission-mode <acceptEdits|auto|bypassPermissions|manual|dontAsk|plan> \
  [--allowedTools "Bash(git *) Edit"] [--disallowedTools ...] [--tools "Bash,Edit,Read"] \
  [--add-dir <dir>...] [--settings <file-or-json>] [--setting-sources user,project,local] \
  [--mcp-config <json|file>...] [--strict-mcp-config] [--plugin-dir <dir>...] \
  [--agent <name>] [--agents <json>] [--permission-prompts host|none] \
  [--max-budget-usd N] [--json-schema <schema>] [--fallback-model m1,m2] [--effort level]
```

- **stdin, one JSON line per user turn:**

  ```json
  {"type":"user","message":{"role":"user","content":"What is your name?"}}
  ```

  Verified: two turns in one process. The process stays alive between turns (checked after 2 s idle) and **exits 0 when stdin is closed**.
- **In-band control requests** (the protocol the Agent SDK uses) **[ran]**:
  - `{"type":"control_request","request_id":"init1","request":{"subtype":"initialize"}}` returns `control_response` with `subtype: "success"` (it lists commands, agents and so on).
  - `{"type":"control_request","request_id":"int1","request":{"subtype":"interrupt"}}` returns `{"type":"control_response","response":{"subtype":"success","request_id":"int1","response":{"still_queued":[]}}}` within ~30 ms. The turn then ends with `result` `subtype:"error_during_execution"`, `is_error:true`, `terminal_reason:"aborted_streaming"`.
  - **The process stays alive.** The next user line was answered normally (`"STILL HERE"`).
- The `system/init` event has a **`capabilities`** array for feature detection. Seen here: `interrupt_receipt_v1`, `interrupt_cancel_queued_v1`, `interrupt_send_now_v1`, `interrupt_requeue_taken_v1`, `msg_lifecycle_v1`, `lifecycle_wakeup_source_v1`, `ui_surface_v1` and others.
- **Approvals over stdio:** the SDK's `canUseTool` callback uses `--permission-prompt-tool` (the SDK passes `stdio`) plus `can_use_tool` control requests from CLI to host. `--permission-prompts none` turns hosting off **[doc]**. **[unverified]:** I didn't exercise the `can_use_tool` wire format.

**C. Claude Agent SDK** (`@anthropic-ai/claude-agent-sdk` for TypeScript, `claude-agent-sdk` for Python):

- It's "a library that runs the Claude Code binary" **[doc]**: a typed wrapper over mode B (`query()`, streaming input, `interrupt()`, `canUseTool`, `resume`, `forkSession`, hooks).
- The office is Rust, so it would either embed Node or Python, or speak mode B's protocol directly. The docs say: "To drive the same agent loop from a language other than Python or TypeScript, run the CLI as a subprocess with `-p`" **[doc]**.

**D. Background daemon (S2's path):**

- `claude --bg …` starts a session. `claude attach|logs|stop|rm <id8>`, `claude agents --json [--all] [--cwd]` and `claude respawn <id>|--all` manage it.
- `-p` rejects `--bg` **[doc]**.
- There's **no programmatic way to send a prompt to a running bg session**: `claude -p --resume <bg-session> "…"` with piped output, `--output-format json` or config flags "sends nothing and exits with status 1" **[doc]**. Only typing into an `attach` PTY works.

**Other modes:** `--remote-control` (interactive only) and `--cloud` (cloud sessions) aren't relevant to local agents.

### 2.2 Instructions

- **Memory files:** `CLAUDE.md` is auto-discovered (user `~/.claude/CLAUDE.md`, project hierarchy, `.claude/rules`).
  - Verified: a `CLAUDE.md` saying "reply in exactly three words" in the cwd shaped the `-p` reply **[ran]**.
  - `--bare` skips `CLAUDE.md` auto-discovery, hooks, plugins, MCP and auto-memory. You then pass `--append-system-prompt[-file]`, `--settings`, `--mcp-config`, `--agents` and `--plugin-dir` explicitly **[doc]**.
- **System prompt:**
  - `--append-system-prompt` adds to the default prompt (verified: "Your name is Zed." → "I'm Zed here.").
  - `--system-prompt` replaces it, and drops the git-status snapshot.
  - `--agent <name>` selects a subagent definition (what `bin/agent` uses). `--agents <json>` defines agents inline.
- **Gotcha (`--system-prompt-snapshot`, on by default except under `--bare`):** the system prompt is recorded on the conversation's first request and "every later request and resume sends the record as-is, even when a later launch passes different text, until the conversation is compacted" **[ran: `--help`]**.
  - So **changing an agent's instructions doesn't reach a resumed conversation** until compaction, unless the run passes `--system-prompt-snapshot off`.
  - This matters for "edit agent → resume".

### 2.3 Sessions

- **Storage:** `~/.claude/projects/<cwd with non-alphanumerics → '->/<session-uuid>.jsonl` **[doc][ran]**.
  - Retention is 30 days (`cleanupPeriodDays`).
  - `CLAUDE_CONFIG_DIR` and `CLAUDE_CODE_PROJECT_DIR_NAME` relocate it.
  - The format is internal. Don't parse it as an API.
- **Getting the id:** `session_id` is on every stream event (`system/init` first) and on `result`.
- **Assigning the id:** `--session-id <uuid>` works with `-p` **[ran]**. Reusing an id whose transcript already exists fails with `Error: Session ID … is already in use.` **[ran]**. So `--session-id` is for **creating**, and `--resume` is for continuing.
- **Resume:** `claude -p --resume <uuid> …` recalled turn-2 content in a new process **[ran]**.
  - It also works from a different cwd: Claude Code "finds the session by its ID in any project on this machine" **[doc][ran]**. It appended to the original project's transcript.
  - `--continue` picks the most recent session in the cwd. `--fork-session` forks instead.
- **Not restored on resume:** `--mcp-config`, `--settings`, `--plugin-dir`, `--fallback-model` and `--add-dir`. **The office must re-pass the full launch spec** **[doc]**. (`--bg` sessions instead save `respawnFlags`; see S2.)
- **Permission mode:** a `-p` resume starts in the mode a new `-p` run would use, which is **Manual since v2.1.302** unless a flag or setting sets one **[doc]**. Always pass `--permission-mode`.
- `-p` and SDK sessions are **excluded from the interactive `/resume` picker and from `claude --continue`**, but `claude --resume <id>` opens them **[doc]**.

### 2.4 Events (stream-json) and idle detection

These were seen in today's runs **[ran]**:

- `system`, with subtypes `init`, `status` (`{"status":"requesting"}`) and `thinking_tokens`.
- `assistant` and `user` (tool results).
- `stream_event` with `--include-partial-messages`: `message_start`, `content_block_*`, `message_delta`, `message_stop`.
- `rate_limit_event`.
- `result`.
- `control_response`.

Also documented **[doc]**: `system/api_retry`, `permission_denied` system messages, `system/plugin_install`, hook events with `--include-hook-events`, and `prompt_suggestion` with `--prompt-suggestions`.

A trimmed real turn:

```json
{"type":"system","subtype":"init","session_id":"26941cd1-…","cwd":"…/ws1","model":"claude-haiku-5-5","permissionMode":"dontAsk","claude_code_version":"2.1.311","apiKeySource":"none","capabilities":["interrupt_receipt_v1","…"],"tools":[…],"mcp_servers":[…],"memory_paths":{…}}
{"type":"assistant","message":{"model":"claude-haiku-5-5","content":[{"type":"text","text":"Hello there, friend."}],"usage":{"input_tokens":2,"cache_creation_input_tokens":11992,"cache_read_input_tokens":13446,"output_tokens":3,…}},"parent_tool_use_id":null,"session_id":"26941cd1-…"}
{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","rateLimitType":"five_hour","unifiedWindows":{"five_hour":{"utilization":0.63,…},"seven_day":{"utilization":0.52,…}}}}
{"type":"result","subtype":"success","is_error":false,"result":"Hello there, friend.","stop_reason":"end_turn","terminal_reason":"completed","num_turns":1,"duration_ms":1888,"duration_api_ms":1558,"total_cost_usd":0.00260456,"usage":{…},"modelUsage":{"claude-haiku-5-5":{"inputTokens":2,"outputTokens":143,"cacheReadInputTokens":13446,"cacheCreationInputTokens":11992,"costUSD":0.00260456,"costBasis":"list",…}},"model_usage_since_last_result":{…},"result_index":0,"queued_turn_count":0,"permission_denials":[],"session_id":"26941cd1-…"}
```

**Turn finished:** one `result` per turn. `subtype` is `success`, `error_during_execution`, `error_max_turns` or similar. `terminal_reason` is `completed`, `aborted_streaming` and so on. `result_index` counts results in the process, and **`queued_turn_count`** gives the queued user turns still pending.

**Busy and idle:**
- Busy runs from writing a user line, or from `system/status requesting`, until a `result` with `queued_turn_count == 0`.
- **`system/init` is re-emitted at the start of every turn** in streaming mode **[ran]**, so it doubles as a "turn started" signal.
- **Caveat:** background work (background subagents and commands, Monitor, `/loop`) can start **another turn after a `result`** without new input **[doc]**. Idle is therefore "last event was a `result`, with no new `init` since". The S1 presence mod (hooks) is a second, independent signal.

### 2.5 Interactive vs headless

- **A `-p` process has no TTY and can't be attached to.** `claude --resume <id>` in a terminal while the headless process is live starts a **second writer**: "If you resume the same session in two terminals without forking, messages from both interleave into one transcript" **[doc]**.
- **What the office can offer for a headless session:**
  - (a) A live read-only view rendered from the event stream. The office already has it.
  - (b) **Take-over:** finish or interrupt the turn, close stdin, then open `claude --resume <uuid>` in a PTY (pty-host) for the human. When the human leaves, the office resumes headless with `-p --resume`. That needs a single-writer lock per session in the office.
  - (c) The `--bg` daemon path (S2), which is attachable but not programmatically promptable.

### 2.6 Usage and cost fields

- **Per assistant message:** `message.usage` with `input_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`, `output_tokens`, `cache_creation.{ephemeral_5m,1h}`, `service_tier`, `speed`.
- **Per `result`:**
  - `total_cost_usd` and `usage` and `modelUsage` are **cumulative for the process**. Verified: turn 1 = 0.00230508, turn 2 = 0.00251445.
  - **`model_usage_since_last_result`** is the per-turn delta. Verified: turn 2 = 0.00020937.
  - When resuming, totals include earlier runs' spend **[doc]**.
  - Also `num_turns`, `duration_ms`, `duration_api_ms`, `ttft_ms` and `subagent_stats`.
- **Cost is a client-side estimate** (`costBasis:"list"`) **[doc]**. Under a subscription, the useful number is **`rate_limit_event`** (5-hour and 7-day utilization and reset times).
- `--max-budget-usd` caps spend in `-p`.

### 2.7 Gotchas

- **Auth:** `-p` uses the subscription OAuth login from the keychain. Here: `authMethod: claude.ai`, `subscriptionType: max`, `apiKeySource: "none"` **[ran]**.
  - `--bare` never reads OAuth or the keychain. It needs `ANTHROPIC_API_KEY` or `apiKeyHelper` **[doc]**.
  - **Policy:** "Unless previously approved, Anthropic does not allow third party developers to offer claude.ai login or rate limits for their products, including agents built on the Claude Agent SDK" **[doc]**.
  - If Agentic Org ships to other users and drives their Claude subscription, that needs a decision (legal/product) **[unverified how it applies to driving the user's own installed CLI]**.
- **Leaked parent environment:** inherited `CLAUDE_CODE_*` and `AI_AGENT` variables leak from a parent agent session. S2 showed `CLAUDE_CODE_CHILD_SESSION=1` **turns off transcript saving**. My runs stripped every `CLAUDE*` variable and `AI_AGENT`. Build the child env from an allowlist (S2 follow-up).
- **User-level config leaks into every agent:** my `-p` runs loaded the user's claude.ai MCP connectors (Google Drive, Claude Docs) and plugins **[ran]**. Use `--strict-mcp-config`, `--setting-sources`, or `--bare` plus explicit flags to pin an agent's toolset.
- **Trust:** `-p` skips the workspace-trust dialog, and repo `.claude/settings.json` hooks and `.mcp.json` servers **still run** **[doc]**. `--bg` instead **requires** prior trust (S2).
- **Permissions:**
  - A `-p` run defaults to Manual mode, and any prompt is denied unless a hook, `--permission-prompt-tool` or an allow rule resolves it **[doc]**.
  - `dontAsk` denies anything that would prompt.
  - S1: an undecided `PermissionRequest` in `-p` produces a false "Needs you".
- **Stop semantics** **[doc][ran]**:
  - Closing stdin finishes the turn and exits 0. After stdin closes, it waits up to 10 min for background work (`CLAUDE_CODE_PRINT_BG_WAIT_CEILING_MS`).
  - SIGINT, or the `interrupt` control request, ends the turn.
  - SIGTERM exits 143 and leaves the turn unrecorded, killing Bash process trees and running `SessionEnd`.
  - `CLAUDE_CODE_RESUME_INTERRUPTED_TURN=1` makes a resume continue the interrupted turn.
- **TTY:** none needed for `-p`. `attach` needs a PTY of at least 40x10 (S2).
- **Startup:** `system/init` arrived after 1.2–4.0 s in today's runs **[ran]**.
- **Concurrency:** I found no documented process cap. N agents share one account's rate limits (`rate_limit_event`). **[unverified]:** behavior at many simultaneous `-p` processes.

---

## 3. OpenAI Codex CLI (rust-v0.167.0, not installed)

### 3.1 Headless and programmatic modes

**A. `codex exec`** (alias `e`): one turn per process **[src: `codex-rs/exec/src/cli.rs`, `utils/cli/src/shared_options.rs`] [doc]**.

```
codex exec [OPTIONS] [PROMPT]            # PROMPT or '-' reads stdin; piped stdin + prompt → appended as <stdin> block
codex exec resume [SESSION_ID|--last] [--all] [PROMPT]   # SESSION_ID = uuid or thread name; --all disables cwd filter
codex exec fork <SESSION_ID> [PROMPT]
codex exec review ...
```

- **Output:**
  - `--json` (alias `--experimental-json`) prints JSONL events on stdout.
  - Without it, progress goes to stderr and the final message to stdout.
  - `-o/--output-last-message FILE` and `--output-schema FILE` (a JSON Schema for the final message).
- **Safety:**
  - `-s/--sandbox read-only|workspace-write|danger-full-access`. `exec` defaults to **read-only**.
  - `--approve-for-me` (alias `--not-so-yolo`) routes approvals through automatic review with workspace-write.
  - `--dangerously-bypass-approvals-and-sandbox` (alias `--yolo`).
  - **`--full-auto` is no longer supported** **[doc]**.
  - `exec` sets `approval_policy = never` by default ("Default to never ask for approvals in headless mode") **[src: exec/src/lib.rs]**.
- **Context:** `-C/--cd DIR`, `--add-dir DIR`, `--worktree`, `--skip-git-repo-check`, `--ephemeral` (no rollout files), `--ignore-user-config`, `--ignore-rules`, `--strict-config`.
- **Model and config:** `-m/--model`, `-p/--profile`, `-c key=value` (config override), `-i/--image`, `--oss`, `--local-provider`.
- **Hooks:** `--dangerously-bypass-hook-trust`.

**B. `codex app-server`** (marked "[experimental]" in `--help`; the documented integration surface for rich clients) **[doc][src]**:

- **Transport:**
  - stdio JSONL by default (`--listen stdio://` or `--stdio`).
  - **`--listen unix://` or `unix://PATH`**.
  - `ws://IP:PORT` is "experimental and unsupported".
  - `off`.
  - JSON-RPC 2.0 **without** the `"jsonrpc"` field.
- **Handshake:** `initialize` (`clientInfo`, optional `capabilities.experimentalApi`, `optOutNotificationMethods`), then the `initialized` notification.
- **Threads:**
  - `thread/start`. Params at v0.167 **[src: v2/thread.rs]**: `model`, `modelProvider`, `cwd`, `approvalPolicy`, `approvalsReviewer`, `sandbox`, `config{}`, `serviceName`, **`baseInstructions`**, **`developerInstructions`**, `personality`, `ephemeral`, plus experimental fields.
  - `thread/resume {threadId, cwd?, developerInstructions?, …, excludeTurns}`.
  - `thread/fork`, `thread/read`, `thread/list`, `thread/turns/list`, `thread/archive`, `thread/delete`, `thread/compact/start`.
- **Turns:**
  - `turn/start {threadId, input:[{type:"text",text}], cwd?, approvalPolicy?, sandboxPolicy?, model?, effort?, outputSchema?}` responds with `status:"inProgress"`.
  - **`turn/steer {threadId, input, expectedTurnId}`** appends input to the running turn.
  - **`turn/interrupt {threadId, turnId}`** ends the turn with `status:"interrupted"`.
- **Server-to-client requests (approvals):**
  - `item/commandExecution/requestApproval` and `item/fileChange/requestApproval`. Decisions: `accept`, `acceptForSession`, `decline`, `cancel`.
  - `item/permissions/requestApproval`, `mcpServer/elicitation/request`, `tool/requestUserInput` (experimental).
- **Notifications:**
  - `thread/started`, **`thread/status/changed`**, `turn/started`, `item/started`, `item/completed`, `item/agentMessage/delta`, **`turn/completed`**, **`thread/tokenUsage/updated`**, **`account/rateLimits/updated`**, `thread/closed` and others.
  - `ThreadStatus` = `notLoaded | idle | systemError | active{activeFlags:[waitingOnApproval|waitingOnUserInput]}` **[src]**.
- **Daemon:** `codex app-server daemon start|stop|restart|version`, plus `codex app-server proxy` (stdio to the running daemon's control socket) **[src]**.
- **Schema:** `codex app-server generate-ts --out DIR` and `generate-json-schema --out DIR` emit **version-matched** types. Use them to generate Rust types or contract tests.

**C. `@openai/codex-sdk`** (TypeScript): wraps the CLI and exchanges JSONL over stdin/stdout (`startThread`, `run`, `runStreamed`, `resumeThread(id)`). It's the same capability as `exec --json` **[doc: sdk/typescript/README.md]**.

**D. Not present at v0.167:** I found **no `mcp-server` and no `proto` subcommand** in the CLI's `Subcommand` enum or the crate list **[src]**. Older releases had `codex mcp-server` and `codex proto`; treat them as gone. Other subcommands of note: `resume`, `fork`, `queue --thread <id|name> --message <text> [--resume]` (queue a message for an existing session), `agents` (command center), `archive`, `delete`, `login`, `logout`, `doctor`, `remote-control`, `exec-server`.

### 3.2 Instructions

- **`AGENTS.md` chain:**
  - Global: `$CODEX_HOME/AGENTS.override.md` or `AGENTS.md`.
  - Then each directory from the repo root down to the cwd: `AGENTS.override.md`, else `AGENTS.md`, else `project_doc_fallback_filenames`. At most one file per directory, and empty files are skipped.
  - The total is capped by `project_doc_max_bytes`, default **32 KiB**, and is **cut silently**.
  - It's rebuilt every run **[doc]**.
- **Config keys** **[src: core/config.schema.json]:**
  - `developer_instructions` ("inserted as a developer role message"), settable per run with `-c developer_instructions="…"`.
  - `instructions` (system).
  - `model_instructions_file`, which replaces the built-in model instructions ("STRONGLY DISCOURAGED").
  - The app-server equivalents are `developerInstructions` and `baseInstructions` on `thread/start` and `thread/resume`.

### 3.3 Sessions

- **Storage:** rollout JSONL under `$CODEX_HOME/sessions/` (default `~/.codex/sessions`), plus `archived_sessions/` **[src: rollout/src/lib.rs][doc]**. `--ephemeral` disables it.
- **Id:** `thread_id` from the first `thread.started` event (exec), or the `thread/start` response or `thread/started` notification (app-server).
- **Resume in the same cwd:**
  - `codex exec resume <id> "<prompt>"`. Without `--all`, `--last` and the picker filter by cwd **[src]**.
  - App-server: `thread/resume {threadId, cwd}`.
  - TUI: `codex resume [<id>|--last]`.
  - **[unverified]:** whether threads started by `exec` or the app-server show in the TUI picker (`--thread-source` exists).

### 3.4 Events and idle detection

`exec --json` **[doc][src: exec/src/exec_events.rs]**:

```json
{"type":"thread.started","thread_id":"0199a213-81c0-7800-8aa1-bbab2a035a53"}
{"type":"turn.started"}
{"type":"item.completed","item":{"id":"item_3","type":"agent_message","text":"Repo contains docs, sdk, and examples directories."}}
{"type":"turn.completed","usage":{"input_tokens":24763,"cached_input_tokens":24448,"output_tokens":122,"reasoning_output_tokens":0}}
```

- Event types: `thread.started`, `turn.started`, `turn.completed`, `turn.failed`, `item.started`, `item.updated`, `item.completed`, `error`.
- Item types: `agent_message`, `reasoning`, `command_execution` (`command`, `aggregated_output`, `exit_code`, `status`), `file_change`, `mcp_tool_call`, `web_search`, `todo_list`, `collab_tool_call`, `error`.
- **Turn finished:** `turn.completed` or `turn.failed`, then the process exits.
- **App-server:**
  - `turn/completed` (`turn.status`: `completed | interrupted | failed`).
  - **Idle and busy come straight from `thread/status/changed`.** `active{activeFlags:["waitingOnApproval"]}` maps to "Needs you".

### 3.5 Interactive vs headless

- The TUI can **connect to a running app-server**: `codex --remote unix://PATH` (or `ws://…`, with `--remote-auth-token-env`) **[src: cli/src/main.rs `InteractiveRemoteOptions`]**.
  - If the office runs `codex app-server --listen unix://<office-dir>/codex.sock`, a human could open a TUI on the same server and its threads.
  - **[unverified]:** whether two clients can drive the same loaded thread concurrently. The docs say `thread/start` auto-subscribes the client; multi-client fan-out isn't confirmed.
- Otherwise, take-over: `codex resume <id>` in a PTY after the headless run ends.

### 3.6 Usage and cost fields

- **`exec`:** `turn.completed.usage = {input_tokens, cached_input_tokens, cache_write_input_tokens, output_tokens, reasoning_output_tokens}` **[src]**.
- **App-server:** `thread/tokenUsage/updated {threadId, turnId, tokenUsage:{total, last, modelContextWindow}}`, where `TokenUsageBreakdown = {totalTokens, inputTokens, cachedInputTokens, cacheWriteInputTokens, outputTokens, reasoningOutputTokens}` **[src]**.
- **No cost or USD field anywhere** (grep of the v2 protocol and the exec events **[src]**).
- `account/rateLimits/updated` gives `{limitId, primary:{usedPercent, windowDurationMins, resetsAt}}` **[doc]**.

### 3.7 Gotchas

- **Auth:**
  - `codex login` (ChatGPT sign-in, stored in `~/.codex/auth.json` or the keyring), or an API key.
  - `CODEX_API_KEY=<key> codex exec …` works per run **[doc]**.
  - App-server: `account/login/start` with `type` = `apiKey | chatgpt | chatgptDeviceCode | chatgptAuthTokens` (the last is experimental) **[doc]**.
  - The docs call ChatGPT-managed auth in automation "an advanced path" **[doc]**.
  - **[unverified]:** OpenAI's terms for third-party products driving a user's ChatGPT-plan Codex.
- **A Git repo is required** unless `--skip-git-repo-check` ("to prevent destructive changes") **[doc]**.
- **Sandbox:** on macOS it's Seatbelt **[unverified on this Mac]**. `danger-full-access` and `--yolo` turn it off.
- **Hooks:** events are `PreModelRequest`, `PostModelRequest`, `PreToolUse`, `PermissionRequest`, `PostToolUse`, `PreCompact`, `PostCompact`, `SessionStart`, `SessionEnd`, `UserPromptSubmit`, `SubagentStart`, `SubagentStop`, **`Stop`**, `Interrupt` **[src: hooks/src/lib.rs]**. They need **persisted hook trust**, or `--dangerously-bypass-hook-trust` for vetted automation. That matters for ADR-0003's Stop adapter.
- **Required MCP servers:** an enabled server with `required = true` that fails to start makes `exec` exit with an error **[doc]**.
- **Env vars:** `CODEX_HOME`, `CODEX_API_KEY` (exec), `OPENAI_API_KEY`. Don't put keys in the env of jobs that run repo code **[doc]**.
- **Stability:** app-server is experimental. Experimental fields need `capabilities.experimentalApi`, and the schema follows each CLI version.

---

## 4. Google Antigravity CLI `agy` (1.5.3, not installed)

### 4.1 Headless and programmatic modes

**A. Print mode** **[doc: /docs/cli/headless, /docs/cli/reference]**:

```
agy -p "<prompt>" [--output-format text|json|stream-json] [--json-schema <schema|file|type>]
    [--model <slug>] [--effort low..max] [--agent <name>] [--mode accept-edits|auto|plan]
    [--continue|-c] [--conversation <id>] [--add-dir <dir>...]
    [--dangerously-skip-permissions] [--sandbox] [--print-timeout <dur, 0=unlimited>]
    [--disable-slash-commands] [--log-file <path>]
```

- Aliases: `-p`, `--print`, `--prompt`.
- There's **no `--cwd`**: the process cwd is the workspace.
- There's **no system-prompt flag**.

**B. Multi-turn stream** (since 1.1.15) **[doc]**:

```
agy --input-format stream-json --output-format stream-json [flags]
```

- stdin takes one JSON `user` event per line. Content is a string or a list of `text` blocks.
- It keeps one `conversation_id` and emits `init` once.
- **Each turn ends with its own `result`**, and the session stays open until stdin closes. Closing stdin finishes the current turn and exits 0.
- **Don't pass `-p`** in streaming mode: prompts passed as flags are dropped. A third-party driver needed `-p=` on 1.4.2 **[3p]**.
- The input line shape is `{"event":"user","message":{"content":"…"}}`. The official docs say a missing `event` field is an error; the full example is from a **[3p]** probe.
- `control_request`/`control_response` events and CLI slash commands **end the session with exit 2**. Invalid JSON, a missing `event` or non-text content gives exit 1.

**C. `agy acp`** (an official ACP server over stdio, since 1.5.0) **[doc: CHANGELOG 1.5.0, /docs/cli/acp]**:

- It's JSON-RPC over stdin/stdout (Agent Client Protocol: `initialize`, `session/new`, `session/prompt`, `session/cancel`, `session/update` notifications, `session/request_permission` from agent to client).
- Per the CHANGELOG it supports images, **"steering a running turn by sending another prompt"**, the modes `default`, `accept-edits` and `plan`, **`session/resume` restoring a session without replaying its history**, a per-session model and thinking level, and `agy acp --login`.
- 1.5.1 and 1.5.2 fixed cancel and resume races in ACP.
- In ACP, `session/prompt`'s response carries `stopReason`. In the ACP spec that's `end_turn`, `max_tokens`, `max_turn_requests`, `refusal` or `cancelled` **[unverified for agy]**.

**D. Antigravity SDK:** Python (`pip install google-antigravity`), `Agent(...).chat()`, with `GEMINI_API_KEY` or Vertex auth **[doc]**. It isn't useful from Rust, and it's API-key based.

### 4.2 Instructions

- **Files** **[doc: /docs/customization-discovery]:**
  - Workspace `AGENTS.md`, `GEMINI.md` and `.agents/` (`rules/*.md`, `agents/<name>.md` or `agents/<name>/agent.md`, `hooks.json`, `skills.json`, `plugins.json`).
  - They're read from the workspace folder and **each parent up to the repo root**, plus `--add-dir` folders.
  - Personal: `~/.gemini/AGENTS.md`, `~/.gemini/GEMINI.md`, `~/.gemini/config/` (rules, agents, `hooks.json`).
- Rules and skills are **re-read before each turn**. Rules have a 24 KB per-file cap **[doc: CHANGELOG]**.
- **System prompt:**
  - There's no flag. Use a **custom agent** (`.agents/agents/<name>.md`, Markdown with YAML frontmatter and H1-delimited system prompts **[doc: CHANGELOG 1.1.x]**), selected with `--agent <name>`. An unknown agent exits 1 in `-p` **[doc]**.
  - Or use workspace rules.
  - So an Agentic Org agent maps to a generated `.agents/agents/<agent-id>.md`.
- **Settings:** `~/.gemini/antigravity-cli/settings.json` (`toolPermission`, `agentMode`, `permissions` allow/ask/deny, `enableTerminalSandbox`, `trustedWorkspaces`, `model`, `modelProvider`) and project overrides in `~/.gemini/config/projects/` **[doc]**.

### 4.3 Sessions

- **Id:** `conversation_id` in the `json` envelope and stream `result`, and in `init` **[doc][3p]**. A third-party trick: an `init` arrives after the first stdin line **[3p]**.
- **Scope:** conversations are scoped to the cwd. "Antigravity CLI scopes conversation histories directly to your current working directory" **[doc]**.
- **Resume:** `--conversation <id>` (by id) or `-c` (most recent). Interactive: `/resume` picker, `/fork`.
- **Storage location: [unverified].** It isn't documented. The CHANGELOG mentions a SQLite store and token files the CLI owns, and third parties warn never to read them **[3p]**.

### 4.4 Events and idle detection

stream-json **[doc]**:

- **`init`** (once): `cwd`, `tools`, `permission_mode` (default `request-review`).
- **`step_update`:** `step_index`, `state` (`ACTIVE | DONE`, plus `ERROR` per **[3p]**), `step_type` (`user_input | agent_response | tool | checkpoint`, plus `subagent`, `system_message` per **[3p]**), text deltas, and `tool_info{name, parameters, output, error}` or `subagent_info`.
- **`result`:** the same shape as the json envelope:
  - `conversation_id`, `status` (`SUCCESS | ERROR | CANCELED | INTERRUPTED | INVALID | WAITING | RUNNING`), `response` (this turn only), `error`, `duration_seconds`, `num_turns`, `usage`.
  - `structured_output`, plus `denied_actions` (1.2.x) and `finished_reason` (`print_timeout | interrupted | error`, omitted on normal finish, 1.4.11).
- **Turn finished:** `result`. **Idle:** after `result`, until the next stdin line.
- **No in-band interrupt in stream mode:** control messages exit 2.
  - SIGINT gives `result{status:"ERROR", error:"interrupted"}` and exit 1, and kills the tool's process group **[3p, 1.4.2]**.
  - So "interrupt" means kill, then `--conversation <id>` resume (2–9 s startup **[3p]**). ACP `session/cancel` is the clean path.
- **Exit codes:** 0 OK; 1 bad model, bad agent or bad input; 2 unsupported stream message; **3 when a model or agent error happens mid-stream, or `--print-timeout` expires** (with an `AGY_ERROR: {...}` JSON line on stderr) **[doc: CHANGELOG]**.
- **Hooks** (for ADR-0003 and presence) **[doc: /docs/hooks]:**
  - `SessionStart`, `PreToolUse`, `PostToolUse`, `PreInvocation`, `PostInvocation`, **`Stop`** (`terminationReason`, `fullyIdle`). `Stop` can return `decision:"continue"`.
  - Files: `.agents/hooks.json` and `~/.gemini/config/hooks.json`.
  - Stdin fields: `conversationId`, `workspacePaths`, `transcriptPath`, `artifactDirectoryPath`, `modelName`, `lastUserInput`.

### 4.5 Interactive vs headless

- There's no attach to a headless process.
- Take-over is `agy --conversation <id>` or `/resume` in a PTY in the same cwd. **[unverified]:** whether headless conversations appear in the picker. `--conversation` is documented as a general flag.
- `--remote-control` exists (browser control of a CLI session), but it's out of scope.

### 4.6 Usage and cost fields

- `usage = {input_tokens, output_tokens, thinking_tokens, cache_read_tokens, total_tokens}`. It's **cumulative per session** (process plus resumed conversation), so diff it per turn **[doc][3p]**.
- **No cost or credit field.**
- Credits and quota come from `agy -p "/credits"` or `"/usage"` with `--output-format json`. These are answered without a model turn and leave no conversation behind **[doc: CHANGELOG 1.1.x]**. But `/usage` and `/model` **aren't allowed inside a stream** session **[doc]**.

### 4.7 Gotchas

- **Auth:** Google sign-in cached in the system keyring. **An interactive `agy` sign-in is needed once.** Headless runs fail fast with `authentication required` **[doc]**.
  - Alternatives: `GEMINI_API_KEY` with `modelProvider:"gemini"`, `AGY_ADC_AUTH` (GCP Application Default Credentials), and `agy acp --login`.
  - **ToS:** a forum thread quotes ToS §6 ("Using third party software, tools, or services to access the Service"). A **non-staff** reply says local headless invocation is supported as long as the tool never touches OAuth tokens **[3p]**. Needs an official answer.
- **Permissions:**
  - Headless **soft-denies** anything needing approval. The run continues, exits 0, and reports `denied_actions` **[doc]**.
  - A `PreToolUse` hook's `allow` reportedly **does not** override headless auto-deny **[3p]**.
  - So unattended runs need `--dangerously-skip-permissions` with a `PreToolUse` hook as the gate **[3p]**. Since 1.5.2, admin-required reviews are still denied **[doc]**.
  - Or use ACP `session/request_permission`.
- **Hook semantics:** an **empty hook stdout counts as allow** (fail-open) **[3p]**.
- **Background work:** headless waits for background tasks unless `--print-timeout` is set (since 1.5.0) **[doc]**.
- **Env vars:** `GEMINI_API_KEY`, `AGY_ADC_AUTH`, `AGY_CLI_*` (UI only), `PLUGIN_ROOT`/`PLUGIN_DATA` (for plugin hooks).
- **Churn:** releases are near-daily (1.5.1 → 1.5.3 in three days), the binary is closed-source, and `agy --version` is a bare number **[3p]**. Pin a version and run contract tests.
- **Event-shape gaps:** a non-text-content line exits 1. Split stdout on `\n` only: U+2028 and U+2029 break naive readline **[3p]**.

---

## 5. Side by side

| Need | Claude Code 2.1.311 | Codex 0.167 | Antigravity 1.5.3 |
| --- | --- | --- | --- |
| Long-lived driver | `-p --input-format stream-json --output-format stream-json --verbose` **[ran]** | `codex app-server` (stdio or `--listen unix://PATH`), JSON-RPC | `--input-format stream-json --output-format stream-json`, or `agy acp` |
| One-shot | `-p --output-format json` | `codex exec --json` | `agy -p --output-format json` |
| Instructions | `CLAUDE.md` tree, `--append-system-prompt`, `--system-prompt`, `--agent`/`--agents` | `AGENTS.md` chain (32 KiB), `developerInstructions` and `baseInstructions`, `-c developer_instructions=` | `AGENTS.md`/`GEMINI.md`/`.agents/rules`, custom agent `.agents/agents/<n>.md` + `--agent` |
| cwd | process cwd (+ `--add-dir`) | `-C` / `thread/start.cwd` | process cwd (+ `--add-dir`) |
| Send a prompt later | another stdin `user` line (queued while busy) | `turn/start`; `turn/steer` mid-turn | another stdin line (queued); ACP prompt = steer |
| Turn done | `result` (+ `queued_turn_count`) | `turn/completed`; `thread/status/changed` → `idle` | `result` / `session/prompt` response |
| Needs approval | `permission_denied` / `can_use_tool` control request (host mode) | `item/*/requestApproval`, `active{waitingOnApproval}` | soft-deny → `denied_actions`; ACP `session/request_permission` |
| Interrupt | `control_request{subtype:"interrupt"}`, process survives **[ran]** | `turn/interrupt` | stream: none (SIGINT kills); ACP `session/cancel` |
| Session id | `session_id` (uuid); can pre-assign `--session-id` | `thread_id` | `conversation_id` |
| Resume | `--resume <uuid>`; re-pass config flags | `thread/resume` / `exec resume <id>` | `--conversation <id>` / ACP `session/resume` |
| Storage | `~/.claude/projects/<cwd>/<uuid>.jsonl` | `~/.codex/sessions/…` | undocumented |
| Stop | close stdin → exit 0; SIGTERM → 143 | close connection / `thread/unsubscribe`; kill | close stdin → finish turn → 0 |
| Human attach | not for `-p`; `--bg` + `claude attach` only | `codex --remote unix://PATH` TUI **[src]** | take-over only |
| Tokens | per message + per turn delta | `turn.completed.usage` / `thread/tokenUsage/updated` | cumulative `usage` |
| Cost | `total_cost_usd` (estimate) | none | none |
| Quota | `rate_limit_event` (5h/7d %) | `account/rateLimits/updated` | `-p "/usage"`, `"/credits"` |
| Stop hook (ADR-0003) | `Stop` | `Stop` (trust needed) | `Stop` (`fullyIdle`) |

---

## 6. Recommendation: one "agent session" abstraction

### 6.1 Shape

Put the contract in a small, stable crate that the office domain owns, for example `crates/org-runtime`. Each runtime gets an adapter crate (`org-runtime-claude`, `org-runtime-codex`, `org-runtime-agy`).

- The office server depends on the trait. `apps/org-office-server` wires in the adapters.
- Adapters depend only on the trait crate and their CLI, never on each other or on office storage.
- That follows the architecture rules: dependency inversion around volatile, third-party CLIs, no cycles, and each adapter with Ce ≤ 3.

```
AgentRuntime            (one per runtime; stateless factory)
  capabilities() -> Caps { attach: Native|TakeOver|None, interrupt: InBand|Kill, cost_usd: bool,
                           approvals: Host|DenyOnly, steer: bool }
  start(spec: SessionSpec) -> Session        // spec = agent id, cwd, instructions, model, permission
                                             //        policy, env allowlist, extra dirs, mcp, mods
  resume(spec, runtime_session_id) -> Session

Session                 (one live agent conversation, owned by one office task)
  send(Input)                          // queued if busy; Steer if caps.steer and asked for
  interrupt()                          // end current turn, keep session
  respond(ApprovalId, Decision)        // only if caps.approvals == Host
  events() -> Stream<SessionEvent>     // normalized; each carries the raw runtime JSON too
  state() -> Starting | Idle | Busy | NeedsApproval | NeedsInput | Stopping | Stopped | Crashed
  stop(Graceful | Now)
  runtime_session_id() -> String       // persisted by the office for resume

SessionEvent: Started{runtime_session_id, model} | TurnStarted | TextDelta | Message
            | ToolStarted/ToolFinished | ApprovalRequested | Denied
            | TurnFinished{status: Ok|Interrupted|Failed, usage_delta: Tokens, cost_delta_usd: Option}
            | Quota{window, used_pct, resets_at} | Error | Exited{code}
```

- **Persist in `office.db`:** `(agent_id, runtime, runtime_session_id, cwd, spec_hash, last_state)`. Resume after an office restart is then `runtime.resume(spec, id)`.
- **Single writer per session:** the office holds a lock. A human take-over releases the headless process first.
- **Usage:** the office stores **per-turn deltas** (it diffs cumulative counters for Claude totals and for agy), and prices Codex and agy tokens itself from a price table, or shows tokens and quota % only.

### 6.2 Mapping

| Operation | Claude Code adapter | Codex adapter | Antigravity adapter |
| --- | --- | --- | --- |
| start | spawn `claude -p --input-format stream-json --output-format stream-json --verbose --session-id <uuid> --agent <id> --append-system-prompt … --permission-mode … --settings '{"env":{…}}' --strict-mcp-config --plugin-dir <presence> --plugin-dir <goals> …`, cwd = workspace, env from allowlist (strip `CLAUDE*`, `AI_AGENT`) | one `codex app-server --listen unix://<office>/codex.sock` per office (or stdio per office); `thread/start {cwd, developerInstructions, approvalPolicy, sandbox, model}` | spawn `agy --input-format stream-json --output-format stream-json --agent <id> --mode … [--dangerously-skip-permissions + PreToolUse gate]` in cwd; or `agy acp` + `session/new` |
| send | stdin `{"type":"user",…}` | `turn/start` (or `turn/steer`) | stdin `{"event":"user",…}` / ACP `session/prompt` |
| events | parse stream-json; Started=`system/init`; TurnFinished=`result` | notifications | `init`/`step_update`/`result`; ACP `session/update` |
| idle/busy | busy from send or `init` until `result` with `queued_turn_count==0`; presence-mod hooks as second signal | `thread/status/changed` | after `result`; `Stop` hook `fullyIdle` |
| approvals | `--permission-prompt-tool stdio` + `can_use_tool` control requests (spike), else `dontAsk`/`auto` + allow rules | `requestApproval` → `respond` | ACP `request_permission`, else skip-permissions + hook gate |
| interrupt | `control_request interrupt` | `turn/interrupt` | ACP `session/cancel`; stream mode: SIGINT then resume |
| resume | `--resume <uuid>` + the **full spec again** (flags aren't restored); note the system-prompt snapshot | `thread/resume {threadId, cwd, developerInstructions}` | `--conversation <id>` / ACP `session/resume` |
| stop | close stdin (graceful), then SIGINT, then SIGTERM after a timeout | `thread/unsubscribe`/close; stop daemon on office stop | close stdin; ACP close |
| usage | `model_usage_since_last_result` + `total_cost_usd`; `rate_limit_event` | `thread/tokenUsage/updated.last`; `account/rateLimits/updated` | diff cumulative `usage` |
| attach | TakeOver (stop headless → `claude --resume <uuid>` in pty-host → hand back), or keep S2's `--bg` path for agents the owner wants to "sit with" | Native: app TUI panel runs `codex --remote unix://…` (spike) | TakeOver: `agy --conversation <id>` in a PTY |

### 6.3 Decision needed: the Claude path versus ADR-0004

S2 chose `--bg` + `claude attach` because the attach panel is the product's core interaction. For **an office that sends prompts and reads structured events**, `--bg` has gaps:

- No programmatic prompt: the only ways in are keystrokes into a PTY, or the agent pulling messages itself through `org-agent msg wait` (ADR-0003).
- No structured events: only hooks, `claude agents --json` polling, and raw `logs`.
- No usage or cost stream.

The `-p` stream driver has all of those, but no live attach. Options:

- **(a) Headless-first (my recommendation for the multi-runtime office):**
  - Every runtime runs under the structured driver.
  - The app's session panel renders the event stream.
  - "Open terminal" is a take-over with a lock.
  - It's uniform across the three runtimes and gives one presence model.
- **(b) Hybrid:**
  - Claude agents run as `--bg` for interactive work.
  - The office talks to them only through `org-agent` messages, hooks and `agents --json`.
  - Codex and agy go headless.
  - Two models to maintain, and Claude turns and usage aren't metered.
- **(c) Keep ADR-0004 as is for Claude**, and accept no programmatic send or usage for Claude.

Whatever the choice, ADR-0004 should state how the office sends a prompt and reads usage for a Claude agent.

### 6.4 Main risks

1. **Contract stability:**
   - Claude: the stdin `control_request` wire format is what the Agent SDK uses but isn't a published, versioned API. Use `system/init.capabilities` for feature detection.
   - Codex: the app-server is "[experimental]". Generate types per pinned version with `generate-json-schema`.
   - agy: closed-source with near-daily releases.
   - Mitigation: pin CLI versions per office, run contract tests per adapter (recorded fixtures, like S1's receiver tests), and keep raw events alongside normalized ones.
2. **Auth and terms for a product:**
   - Anthropic disallows third-party products offering claude.ai login without approval.
   - The Google ToS §6 position is unclear.
   - OpenAI's position on ChatGPT-plan auth in third-party products is unverified.
   - Personal use of one's own CLI is the normal case; shipping Agentic Org to others needs legal and product sign-off and an API-key path for each runtime.
3. **Approvals in unattended runs:** all three deny or soft-deny by default. Without a host approval bridge, agents silently fail actions:
   - Claude: `permission_denied` / `permission_denials`.
   - Codex: `approval_policy = never` in `exec`.
   - agy: `denied_actions`, with exit 0 and an empty response.
   The app-server and ACP have proper approval requests. Claude needs the `can_use_tool` path proven.
4. **Environment and isolation:**
   - Inherited `CLAUDE_CODE_*` variables (transcripts silently off).
   - The user's global MCP connectors and plugins leak into every agent (seen here).
   - A GUI-launched office has a minimal `PATH` (S2/S3).
   - Use an env **allowlist** and explicit config isolation (`--strict-mcp-config`, `--setting-sources`, `--ignore-user-config`, maybe per-agent `CLAUDE_CONFIG_DIR` + `CLAUDE_CODE_PROJECT_DIR_NAME`, and `CODEX_HOME`).
5. **Resume fidelity:**
   - Claude doesn't restore `--settings`, `--mcp-config`, `--plugin-dir` or `--add-dir`, and freezes the system prompt at the first request (`--system-prompt-snapshot`).
   - Codex filters by cwd.
   - agy is cwd-scoped.
   - The office must store and replay the whole `SessionSpec` and always resume in the original cwd.
6. **Metering:** only Claude has USD, and it's an estimate. Codex and agy need an office-side price table or quota-only display. Counters are cumulative in different ways (per process for Claude, per session for agy), so record deltas.
7. **Process cost and concurrency:**
   - One process per agent for Claude and agy (Claude init took 1–4 s here; memory per process wasn't measured).
   - Codex can host many threads in one app-server.
   - Every runtime shares one account's rate limits across all of the office's agents, so the scheduler needs quota awareness.
8. **Interrupt asymmetry:** agy's stream mode has no in-band interrupt, so cancelling means killing and resuming (seconds). Prefer `agy acp` if its spike passes.

### 6.5 Spikes to close the gaps

- **S-claude-driver:**
  - The `can_use_tool` approval round-trip through `--permission-prompt-tool stdio`.
  - `--include-hook-events`.
  - Extra turns from background work after `result`.
  - Take-over: stop headless → interactive `--resume` in pty-host → back to `-p --resume`.
  - Resume with the full spec replayed, and the system-prompt snapshot behavior.
  - RSS per idle process.
- **S-codex** (install a pinned `@openai/codex@0.167.x`):
  - App-server over `unix://`, two threads in two cwds, approvals, interrupt.
  - `thread/tokenUsage/updated`.
  - TUI `--remote` joining a thread the office already loaded.
  - Hook trust for the ADR-0003 Stop adapter.
- **S-agy** (install 1.5.x, sign in once):
  - `agy acp` versus stream-json: interrupt, resume, `request_permission`, usage.
  - Where conversations live.
  - Whether headless conversations are resumable interactively.
  - The custom-agent file as the instructions carrier.

## Sources

- Local:
  - `claude --help`, `claude agents|attach|stop|logs|rm|respawn|auth --help`, `claude auth status` (2.1.311).
  - Live runs (scripts and captures in the session scratchpad, test transcripts deleted afterwards).
  - `agents/bin/agent`, `agents/hr/hr.py` (`session_command`).
  - App repo: `origin/spike/s1-s2:spikes/s2-sessions/README.md`, `spikes/s1-status/README.md`; `origin/v1:docs/adr/0001-app-is-a-client-office-is-a-server.md`, `0003-agent-messaging-office-first.md`.
- Claude Code:
  - [Run Claude Code programmatically](https://code.claude.com/docs/en/headless)
  - [Manage sessions](https://code.claude.com/docs/en/sessions)
  - [Agent SDK overview](https://code.claude.com/docs/en/agent-sdk/overview)
- Codex:
  - [Non-interactive mode](https://learn.chatgpt.com/docs/non-interactive-mode)
  - [App server](https://learn.chatgpt.com/docs/app-server)
  - [AGENTS.md guide](https://developers.openai.com/codex/guides/agents-md)
  - openai/codex at tag `rust-v0.167.0`: `codex-rs/exec/src/cli.rs`, `exec/src/exec_events.rs`, `exec/src/lib.rs`, `cli/src/main.rs`, `cli/src/queue_cmd.rs`, `utils/cli/src/shared_options.rs`, `app-server-protocol/src/protocol/v2/thread.rs`, `core/config.schema.json`, `rollout/src/lib.rs`, `hooks/src/lib.rs`, `sdk/typescript/README.md` ([repo](https://github.com/openai/codex))
- Antigravity:
  - [Headless mode](https://antigravity.google/docs/cli/headless/)
  - [CLI commands](https://antigravity.google/docs/cli/reference/)
  - [ACP](https://antigravity.google/docs/cli/acp/)
  - [Managing conversations](https://antigravity.google/docs/cli/conversations/)
  - [Customization discovery](https://antigravity.google/docs/customization-discovery/)
  - [Lifecycle hooks](https://antigravity.google/docs/hooks/)
  - [CLI AI credits](https://antigravity.google/docs/cli/credits/)
  - [SDK overview](https://antigravity.google/docs/sdk/overview/)
  - [CHANGELOG](https://github.com/google-antigravity/antigravity-cli/blob/main/CHANGELOG.md)
  - [ACP spec overview](https://agentclientprotocol.com/protocol/overview)
- Third-party, leads only:
  - [agy-acp spike, issue #49](https://github.com/lucasschirm/claude-marketplace/issues/49) (agy 1.4.2)
  - [station issue #209](https://github.com/kontourai/station/issues/209)
  - [Forum: third-party tools calling agy headless](https://discuss.ai.google.dev/t/is-it-allowed-for-an-open-source-tool-to-call-the-official-agy-cli-in-headless-mode/183429)
