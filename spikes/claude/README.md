# Spike S0 (Claude): driving Claude Code headless from the office

Claims come from runs on 2026-11-08, except those tagged **[binary]** or **[--help]** below, which were read (from the CLI binary or `claude --help`), not run.

Evidence tags:
- **(no tag):** recorded by a script, in `results/` or `fixtures/`.
- **[hand]:** observed by hand during the spike (`ls`, `ps`, a shell check), not saved by a script.
- **[binary]:** read from the CLI binary (`strings`), not run.
- **[--help]:** from `claude --help`.

- **CLI:** `claude` 2.1.311 (`/opt/homebrew/bin/claude` → `/opt/homebrew/Caskroom/claude-code/2.1.311/claude`)
- **Host:** macOS 26.7.2 (Darwin 25.6.0, arm64), Python 3.15.2, git 2.54.0 (Apple Git-157)
- **Model:** `--model haiku` (`claude-haiku-5-5`), `--max-budget-usd 0.50` per process
- **Model turns used:** about 55-60, including 1 interactive turn **[hand]** (counted with a run log that was later deleted; the scripts' own docstrings add up to the lower end). A one-line haiku turn reported about $0.002 `total_cost_usd` (list-price estimate).

## Summary

| # | Item | Verdict | The one fact the adapter needs |
| --- | --- | --- | --- |
| 0 | Isolated auth | **pass** (undocumented) | `CLAUDE_CONFIG_DIR=<agent dir>` plus `CLAUDE_SECURESTORAGE_CONFIG_DIR=` (set, empty) authenticates with the user's keychain login, and everything else (transcripts, settings, skills, plugins) lives in the agent's dir, except the two empty folders in fact 11. `CLAUDE_CONFIG_DIR` alone is logged out. |
| 1 | Long-lived stream-json | **pass** | One JSON line per user turn. Each turn: `system/init`, `assistant`/`user` events, then exactly one `result`. Close stdin when idle: exit 0 in 0.3 s. |
| 2 | Approvals (`can_use_tool`) | **pass**, with limits | Needs `--permission-prompt-tool stdio`: without it no `can_use_tool` arrived, and the one prompting command tried was denied. Answer with `control_response` `{behavior:"allow"}` or `{behavior:"deny",message}`. **No CLI timeout within 150 s** (longer not tested): the office owns expiry. |
| 3 | Interrupt | **pass** | `control_request {subtype:"interrupt"}` is acked in about 2 ms. The turn ends at once with `result` `error_during_execution` (`aborted_tools` or `aborted_streaming`). A running tool is killed, and the process stays usable. |
| 4 | Resume | **pass**, with limits | `--resume <id>` worked from another cwd, including after the original cwd was deleted. The **system prompt is snapshotted** (agent prompt and appended file), so the new text is ignored. `--settings` is **not** restored, so it must be replayed. |
| 5 | Isolation | **pass** with `--setting-sources ""` | `""` (empty string) loads no settings file, no `CLAUDE.md` (user, project or local, **cwd included**), and no user or project skills, agents or commands. **Auto-memory (`MEMORY.md`) still loads**, unless the office's `--settings` has `autoMemoryEnabled: false`. `none` is rejected. `--safe-mode` is **not** isolation. A workspace skill's `allowed-tools` **does** pre-approve tools, but only when the project source is loaded. |
| 6 | Inline agent identity | **pass** | `--agents '<json>' --agent <name>`: its prompt and `tools` apply to the main thread. **An unknown `--agent` falls back silently** to the default, and `init` has no field naming the active agent, so verify it against the agent list in `initialize`. |
| 7 | Tool detachment | **fact that changes the design** | Each Bash tool call runs in **its own session and process group** (`setsid`). Killing claude's group leaves the tool tree running, reparented to launchd. SIGTERM to claude does kill the tool trees. |
| 8 | Sandbox and office socket | **pass** | `sandbox.network.allowUnixSockets: [<path>]` allows exactly that path, **even one that didn't exist when claude started** (it was created before the turn). Unlisted sockets are refused (`EPERM`). With no list, every socket is refused. |
| 9 | git commit in the sandbox | **pass**, but roots differ from the design | Claude's sandbox **by itself** makes much of the linked worktree's git common dir writable (refs, `HEAD`, `packed-refs`, `info/exclude`, …), but not `config`, `config.worktree`, `hooks/` or `objects/info`. The office's `denyWrite` entries override that, and the commit still works. `git config --worktree` was denied in both sandbox runs. |
| 10 | Take-over | **pass** | Headless → `claude --resume <id>` in a PTY → headless `--resume`: same session id, one transcript, and the conversation carries both ways. A fresh config dir must be pre-seeded (onboarding, trust). |
| 11 | Memory | **measured** | Idle RSS is 216 MiB after `initialize` with no turn yet (210 MiB in an unscripted probe **[hand]**), 228 MiB after 1 turn, and 238 MiB after 4 turns. |

## Facts that contradict or change `agent-sessions.md` (rev 3.1)

1. **Process groups (decision 2, "Processes and safety", S6 orphans).** Claude puts every Bash tool command in a **new session and process group**: `sid == pgid == the tool's zsh pid`, a direct child of claude. So:
   - "SIGKILL the group" (the last step of the stop sequence) and orphan-cleanup step 1 ("the group is killed") **don't reach tools**. They survive, reparented to pid 1.
   - Orphan step 2 ("list the group's members") never sees tools either.
   - So only step 3 (the `ORG_SESSION_MARK` environment scan) would find them (an inference from the process trees; the office's scan wasn't run). Tools do inherit claude's environment: our allowlisted variables reached `env` inside the tool.
   - SIGTERM to claude (not to the group) killed every tool tree and exited 143, so the stop sequence's SIGTERM step works (SIGINT wasn't tested). But a SIGKILL'd or crashed claude always leaves its running tools behind.
   - The office could also record the session ids of claude's direct children.
2. **Where the conversation lives (decision 3, decision 6).** The design assumes `~/.claude/projects/…` and the user's own config dir. Instead, the office can give each agent its own `CLAUDE_CONFIG_DIR` (e.g. `<runtime dir>/claude/`), and still use the user's sign-in through `CLAUDE_SECURESTORAGE_CONFIG_DIR=` (empty).
   - Then the user's `~/.claude` (`CLAUDE.md`, skills, agents, plugins, settings, history) never loads into an agent, and transcripts, auto-memory and shell snapshots are per agent.
   - **But the account's synced claude.ai plugins still load** (downloaded into the agent's config dir) whenever the `user` setting source is on, which includes the default. **[hand]**, seen before the privacy re-scrub removed their names from the fixtures. Only `--setting-sources ""` kept them out (item 5).
   - The catch: the variable is **undocumented** (found in the binary, verified by runs). See Open questions.
   - With a per-agent dir, the "transcripts are plaintext" exception (U-S3) moves from `~/.claude` to the agent's runtime dir.
3. **The runtime reads no instruction file from the workspace or its parents (decision 8, "Security").** Under `--setting-sources ""`, the model saw **none** of these (items 5 and 5c):
   - the cwd's `CLAUDE.md` and `CLAUDE.local.md`
   - a `CLAUDE.md` in the cwd's parent (a home inside a repo)
   - `AGENTS.md`, `.agents/rules/*.md` and `.claude/rules/*.md`
   - a `CLAUDE.md` in an `--add-dir` folder
   - the user dir's `CLAUDE.md`

   The parent, `AGENTS.md`, rules and `--add-dir` markers were checked with no tools (`--tools ""`) and again with the default 28 tools (`--with-tools`), with the same result.

   So, for Claude, the task branch's instruction files don't steer the session, and the design's sentence about runtimes reading cwd instruction files should except Claude.
   - **Not tested:** a nested `CLAUDE.md` that Claude may load lazily when the agent reads files in a subfolder.
   - The one exception is **auto-memory** (fact 14).
4. **The sandbox's writable roots for a task worktree ("Git in worktrees").** Claude's own sandbox adds the linked worktree's git common dir (`work.git`) **by itself**, with built-in denies. With only the cwd configured:
   - **Allowed:** `refs/heads/<any branch>`, `HEAD` (the bare repo's), `packed-refs`, `info/exclude`, `description`, `FETCH_HEAD`, `logs/`, `objects/`, and `worktrees/t1/{HEAD,index,gitdir}`.
   - **Denied:** `config`, `config.worktree` (both levels), `hooks/`, `objects/info/`, `worktrees/t1/commondir`, the worktree's `.git` file, `.claude/**` in the cwd, and an unrelated sibling repo.
   - The design already makes all of `work.git/refs/` writable and lists "a hostile agent could move another task's branch" as a known limit, so the refs aren't new.
   - **What is new:** the sandbox by itself also allows `HEAD` (the bare repo's), `packed-refs`, `info/exclude`, `description`, `FETCH_HEAD` and `worktrees/<id>/gitdir`. The design treats `info/exclude` (decision 6) and `gitdir` ("Git in worktrees") as read-only, and doesn't list the others as writable.
   - Those claims hold **only if the office adds `denyWrite` entries**, which do take precedence (verified in B3), and a commit still works with them.
   - Whether a `denyWrite` on `refs/heads` plus an `allowWrite` on the agent's own branch can narrow refs to "only my branch" is **untested**.
5. **`git config --worktree` is denied** by Claude's sandbox even when `work.git/worktrees/<id>` is in `allowWrite`: `could not lock config file …/config.worktree: Operation not permitted`. That's stricter than the design ("an agent's `git config --worktree` writes to its own config.worktree"), and harmless.
6. **No CLI-side approval timeout was seen within 150 s** (decision 7). An unanswered `can_use_tool` waited 150 s with no reaction. A CLI timeout somewhere between 150 s and 30 minutes would race the office's expiry, and that wasn't tested. The office's 30-minute expiry must answer the request itself, with a deny or an interrupt. On interrupt the CLI sends `control_cancel_request` for the pending id.
7. **`deny` with `interrupt: true` hides the reason from the model.** The model gets a generic "The user doesn't want to proceed…" and the turn ends. Use a plain `deny` with a `message` when the agent should see why.
8. **`allow` with `updatedInput` runs the rewritten input, and the model isn't told.** It reported running the original command. Don't rely on silent rewrites.
9. **Claude injects its own variables into every tool's environment**, beyond the office allowlist: `CLAUDECODE`, `AI_AGENT`, `CLAUDE_PID`, `CLAUDE_CODE_SESSION_ID`, `CLAUDE_CODE_CHILD_SESSION`, `CLAUDE_CODE_ENTRYPOINT`, `CLAUDE_CODE_EXECPATH`, `CLAUDE_CODE_SESSION_ATTENDED`, `CLAUDE_EFFORT`, `GIT_EDITOR`, `COREPACK_ENABLE_AUTO_PIN`, and **`CLAUDE_CODE_MESSAGING_SOCKET` / `CLAUDE_CODE_MESSAGING_TOKEN`**.
   - The messaging token is a secret an agent's tools can print, so add it to the redaction list.
   - Each claude process listens on `/tmp/cc-socks/<pid>.sock` (`init.messaging_socket_path`), a machine-wide directory shared with the user's own interactive sessions **[hand]**: the user's live sessions' sockets were in the same folder.
     - Mode 0600 **[hand]**.
     - Removed on a clean exit and after SIGTERM **[hand]**.
     - **Left behind after SIGKILL** (`results/item07.json` `R1_cc_socket_left_after_sigkill: true`), so `org-supervise` should remove it after reaping.
   - Under the sandbox (no `allowUnixSockets` entry for it), a tool can't connect even to its own messaging socket.
10. **A shell snapshot is sourced before every Bash command** (item 7's process tree: `/bin/zsh -c source <cfg>/shell-snapshots/snapshot-zsh-….sh …`).
    - **[hand]:** this run's snapshot held no aliases, `export PATH=…` equal to the allowlisted `PATH`, and Claude's own wrapper functions: `find` and `grep` shadowed by the bundled bfs and ugrep, `rg`, and a `pkill` that refuses patterns matching the CLI.
    - **Risk, not observed:** it's built from the user's `$SHELL` with the real `HOME`, so the user's rc aliases and functions could reach agents (see Open questions).
11. **The CLI writes into the real `~/.claude` regardless of `CLAUDE_CONFIG_DIR`** (see "Paths written"). A sandbox-enabled session created the empty folders `~/.claude/ssh` and `~/.claude/bridge-spawn` (**[hand]**: attributed by their birth times, 07:38:20 and 07:38:23, 2-5 s after the first sandboxed run started). `bridge-spawn` is hard-coded as `join(homedir(), ".claude", "bridge-spawn")` **[binary]**. The OAuth refresh lock also lives in `~/.claude` when `CLAUDE_SECURESTORAGE_CONFIG_DIR=` is empty **[binary]**, though no lock file was seen.
12. **Claude Code blocks long foreground sleeps** in Bash (`Blocked: sleep 47 followed by: echo done. To wait for a condition, use Monitor…`). **[hand]:** seen in a first item 3 run whose fixture the later run overwrote. Agents get steered to `run_in_background` and Monitor, which can start turns after a `result`. Busy/idle tracking must keep treating a new `system/init` as "busy again" (research §2.4).
13. **`--safe-mode` doesn't isolate:** every settings file's `env` still applied (user, project and local; seen by the tool's `env`).
14. **Auto-memory is still on under `--setting-sources ""`, and loads into context** (item 5c).
    - A `MEMORY.md` planted in `<config dir>/projects/<cwd slug>/memory/` was seen by the model.
    - It's an agent-writable instruction channel that persists across sessions with the same cwd, which cuts against decision 8 ("what an agent writes … never changes what the office tells the next session").
    - **`--settings '{"autoMemoryEnabled":false}'` turns it off:** `memory_paths: null`, and the marker wasn't seen (`results/item05c-no-memory.json`). Under `--restricted`, `init` also showed `memory_paths: null`; whether a `MEMORY.md` would load there wasn't tested.
    - `CLAUDE_CODE_DISABLE_AUTO_MEMORY` exists **[binary]** (not tested).
15. **A turn aborted mid-stream reports no usage in its `result`.**
    - In item 3's B, `result.usage` is all zeros, and `total_cost_usd` (0.00023587) and `duration_api_ms` (1089) repeat A's cumulative values.
    - That API call's usage (input 2, cache_creation 57, cache_read 19430, output 2) still appears on the turn's `assistant` events (`message.usage`) and in the `stream_event` `message_delta.usage`.
    - The `assistant` copy is a `message_start` snapshot, so its `output_tokens` can undercount. Only the final output count needs `message_delta`, which needs `--include-partial-messages`.
    - A turn interrupted during a tool (A) reported its usage normally in `result`.
    - So quota accounting should sum `assistant` (or `message_delta`) usage for aborted turns, not rely on their `result`.

## How to run

```
cd spikes/claude
python3 item00_auth.py              # item 0 (no model call)
python3 item01_stream.py            # items 1 and 11
python3 item02_approvals.py 150     # item 2 (arg: seconds to leave a request unanswered)
python3 item02b_approvals_fresh.py  # item 2, deny variants in fresh conversations
python3 item03_interrupt.py         # item 3
python3 item04_resume.py            # item 4
python3 item05_isolation.py         # item 5
python3 item05b_skill_control.py    # item 5 control (plants item05's markers itself)
python3 item05c_instruction_files.py [--no-memory] [--with-tools]  # item 5: other instruction files, auto-memory
python3 item06_agent_identity.py    # item 6
python3 item07_detach.py            # item 7
python3 item08_09_sandbox.py [--dry]  # items 8 and 9 (--dry: git setup and probe, no sandbox, no model)
python3 item09b_sandbox_gitdir.py [--deny]  # item 9: what the sandbox allows by itself / with denyWrite (builds its own repos)
python3 rescrub.py                  # offline: re-apply the scrubber to every fixture and result (no model call)
python3 item10_takeover.py          # item 10
```

- `lib.py` holds the harness: the isolated env, stream-json framing, control requests, process-group kill, scrubbed fixture writer.
- Each script writes `results/<item>.json` (scrubbed) and `fixtures/<scenario>.jsonl`. Fixture lines are `{"dir":"in"|"out","t_ms":<ms since spawn>,"msg":<the JSON line>}`, with uuids replaced by stable `<uuid-N>`, temp paths by `<tmp>`, and personal fields by `<scrubbed>`.
- Every run uses a private `tempfile.mkdtemp` dir, `/tmp/claude-spike-<item>-XXXXXXXX/` (0700), with `cfg/`, `ws/` and `tmp/` in it. It's removed at exit unless you pass `--keep` or set `CLAUDE_SPIKE_KEEP=1`.
- Each script prints the number of model turns it sent, to stderr.
- **Tool approvals are exact (in the current scripts):** `lib.approver` answers every `can_use_tool` request. It allows only the scenario's own tool call (`tool_name == "Bash"` and `input.command` equal to the expected string, never with `dangerouslyDisableSandbox`), and denies everything else, printing it to stderr. Scenarios that expect no tool deny every request. **Re-runs** record the denials as `denied_unexpected_calls`; the existing results predate that field.
- **What runs without reaching the approver:**
  - the tools the CLI auto-allows itself (e.g. read-only tools such as Read inside the workspace)
  - the exact allow rules in items 3 (`--allowedTools "Bash(sh wait47.sh)"`), 4 (`touch ok-r1.txt` … `ok-r4b.txt`), 8/9 and 9b (`sh <probe>`)
  - in item 5, the planted workspace skill's `allowed-tools` (the very thing it tests). That rule is now narrowed to `Bash(touch SKILL_RAN.txt)`; the recorded run used `Bash(touch *)`.
- **Default setting sources:** items 1, 2, 2b, 3, 5 (its `default` variant) and 7 run with the default sources, so the account's synced claude.ai plugins, and any hooks they carry, load into them. For re-runs where isolation isn't what's being tested, adding `--setting-sources ""` is recommended. That wasn't re-run here.
- `item02`'s scenario answers (allow, rewrite, the deny variants, no answer) apply only to the scenario's exact command.
- **Running item 8/9 makes the CLI create the empty folders `~/.claude/ssh` and `~/.claude/bridge-spawn`** (fact 11). `lib.py` removes them again at exit, but only if this process spawned a sandbox-enabled session, didn't find them at start, and they're still empty.
- **Changes made after the recorded runs** (from the independent review). These change cleanup and the selection logic only, not what was measured:
  - `lib.Session.close` and the exit hook now SIGTERM claude before the group SIGKILL, because tools live in their own sessions.
  - The exit hook also kills the item 10 PTY child.
  - `item07` matches the sleeps by session id under our claude, so it can't hit unrelated `sleep` processes. It kills a survivor only if its pid, command and session id all match the snapshot, and removes R2's and R3's leftover `/tmp/cc-socks/<pid>.sock` as well as R1's.
  - Every script now approves only its scenario's exact command (`lib.approver`). The recorded runs allowed any `can_use_tool` in items 3, 4, 5, 7 and 8/9, and in item 2's scenarios. Every request those runs recorded was the scenario's own command: the fixtures (items 2, 4 and 5; items 3 and 8/9 got none), plus item 7's process trees and stdout tail, which show only `sh hold.sh` (item 7 saved no fixture).
  - `item02`'s P2 now stops through `close()` instead of reaping and then group-killing.
  - Temp dirs come from `mkdtemp` and are removed at exit. `item05b` and `item09b` build their own setup (`item05_setup.py`, `gitfixture.py`) instead of reading another script's fixed `/tmp` dir.
  - `item05` records only built-in counts, marker names and a count of the rest, not the account's synced command names.
  - `Session.stop` reads the child's state with `waitid(WNOWAIT)` (running, zombie or reaped), always checks at least once, sends SIGTERM with `os.kill` (`Popen.terminate` polls and could reap), and never group-kills a leader that's already reaped.
  - `item07` removes the leftover sockets while each claude is still an unreaped zombie, so a recycled pid's socket in the shared `/tmp/cc-socks` can't be hit.
  - The follow-up "ALIVE" turns in items 2 and 2b also run under a deny-all approver.
  - `item02` takes its wait time from the first numeric argument, so `--keep` works there too.
  - `item10` crops the TUI notice lines with `lib.crop_tui_notices`, for the fixture and for `pty_tail`.
  - `lib` installs SIGTERM and SIGHUP handlers that exit normally, so the atexit cleanup (kill children, remove temp dirs) still runs. `item10` has a 600 s wall-clock limit (`lib.deadline`) that also exits through the cleanup.
  - `item10`'s forked child calls `os._exit(127)` if `chdir` or `execve` fails, so it never runs the parent's code.
  - `item07` R2 removes the leftover socket only if that claude has exited.
  - `item09b` now passes the office git env (`gitfixture.git_env`) to claude. **The recorded B2/B3 runs didn't**, so the B3 commit took its identity from the user's global git config. That was a read, not a write. Each run also builds fresh repos now, so B3 no longer inherits B2's leftovers.
  - `item07` R1 always sends the group kill.
  - `item03` B now waits for a `text_delta`.
  - `item02` saves its in-conversation deny variants as `approval-inconv-*`, so they no longer overwrite item02b's fixtures.
  - The exit hook signals only live, unreaped children, so a reused pid or process group is never hit.
  - The scrubber replaces emails and tokens before `$USER`.
  - `Session.stop` waits for claude without reaping it (`waitid` with `WNOWAIT`), SIGKILLs the group while the leader is still a zombie, so its group id can't be recycled, and then reaps. On macOS, `killpg` on a group holding only a zombie returns EPERM, which is treated as "already gone".

**The isolated env** (`lib.clean_env`), built from an allowlist with no inherited `CLAUDE_*` or `AI_AGENT`:

```
PATH=/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin  HOME=<real home>  USER  LOGNAME
LANG=en_US.UTF-8  SHELL=/bin/zsh  TMPDIR=<tmp>/tmp  DISABLE_AUTOUPDATER=1
CLAUDE_CONFIG_DIR=<tmp>/cfg  CLAUDE_SECURESTORAGE_CONFIG_DIR=
```

**The base argv** (`lib.stream_args`):

```
claude -p --input-format stream-json --output-format stream-json --verbose --model haiku
  --max-budget-usd 0.50 --strict-mcp-config --mcp-config '{"mcpServers":{}}'  [+ per item]
```

---

## 0. Authentication in isolation

Script: `item00_auth.py` (no model call). Result: `results/item00.json`.

- `CLAUDE_CONFIG_DIR=<tmp>/cfg claude auth status` → `"loggedIn": false`, `"authMethod": "none"`, exit 1.
- **[binary]** The keychain service name is `"Claude Code-credentials" + ("" | "-" + sha256(dir)[:8])`. The suffix is left off only when `CLAUDE_CONFIG_DIR` is unset, or when `CLAUDE_SECURESTORAGE_CONFIG_DIR` is set to the empty string.
- `CLAUDE_CONFIG_DIR=<tmp>/cfg CLAUDE_SECURESTORAGE_CONFIG_DIR= claude auth status` → `"loggedIn": true, "authMethod": "claude.ai", "configDirectory": "<tmp>/cfg"`, exit 0. Every model run below authenticated this way (`apiKeySource: "none"`).
- `.claude.json`, `projects/` (transcripts), `plugins/` (account-synced plugins), `shell-snapshots/`, `session-env/` and `backups/` were all written into `<tmp>/cfg` **[hand]**.
- Side effects outside the dir: see fact 11.
- **Verdict: pass**, but it rests on an undocumented variable (Open questions).
- The office should run `claude auth status` (no model call) as a preflight.

## 1. Long-lived stream-json

Script: `item01_stream.py`. Fixtures: `stream-json-multi-turn.jsonl`, `stream-json-queued.jsonl`.

```
claude -p --input-format stream-json --output-format stream-json --verbose --model haiku \
  --max-budget-usd 0.50 --strict-mcp-config --mcp-config '{"mcpServers":{}}' --permission-mode manual
```

**In** (one JSON line each):

```json
{"type":"control_request","request_id":"host-1","request":{"subtype":"initialize"}}
{"type":"user","message":{"role":"user","content":"Reply with exactly the word ONE and nothing else."}}
```

**Out:**

- **Before any user line:** if `initialize` was sent, a `control_response` arrives about 0.6 s after spawn. Its `response` has `commands`, `agents`, `models`, `account`, `capabilities`, `current_permission_mode`, `pid`, … It costs no model call, and two `system/commands_changed` events follow. **No `system/init` is sent until the first user line.**
- **Per turn:**
  - `system/init`, re-sent at **every** turn, about 40 ms after the user line. It carries `session_id`, `cwd`, `tools`, `mcp_servers`, `model`, `permissionMode`, `agents` (the names of the available agents, **not** which one is active; there's no field for that), `skills`, `plugins`, `slash_commands`, `capabilities`, `memory_paths`, `messaging_socket_path`, `claude_code_version`, `apiKeySource`, …
  - `assistant` events: **one per content block**. A thinking block and a tool_use block from the same API message arrive as two events with the same `message.id`. Each has `message.usage`.
  - `user` events with `tool_result` blocks, plus `tool_use_result` (structured) and `tool_result_meta[].permission_decision` (e.g. `{"decision":"accept","source":"config","reason_type":"mode"}`, or `source:"user_temporary"` when the host allowed it).
  - `rate_limit_event` (5-hour and 7-day `utilization`, `resetsAt`).
  - `result`.
- **`result`** has `subtype` (`success` / `error_during_execution`), `is_error`, `result` (the text; `null` on error), `stop_reason`, `terminal_reason` (`completed`, `aborted_tools`, `aborted_streaming`), `num_turns`, `result_index` (0, 1, 2, … per process), `queued_turn_count`, `total_cost_usd` (**cumulative per process**), `usage`, `modelUsage`, `permission_denials`, `subagent_stats`, `session_id`.
- **The next prompt** is just another user line after the `result`.
- **Two lines written back to back** ran as two sequential turns, each with its own `init` and `result`. `queued_turn_count` was **0 on both**, so it isn't a reliable queue indicator.
- `--permission-mode manual` shows as `"permissionMode":"default"` in `init`.
- **Closing stdin while idle:** exit 0 after 0.33 s, nothing on stderr. The `/tmp/cc-socks/<pid>.sock` is removed **[hand]**.
- **Verdict: pass.**

## 2. Approvals

Scripts: `item02_approvals.py`, `item02b_approvals_fresh.py`. Fixtures: `approval-{allow,allow-rewrite,deny-message,deny-interrupt,deny-invalid,error-response,no-answer,pending-stdin-closed,no-prompt-tool-flag}.jsonl`.

Flags: base plus `--permission-prompt-tool stdio --permission-mode manual`. (`--permission-prompt-tool` is a hidden flag **[binary]**. `--permission-prompts host` is the default **[--help]**.)

**Request** (CLI → host):

```json
{"type":"control_request","request_id":"<uuid>","request":{"subtype":"can_use_tool","tool_name":"Bash","display_name":"Bash",
 "input":{"command":"echo nope > deny.txt","description":"Write nope to deny.txt"},"description":"Write nope to deny.txt",
 "permission_suggestions":[{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"echo nope *"}],"behavior":"allow","destination":"localSettings"},
                          {"type":"addDirectories","directories":["<tmp>/s2/ws"],"destination":"session"}],
 "blocked_path":"<tmp>/s2/ws/deny.txt","tool_use_id":"toolu_…"}}
```

**Response** (host → CLI): `{"type":"control_response","response":{"subtype":"success","request_id":"<same uuid>","response":<decision>}}`. The schema **[binary]** is `{behavior:"allow", updatedInput?, updatedPermissions?, toolUseID?, decisionClassification?}` or `{behavior:"deny", message, interrupt?, toolUseID?, decisionClassification?}`.

| Scenario | Decision sent | Observed |
| --- | --- | --- |
| allow | `{"behavior":"allow"}` (no `updatedInput`) | Ran; file written; `permission_decision.source:"user_temporary"`. |
| allow-rewrite | `{"behavior":"allow","updatedInput":{…,"command":"echo rewritten > rewritten.txt"}}` | The **rewritten** command ran (`rewritten.txt` exists, `original.txt` doesn't); the model reported running the original. |
| deny-message | `{"behavior":"deny","message":"Office policy: …"}` | `tool_result` `is_error:true` with the message **verbatim**; the turn continues; `result` `success`; `permission_denials` lists it. |
| deny-interrupt | `{"behavior":"deny","message":"Stopped by the owner","interrupt":true}` | `tool_result` gets the generic "The user doesn't want to proceed…" (**the message isn't shown**), plus a user text `[Request interrupted by user for tool use]`; `result` `error_during_execution`, `terminal_reason:"aborted_tools"`. The next turn works. |
| deny-invalid | `{"behavior":"deny"}` (no message) | Treated as a deny: "The canUseTool callback returned an invalid permission result. Expected …". |
| error-response | `{"subtype":"error","request_id":…,"error":"office: approval store unavailable"}` | Deny: "Tool permission request failed: Error: office: approval store unavailable". |
| no-answer | nothing for 150 s, then `interrupt` | **No timeout in 150 s.** On interrupt the CLI sends `{"type":"control_cancel_request","request_id":"<pending uuid>"}`, then the turn ends `error_during_execution` / `aborted_tools`, with the call in `permission_denials`. The next turn works (`ALIVE`). |
| pending, then stdin closed | stdin closed with a request open | Auto-deny "Tool permission stream closed before response received". **The model still finishes the turn** (one more API call), then exit 0 after 1.5 s. |
| no `--permission-prompt-tool` | (none) | **No `control_request`.** The write was denied ("Output redirection … needs approval"). |

- **In-conversation effect:** after the first deny, the model refused later, similar writes **by itself**, without sending `can_use_tool` (seen in P1). That's why the deny variants were re-run in fresh processes.
- **Verdict: pass** for the round trip. Limits: no built-in expiry; `interrupt:true` drops the message; rewrites are invisible to the model.

## 3. Interrupt

Script: `item03_interrupt.py`. Fixtures: `interrupt-during-tool.jsonl`, `interrupt-during-text.jsonl`.

Flags: base plus `--permission-mode manual --permission-prompt-tool stdio --allowedTools "Bash(sh wait47.sh)" --include-partial-messages`.

- **Send:** `{"type":"control_request","request_id":"host-1","request":{"subtype":"interrupt"}}`. The ack `{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"still_queued":[]}}}` arrives in 1-2 ms.
- **During a running Bash tool** (`sh wait47.sh` running `sleep 47`, seen in `ps`):
  - `system/task_notification` arrives with `status:"stopped"`, followed by a `tool_result` "The user doesn't want to proceed with this tool use…" (`is_error`) and a user text `[Request interrupted by user for tool use]`.
  - Then `result` `error_during_execution`, `terminal_reason:"aborted_tools"`, about 10 ms after the interrupt.
  - **The `sleep` was gone** 2 s later.
  - A `system/task_started` (`task_type:"local_bash"`) marks the tool's start.
- **During streaming (case B):** the recorded run interrupted during the **thinking** block. The first text delta ("The l") arrived 2 ms after the interrupt, because the predicate then matched any `content_block_delta`; the script is fixed for re-runs.
  - A final `assistant` event carries what had streamed, then a user text `[Request interrupted by user]`, then `result` `error_during_execution`, `terminal_reason:"aborted_streaming"`, about 10 ms after the interrupt.
  - That `result`'s `total_cost_usd` didn't grow (fact 15).
- **After either case**, the next user line is answered normally (`AFTER`).
- **Interrupt while idle:** it's acked the same way and no `result` follows.
- **`--include-partial-messages` adds** `stream_event` (`message_start`, `content_block_start/delta/stop`, `message_delta`, `message_stop`), `system/status` (`requesting`) and `system/thinking_tokens`.
- The `--allowedTools "Bash(sh wait47.sh)"` rule pre-approved the call: no `can_use_tool` was sent.
- The first Bash call of a process started about 3 s after its `tool_use` event (`system/task_started` at +3.1 s), probably the shell snapshot being taken. An interrupt inside that gap rejects the call before it starts **[hand]**: that happened in one earlier attempt (its fixture was overwritten), which I first misread as an unanswered approval.
- A first attempt with a bare `sleep 47` was **blocked by Claude Code** itself (fact 12).
- **Verdict: pass.**

## 4. Resume

Script: `item04_resume.py`. Fixtures: `resume-R0-new.jsonl` … `resume-R4b-cwd-recreated.jsonl`.

Spec (base plus): `--setting-sources "" --settings <file: permissions.allow Bash(touch ok-*) in the recorded run; the script now lists exact Bash(touch ok-r1.txt) … Bash(touch ok-r4b.txt)> --permission-mode manual --permission-prompt-tool stdio --agents '{"zephyr":{…,"prompt":"…prefix ZEPHYR-V<n>:"}}' --agent zephyr --append-system-prompt-file <instructions-v<n>.md: codeword ALPHA|BRAVO>`

| Run | Launch | Reply | Approval asked for `touch ok-*`? |
| --- | --- | --- | --- |
| R0 | spec v1, `--session-id <uuid>` | `ZEPHYR-V1: OK. I'll remember the word PELICAN.` | — |
| R1 | spec **v2**, `--resume` | `ZEPHYR-V1: Codeword ALPHA, remembered word PELICAN` → **v1 snapshot kept** (agent prompt and appended file) | no (settings replayed) |
| R2 | `--resume` with **no** `--settings/--agents/--agent/--append` | `ZEPHYR-V1: Codeword ALPHA …` → identity kept by the snapshot | **yes** → `--settings` is not restored |
| R3 | spec v2, `--resume --system-prompt-snapshot off` | `ZEPHYR-V2: Codeword BRAVO …` (the model also "corrected" its earlier replies) | no |
| R4a | original cwd **deleted**, spec v2, `--resume`, cwd = another dir | `ZEPHYR-V1: Codeword ALPHA, remembered word PELICAN`; `init.cwd` = the new dir; the file landed there | no |
| R4b | cwd re-created empty at the same path, spec v2, `--resume` | `ZEPHYR-V1: Codeword ALPHA …` | no |

- `session_id` stayed the same in every run. There's one transcript file, in the **original** cwd's project folder (`cfg/projects/-private-tmp-claude-spike-s4-ws/<id>.jsonl`), even for the resume from another cwd.
- `--system-prompt-snapshot off` applies **to that launch only**. The next default launch (R4) went back to the v1 record, so the record isn't replaced.
- **Verdict: pass.** Replay the full spec every time, and send instruction changes as a message (as the design says), or pass `off` on **every** launch.

## 5. Isolation

Scripts: `item05_isolation.py`, `item05b_skill_control.py`. Fixtures: `isolation-*.jsonl`.

**Markers** (never in the real `~/.claude`):
- **User** (= the isolated `CLAUDE_CONFIG_DIR`): `CLAUDE.md` (`USER-MARKER-U7`), `settings.json` (env `SPIKE_USER_ENV`, deny `WebSearch`), `skills/user-skill-u7`, `agents/user-agent-u7.md`, `commands/user-cmd-u7.md`.
- **Workspace:** `CLAUDE.md` (`PROJ-MARKER-P7`), `CLAUDE.local.md` (`LOCAL-MARKER-L7`), `.claude/settings.json` (env `SPIKE_PROJECT_ENV`, deny `WebFetch`), `.claude/settings.local.json` (env `SPIKE_LOCAL_ENV`, deny `NotebookEdit`), `.claude/skills/proj-skill-p7` (`allowed-tools: Bash(touch *)` in the recorded run; now `Bash(touch SKILL_RAN.txt)`), `.claude/agents/proj-agent-p7.md`, `.claude/commands/proj-cmd-p7.md`, `.mcp.json`.

| Flags | Settings env / denies applied | `CLAUDE.md` markers the model saw | Skills / agents / commands loaded | Notes |
| --- | --- | --- | --- | --- |
| (none) | user, project, local | U7, P7, L7 | user and project markers | everything loads |
| `--setting-sources ""` | **none** | **NONE** | **none** | built-in plugins/skills only; auto-memory still on (in the config dir) |
| `--setting-sources user` | (initialize only) | — | user markers **plus the account's synced claude.ai plugins**, downloaded into `<cfg>/plugins/synced/` **[hand]** | **[hand]:** synced plugins load whenever the `user` source is on, which includes the default; they appeared in the default runs' `init.plugins`. The privacy re-scrub removed them, so neither the plugins nor that count is in `results/` or `fixtures/` any more. |
| `--setting-sources project` | (initialize only) | — | project markers | |
| `--setting-sources none` | — | — | — | **rejected**: `Invalid setting source: none. Valid options are: user, project, local`, exit 1 |
| `--restricted` | env can't be checked (no Bash); the WebSearch and NotebookEdit denies were **not** applied (both tools present) | NONE | none | **no Bash** (`NOBASH`), no WebFetch; `memory_paths: null` (whether a `MEMORY.md` would load wasn't tested; `--safe-mode` and `restricted+tools-bash` also showed `memory_paths: null`) |
| `--restricted --tools Bash,Read,Edit,Write,Glob,Grep,Skill` | none | NONE | none | Bash back; only the listed tools |
| `--safe-mode` | **user, project, local** env applied (seen by `env`). WebSearch, WebFetch and NotebookEdit absent, but that can't be told apart from what safe-mode removes by itself | NONE | none | **not isolation** |

- `--strict-mcp-config` gave `mcp_servers: []` in every run (the user's claude.ai connectors and `.mcp.json` didn't load). There was no control run without it.
- The skills in the real `~/.claude/skills` never loaded in any variant, because the config dir was isolated.
- Built-in plugins load whatever the flags: `cc-plugin-agents-md`, `cc-plugin-telemetry`, `cc-plugin-plugin-authoring`.
- **Workspace skill `allowed-tools`:**
  - **Default sources:** `/proj-skill-p7` ran `touch SKILL_RAN.txt` **without any `can_use_tool`** (`permission_decision.source:"config"`). The control (`item05b`: the same `touch` asked for directly, same flags) **did** send `can_use_tool`. So a workspace skill's `allowed-tools` pre-approves tools: **fail**, if project sources are ever loaded.
  - **`--setting-sources ""`:** the skill isn't loaded (`/proj-skill-p7` "isn't available"): **pass**.
- **Other instruction sources, auto-memory** (`item05c_instruction_files.py`, fixtures `isolation-instruction-files-sources-empty[-no-memory|-with-tools].jsonl`): see facts 3 and 14.
  - Flags: `--setting-sources "" --tools "" --add-dir <dir>`, and again without `--tools ""` (`--with-tools`: 28 tools, any tool request denied).
  - Markers in a parent `CLAUDE.md`, `AGENTS.md`, `.agents/rules`, `.claude/rules`, an `--add-dir` `CLAUDE.md` and the auto-memory `MEMORY.md`.
  - The reply was `MEMORY-MARKER-A6` only, both with and without tools.
  - With `--settings '{"autoMemoryEnabled":false}'`, the reply was `NONE`.
- **Verdict:** use `--setting-sources ""`, plus `autoMemoryEnabled:false` in the office's settings, `--strict-mcp-config` and a per-agent `CLAUDE_CONFIG_DIR`.
- `--restricted` (plus an explicit `--tools` list) is an extra layer worth considering. It also confines the file tools to the working dirs and refuses bypass **[--help]**.

## 6. Inline agent identity

Script: `item06_agent_identity.py`. Fixtures: `agent-inline-json.jsonl`, `agent-file.jsonl`, `agent-unknown.jsonl`.

Flags: base plus `--setting-sources "" --permission-mode manual --permission-prompt-tool stdio` and:

| Variant | Extra flags | Reply | `init.tools` |
| --- | --- | --- | --- |
| inline JSON | `--agents '{"zephyr":{"description":"…","prompt":"You are Zephyr… Begin every reply with ZEPHYR:","tools":["Read","Bash"]}}' --agent zephyr --append-system-prompt-file <file: handle river-7>` | `ZEPHYR: Name: Zephyr; office handle: river-7.` | `["Read","Bash"]` |
| file | `--agents <path to the same JSON> --agent zephyr` | `ZEPHYR: Name: Zephyr; office handle: NONE …` | `["Read","Bash"]` |
| unknown | `--agent zephyr` (no `--agents`) | `Claude (Haiku 5.5); office handle: NONE.` | the full default set |

- The agent's `prompt` shapes the main thread, and its `tools` restrict the main thread's toolset.
- `--append-system-prompt-file` adds to it, and doesn't show in `ps`. The inline `--append-system-prompt <text>` wasn't tested in this spike.
- `initialize` lists the agent (`{"name":"zephyr","description":…}`), but `system/init` has **no field naming the active agent**.
- **An unknown `--agent` is silently ignored:** exit 0, nothing on stderr. The adapter should check that `initialize.agents` contains it, and can check `init.tools` too.
- Interactive mode shows `@zephyr` in the TUI header (item 10).
- **Verdict: pass.**

## 7. Tool detachment

Script: `item07_detach.py`. Result: `results/item07.json`.

Command: base argv plus `--permission-mode manual --permission-prompt-tool stdio`, with the harness (in the recorded run) allowing each `can_use_tool`; the script now allows only `sh hold.sh`. The Bash tool runs `sh hold.sh`, which starts `( sleep 62 & )`, `nohup sleep 63 &` and a foreground `sleep 61`.

The process tree, recorded by the harness with `ps` and `os.getsid`:

```
claude            pid 3623  pgid 3623  sid 3623   (our child, start_new_session)
└ /bin/zsh -c source <cfg>/shell-snapshots/snapshot-zsh-….sh … eval '…'   pid 3694 pgid 3694 sid 3694   ← new session
  └ sh hold.sh    pid 3695  pgid 3694  sid 3694
    ├ sleep 63    pgid 3694  sid 3694
    └ sleep 61    pgid 3694  sid 3694
sleep 62          ppid 1     pgid 3694  sid 3694   (double fork)
```

| Run | Action | Survivors |
| --- | --- | --- |
| R1 | `SIGKILL` claude's pid | **all** of zsh, sh, the three sleeps (zsh reparented to 1); `/tmp/cc-socks/<pid>.sock` **left behind** |
| R2 | `SIGTERM` claude's pid | **none** after 6 s; claude exited 143 in 0.6 s; its `.sock` was removed |
| R3 | `killpg(claude's pgid, SIGKILL)` only (the design's last stop step) | **all** |

- The tool sees claude's environment (`env.txt`): every allowlisted variable plus Claude's own (fact 9).
- **Verdict: tools detach** (their own session and group).
- Cleanup has to use SIGTERM to claude first, then the mark scan (or the recorded tool session ids).
- Leftovers were killed by pid, and the `.sock` files were removed.
- SIGINT to claude wasn't tested.

## 8. Sandbox and the office socket

Script: `item08_09_sandbox.py`. Fixtures: `sandbox-A-design-roots.jsonl`, `sandbox-B-cwd-only.jsonl`.

Command: base argv plus `--setting-sources "" --settings <file> --permission-mode manual --permission-prompt-tool stdio`, with the git env below; cwd `worktrees/t1`. One turn per process, which runs `sh <rt>/probe.sh`. The settings file:

```json
{"sandbox":{"enabled":true,"autoAllowBashIfSandboxed":true,"allowUnsandboxedCommands":false,
  "filesystem":{"allowWrite":[…],"denyWrite":[…]},
  "network":{"allowUnixSockets":["/private/tmp/claude-spike-s8/run/a.sock","/private/tmp/claude-spike-s8/run/c.sock"]}},
 "permissions":{"allow":["Bash(sh /private/tmp/claude-spike-s8/rt/probe.sh)"]}}
```

`c.sock` didn't exist when claude started. The harness created it after `initialize` and before the turn.

| Socket | A (listed: a, c) | B (no list) |
| --- | --- | --- |
| `a.sock`, listed, existed at launch | **CONNECTED** | refused (`PermissionError: [Errno 1] Operation not permitted`) |
| `b.sock`, not listed | **refused** (EPERM) | refused |
| `c.sock`, listed, **created after launch** | **CONNECTED** | refused |
| claude's own `$CLAUDE_CODE_MESSAGING_SOCKET` | refused | refused |

- Writes outside the roots were denied with `Operation not permitted`.
- No `can_use_tool` was sent: `autoAllowBashIfSandboxed` (on in the recorded runs) plus the allow rule handled it. The harness denies any request carrying `dangerouslyDisableSandbox`, and none came.
- **The scripts now set `autoAllowBashIfSandboxed: false`** (items 8/9 and 9b). With it on, every sandboxed Bash command runs without a `can_use_tool`, so the harness could never deny an unexpected one. The exact `permissions.allow` rule still pre-approves the probe. The JSON above is the recorded runs' setting.
- Paths were given as realpaths (`/private/tmp/…`). A `/tmp/…` spelling (a symlink) **wasn't tested**.
- **Verdict: pass.** The allow list can name a socket path that doesn't exist when claude starts.
- **Limit:** `c.sock` existed by the time the tool command ran. A path that's still missing when the command starts wasn't tested; it should matter only if the sandbox profile is built per command.

## 9. git commit in the sandbox

Scripts: `item08_09_sandbox.py`, `item09b_sandbox_gitdir.py [--deny]`. Fixtures: `sandbox-A-design-roots.jsonl`, `sandbox-B-cwd-only.jsonl`, `sandbox-B2-cwd-only-gitdir-probe.jsonl`, `sandbox-B3-cwd-only-plus-denywrite.jsonl`.

**Setup** (no sandbox):
- `origin` → `git clone --bare` → `work.git`
- `git config extensions.worktreeConfig true`, with `core.bare` moved to `work.git/config.worktree` (`rev-parse --is-bare-repository` still `true`)
- `git -C work.git worktree add worktrees/t1 -b task/t1 main`. Git 2.54 also wrote `work.git/worktrees/t1/config.worktree`.
- An agent home inside a normal repo: `homerepo/agents/x`.

**Env passed to claude**, which reaches the tools:

```
GIT_CONFIG_NOSYSTEM=1  GIT_CONFIG_GLOBAL=<rt>/gitconfig  GIT_CONFIG_COUNT=4
GIT_CONFIG_KEY_0=core.hooksPath GIT_CONFIG_VALUE_0=<rt>/hooks   (also core.fsmonitor=false, gc.auto=0, maintenance.auto=false)
```

Inside the sandbox, `git config --show-origin` showed `command line: <rt>/hooks` for `core.hooksPath` and `file:<rt>/gitconfig` for `user.email`. So the env-based config works under the sandbox.

**The probe runs** (cwd = `worktrees/t1` in all of them):

| Check | Unsandboxed (`--dry`, **[hand]**: prints only) | A: design roots | B: cwd only |
| --- | --- | --- | --- |
| task commit in `worktrees/t1` | OK | **OK** | **OK** (see below) |
| `git config --worktree` | OK | denied (`could not lock config file …/worktrees/t1/config.worktree`) | denied |
| `git config` (writes `work.git/config`) | OK | denied | denied |
| home-in-repo commit | OK | **OK**, with a harmless `cannot lock ref 'AUTO_MERGE': Unable to create '…/.git/AUTO_MERGE.lock'` | denied (the home isn't writable) |
| `work.git/config`, `hooks/`, `objects/info/alternates`, `worktrees/t1/commondir` | writable | denied | denied |
| `homerepo/.git/config`, `.git/hooks/`, create `.git/commondir` | writable | denied | denied |

A's roots:
- `allowWrite`: `work.git/{objects,refs,logs,worktrees/t1}`, `homerepo/agents/x`, `homerepo/.git/{objects,refs,logs}`, and the literal files `homerepo/.git/{index,index.lock,HEAD,HEAD.lock,ORIG_HEAD,COMMIT_EDITMSG,packed-refs,packed-refs.lock}`. Several of those files didn't exist yet, and the commit worked, so **literal non-existent paths work**.
- `denyWrite`: `work.git/objects/info`, `work.git/worktrees/t1/{commondir,gitdir}`, `homerepo/.git/objects/info`.

**What the sandbox allows by itself** (B2: cwd only, each path opened for append) and **with the office's `denyWrite`** (B3: `denyWrite` = `work.git/refs/heads/main`, `work.git/HEAD`, `work.git/packed-refs`, `work.git/info`, `work.git/description`, `work.git/FETCH_HEAD`, `work.git/worktrees/t1/gitdir`):

| Path | B2 | B3 |
| --- | --- | --- |
| `work.git/refs/heads/main` (another branch) | **ALLOW** | DENY |
| `work.git/refs/heads/task/t1` (own branch) | ALLOW | ALLOW |
| `work.git/HEAD`, `packed-refs`, `info/exclude`, `description`, `FETCH_HEAD` | **ALLOW** | DENY |
| `work.git/logs/HEAD`, `objects/pack/*`, `worktrees/t1/{HEAD,index}` | ALLOW | ALLOW |
| `work.git/worktrees/t1/gitdir` | **ALLOW** | DENY |
| `work.git/config.worktree`, `worktrees/t1/config.worktree` | DENY | DENY |
| `origin/README` (an unrelated repo) | DENY | DENY |
| the worktree's `.git` file, `.claude/settings.json`, `.claude/skills/x/SKILL.md` | DENY | DENY |
| `.gitattributes`, a plain file in the cwd | ALLOW | ALLOW |
| **task commit** | — | **OK** |

Notes on B2 and B3:
- `--debug-file` captured no seatbelt profile (the `seatbelt_*` keys in `results/item09b.json` and `item09c.json` are empty). So "the sandbox adds the git common dir by itself" is inferred from the probes, not read from the profile.
- The probe hides stderr, so a missing path also reads as DENY. For the `.claude/**` rows, the `mkdir` before them runs inside the sandbox too, so DENY there means the folder or file couldn't be created.
- B2 left empty `FETCH_HEAD`, `logs/HEAD` and `objects/pack/probe` files, which B3 inherited.

**Verdict: pass.** `git commit` works in the sandbox for a linked worktree and for a home inside a repo. The roots differ from the design (facts 4 and 5):
- The office must add `denyWrite` entries to keep the parts the design calls read-only.
- The literal list should also include `AUTO_MERGE` and `AUTO_MERGE.lock`, or accept that lock error.

## 10. Take-over

Script: `item10_takeover.py`. Fixtures: `takeover-headless-before.jsonl`, `takeover-pty.txt` (ANSI stripped), `takeover-headless-after.jsonl`.

- **H0, headless:** spec (`--setting-sources "" --strict-mcp-config --mcp-config {} --model haiku --agents … --agent zephyr --append-system-prompt-file … --permission-mode manual`) plus `--session-id <uuid>`. "Remember HERON" → `OK`. Then stdin closed.
- **T, interactive:**
  - Started with `pty.fork()` (new session, the PTY as controlling tty, 120x40, `TERM=xterm-256color`) as `claude --resume <uuid> <same spec>`.
  - The config dir was pre-seeded with `{"hasCompletedOnboarding":true,"projects":{"<cwd>":{"hasTrustDialogAccepted":true}}}`. The TUI came up in 3.1 s with no dialog, showing `@zephyr`, the earlier turn and `manual mode on`.
  - Typed the question and `\r` → answered `HERON, and I'll remember FALCON too.` Then `/exit` → exit 0.
- **H1, headless:** `--resume <uuid>` with the full spec. "Which two words?" → `HERON and FALCON.`
- The same session id throughout, and one transcript file.
- The interactive run had no budget cap: `--max-budget-usd` works only with `--print`.
- The harness answered the TUI's `ESC[6n` (cursor position) and `ESC[c` (device attributes) queries. Whether the TUI needs them wasn't isolated.
- **Verdict: pass.** Without pre-seeding, a fresh config dir would show onboarding and the trust dialog (**not tested**).

## 11. Memory

From `item01_stream.py`, `ps -o rss -p <claude pid>` (the process only, not its children). Command: base argv plus `--permission-mode manual`.

| Point | RSS |
| --- | --- |
| idle after `initialize`, no turn (an unscripted probe **[hand]** / item 1) | 215,184 KiB (210 MiB) / 221,360 KiB (216 MiB) |
| idle 5 s after turn 1 | 233,840 KiB (228 MiB) |
| idle 5 s after turn 4 | 243,488 KiB (238 MiB) |

So roughly 210-240 MiB per idle live agent, which grows slowly with the conversation. Four live agents (the U-S2 default) is about 1 GiB.

---

## Open questions / limits for the design

1. **Is `CLAUDE_SECURESTORAGE_CONFIG_DIR=` acceptable?** It's undocumented and could change in any release.
   - The alternatives are the real `~/.claude` as the config dir, which is the design's assumption and brings the user's config, plugins and history into every agent unless `--setting-sources ""` holds; `CLAUDE_CODE_OAUTH_TOKEN` from `claude setup-token`, which is long-lived, inference-only, and needs the user to make it; or an API key.
   - It needs a decision (ada / the user), plus a version gate and the `auth status` preflight.
2. **Hard-coded `~/.claude` writes** (fact 11): what else in the binary ignores `CLAUDE_CONFIG_DIR` (`bridge-spawn`, `ssh`, the OAuth refresh lock)? That matters if `~/.claude/**` is a fixed deny for agents. It's harmless here because the runtime does the writing, not the agent's tools.
3. **Other agents' branch refs:** can `denyWrite: work.git/refs/heads` plus `allowWrite: work.git/refs/heads/task/<id>` express "only my branch"? Untested. Otherwise, per-branch denies have to be regenerated before every start.
4. **The shell snapshot** (fact 10): should the office set `SHELL=/bin/sh`, or a clean `ZDOTDIR`, so that the user's rc functions and aliases don't reach agents? Untested.
5. **`/tmp/cc-socks/<pid>.sock` and `CLAUDE_CODE_MESSAGING_TOKEN`:** what can a holder of the token do over that socket, and can office-run agents reach the user's own Claude sessions when the sandbox is off? Not tested (I didn't touch the user's sockets). The sandbox blocks it when on.
6. **Unanswered approvals:** the office must expire them itself (deny or interrupt). A run of at least 31 minutes would confirm the CLI has no timeout of its own. Also decide whether "expired" should end the turn (`interrupt:true`, so the model never sees why) or let the agent continue (plain deny with a message).
7. **The system-prompt snapshot:** pass `--system-prompt-snapshot off` on every launch, or always send changes as a message? `off` changes the cached prefix on each launch. The cost wasn't measured.
8. **`queued_turn_count`** was 0 even with a second line already written. Busy/idle should key on `result` and `init` counts, not on this field.
9. **Background work after `result`** (Monitor, `run_in_background`, which Claude itself steers agents towards) wasn't exercised. The research's "idle = last event was a `result` with no new `init`" rule is still unverified here.
10. **Not tested:** SIGINT to claude; whether sandboxed tools can write the auto-memory dir; a nested `CLAUDE.md` loaded lazily; `--bare` (it needs an API key: no OAuth); `--include-hook-events`; `--replay-user-messages`; `updatedPermissions` (`allow_always` through the CLI); many concurrent processes; a sandbox path spelled via the `/tmp` symlink; the TUI without a pre-seeded config.
11. **Model behaviour, not protocol:** after a deny, the same conversation avoids similar actions without asking again. After an instruction change, the model "corrects" its earlier answers. Both matter for UX copy, not for the adapter.

## Paths written

- **In the worktree:** `spikes/claude/` only: `README.md`, `lib.py`, the `item*.py` scripts, `fixtures/` (40 files, about 0.9 MB, scrubbed), `results/` (17 JSON files, scrubbed).
- **Temp:** the recorded runs used fixed dirs, `/tmp/claude-spike-{auth,probe,s0,s1,s2,s2b,s3,s4,s5,s5c,s6,s7,s8,s10}/`, plus `/tmp/claude-spike-runs.log`. **All removed.** Scripts now use private `mkdtemp` dirs that they remove at exit.
- `/tmp/cc-socks/<pid>.sock`: the CLI's. Clean exits removed theirs. The two SIGKILL runs left one each, and I removed both (`3623.sock`, `4147.sock`).
- **The real `~/.claude`** **[hand]:** two empty folders, `~/.claude/ssh` and `~/.claude/bridge-spawn`, were created at 07:38:20 and 07:38:23, during the first sandbox-enabled run. They were the only entries born in `~/.claude` during the spike, and both were removed with `rmdir`.
  - New `~/.claude/backups/.claude.json.backup.*` files and `~/.claude.json` changes during the spike don't mention any spike path, and kept appearing after the last run, so they came from the user's live sessions.
  - No OAuth or storage lock files were found.
- **Keychain:** read by the CLI (the item `Claude Code-credentials`). Nothing was written by the harness. The CLI would write back only on a token refresh, the same as any `claude` run.
- **API ids** (`msg_…`, `toolu_…`, `req_…`) are left in the fixtures. They're per-request ids, not personal data or secrets.
- **Thinking `signature` blobs** (24 non-empty ones, in thinking blocks and `signature_delta` events) are `<scrubbed>`. So are the `description` and `displayName` of every model in `initialize`'s `models` list, and the `organization` key is dropped from `account`. `lib.Scrubber` does the same on re-runs.
- **Re-scrubbed after a privacy review** (`rescrub.py`, offline, no model call). Every fixture and result was rewritten so that:
  - all values inside `rate_limit_info` (utilization, `resetsAt`, overage status and reason) are `<scrubbed>`, with the keys kept
  - account-synced plugins (source `…@synced`) and their commands, skills and aliases are dropped from every list
  - the descriptions of the plan-related built-in commands (`usage-credits`, `extra-usage`, `upgrade`, `usage`) are `<scrubbed>`
  - TUI notice lines hinting at the plan or org (an upsell line, a hidden-notices count, and the `/usage-credits` help line) are cut from `takeover-pty.txt`

  On re-runs, `lib.Scrubber` does the JSON part and `lib.crop_tui_notices` the PTY crop (`item10` uses it for the fixture and for `pty_tail`).
- **`results/item05.json` was edited by hand:** the original run recorded the user's real skill names. They, and the count that replaced them for a while, were removed. Only `real_user_skills_present: true` is left (the user has several), plus a `_note` saying so. Re-runs record only booleans for this and for the synced commands and plugins.
- **Processes** **[hand]:** every spawned claude ran in its own group and was closed or killed. The tool `sleep`s from item 7 were killed by pid. `pgrep -x claude` at the end matched the baseline taken before the spike.
