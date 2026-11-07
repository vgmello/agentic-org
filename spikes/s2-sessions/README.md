# Spike S2: background sessions and the attach panel

Throwaway spike for ADR-0004 (design §5, §6, §12). Remy ran it on 2026-11-07, against Claude Code **2.1.311** on macOS (Darwin 25.6, arm64), with a rework pass the same day.
All sessions ran in a throwaway workspace (`/tmp/ac-spike-s2/ws`, a scratch git repo). They used a test agent `zz-spike-tester-max` ("always start replies with MAXMARK"),
a copy of `mods/goals`, and a marker mod that logs hook events. Every session was stopped and then removed with `claude rm` (see the end).

**Evidence** is in `evidence/` (first pass) and `evidence/rework/` (rework):

- `*.raw`: the exact PTY bytes.
- `*.raw.txt`: the same bytes with ANSI removed. Cursor moves eat some spaces.
- `*.screen.txt`: final screens rendered with `pyte`.
- `marker.log` and `rework/marker2.log`: every hook event, as JSON lines.

The evidence is **redacted** with `kit/tools/redact.py`:

- Home path → `~`.
- Plan name → `Claude <plan>`.
- Plan usage → `5h NN% ↻XX`.
- In the agent-view captures, every session row that isn't a `zz-spik…` spike row is replaced by a placeholder. The match is on the row's shape, not on keywords. Those two raw captures were deleted. Their OSC titles are kept in `q4-agent-view-titles.txt`.
- One real agent name is kept on purpose: `ac-staff-engineer-ada`, the leaked `CLAUDE_CODE_AGENT` value that proves Q7.

Redacting the raw files changes their byte counts slightly. The repro kit (marker mod, test agent, PTY driver, scripts) is in `kit/`.

## Outcome

**Keep the chosen design (Claude Code background sessions + `claude attach` in a PTY). Don't build the fallback.**

Nothing blocks it:

- Every background session already runs in Claude Code's own PTY host (`claude bg-pty-host … 200 50`), under a per-user daemon (`claude daemon run --origin transient`).
- A re-attach repaints the whole current screen, including a pending permission prompt, so we need no ring buffer.

Changes to the plan in §6:

1. **`--bg` ignores `--session-id`.** orgd can't assign the uuid. It reads the 8-hex short id from `--bg`'s stdout, and takes the full `sessionId` from `claude agents --json`.
2. **`attach`, `stop` and `logs` take the short id, never the full uuid.** `--resume` takes the uuid. Store both.
3. **The caller's environment doesn't reach the session.** Only cwd and `PATH` do, and `PATH` comes from **whichever client last started or woke the session**: `--bg`, `--bg --resume`, or `attach` (Q7). A session woken from the panel gets orgd's `PATH`, so orgd must run `attach` with the login-shell `PATH` (S3). Pass `AC_AGENT` through `--settings '{"env":{…}}'`. Sessions start from the env of whoever spawned the daemon, which today is an agent session.
4. **Reopen with `claude attach <id8>`.** It wakes a stopped session with its saved options and never makes a copy. `--bg --resume <uuid>` works only with no other flags, and only once the stop has settled. Right after `stop` returns it **usually forks a copy that loses all mods** (5 of 7 trials).
5. **The agent view is reachable from the panel** (`←` on an empty prompt, Ctrl+C twice, and after Ctrl+Z). It lists and acts on *every* background session of the user. The panel has to guard it (see Q4 and the mitigation).
6. **`--bg` needs a trusted workspace**, and **it appends stdin to the prompt**. Spawn it with stdin set to `/dev/null`.
7. **Background sessions block Edit and Write in the main checkout** (worktree isolation) unless `"worktree": {"bgIsolation": "none"}` is set. orgd must make sure it is (Q8).

## Findings by question

### Q1. `claude --bg --session-id <uuid> --agent --name --plugin-dir A --plugin-dir B`

| Item | Verdict | Evidence |
| --- | --- | --- |
| `--session-id` | **Broken.** It's ignored, with stderr `warning: --bg manages the session id; ignoring --session-id (use --resume <id> to continue an existing session)`. We passed `d2d18470-…` and the session got `370c5656-…` | `q1-bg-start.txt` |
| `--agent` | **Confirmed.** Replies start with MAXMARK, `agent_type` in every hook input is `zz-spike-tester-max`, and the transcript has `{"type":"agent-setting","agentSetting":"zz-spike-tester-max"}` | `marker.log`, `q2-transcript-67894d6a-excerpt.txt` |
| `--name` | **Confirmed.** It shows up as `name` in `claude agents --json`, in the transcript's `custom-title`, and on the prompt-box rule | `q1-agents-after-start.txt`, `q2-transcript-67894d6a-excerpt.txt` |
| Both `--plugin-dir`s | **Confirmed** (Q2) | |
| Output | `backgrounded · <id8> · <name>` plus four hint lines. Without a prompt, the first line ends `(idle — send a prompt to start)`. Exit code 0 | `bg-starts.txt`, `rework/bg-starts.txt` |
| Speed | Returns in **0.57–0.80 s**. The daemon hands the session a pre-started spare process | same |
| Short id | Always the first 8 hex characters of `sessionId`, and the job folder `~/.claude/jobs/<id8>/` (`state.json` has `respawnFlags`, `name`, `intent`) | `q1-job-state-370c5656.json` |
| Untrusted workspace | **First-run blocker.** Exit 1 with `Workspace not trusted. Run \`claude\` in /private/tmp/ac-spike-s2/ws once and accept the trust prompt, then retry.` | `q1-bg-start.txt` |
| stdin | **Gotcha.** `--bg` appends the piped stdin to the prompt: our launcher's own source ended up in `intent` and in the model's prompt | `q1-job-state-370c5656.json` |

### Q2. Mods in background sessions: **confirmed**

- **Marker mod (classic command hooks):** these all fired with the right `session_id`:
  - `SessionStart`, with source `startup`, `resume` (370c5656, woken by attach) or `fork` (33596af9)
  - `UserPromptSubmit`
  - `PreToolUse` for `Bash` (09b420de)
  - `Notification`, both `permission_prompt` and `idle_prompt`
  - `Stop`
  - `SessionEnd`, with reason `other` on `claude stop` (9 events)

  Source: `marker.log`.
- **Goals mod (function hooks):** the transcript shows the CreateGoal call and its result: `mcp__goals__CreateGoal {"condition": …}` → `Goal set: The assistant has replied with the word DONE …`.
  - The TUI then showed `running Stop hook · 6s` and the toast `Goal met: The assistant has replied with …`.
  - Sources: `q2-transcript-67894d6a-excerpt.txt`, `q7-logs-67894d6a.txt`.
- **Waking keeps the mods.** Mods are re-applied when a stopped session is woken by `attach` or by `--bg --resume` without flags, because the job saves `respawnFlags`. The resumed 370c5656 fired the marker hooks.
- **Notes for S1 and ADR-0003:** `PreToolUse` did **not** fire for:
  - (a) `mcp__goals__CreateGoal`, a tool registered by a mod
  - (b) a Bash `sleep 30` that Claude Code's built-in guard blocked
  - (c) a Write blocked by the worktree guard (Q8)

  Source: `marker.log`, `rework/q12-marker-events.txt`. In other words, tools a mod provides, and tool calls Claude Code blocks before hooks run, never reach a presence mod as tool events.

### Q3. Which ids `attach`, `stop` and `logs` accept

| Argument | `logs` | `attach` | `stop` |
| --- | --- | --- | --- |
| Full uuid | ✗ `No job matching '<uuid>'. Run 'claude agents' to list running sessions.` (exit 1) | ✗ same text | ✗ same text |
| Short id `370c5656` | ✓ | ✓ | ✓ `stopped 370c5656` (exit 0, also when already stopped) |
| Id prefix `370c56` | ✓ (prefix match) | not tested | not tested |
| Upper case `370C5656` | ✗ `No job matching` | not tested | not tested |
| `zz-spike-a`, with `zz-spike-ab` present | ✗ `2 sessions match 'zz-spike-a':` with both listed, then `Use more of the name, or the id.` (exact name doesn't win) | ✗ same | ✗ `No job matching 'zz-spike-a'` (stop takes no names) |
| Substring `spike-ab` | ✓ matches `zz-spike-ab` (**substring**, not only prefix) | | |
| A name two sessions share | ✗ `2 sessions match 'zz-spike-tester-max': … Use the id.` | | |

Evidence: `q3-*.txt`, `q3-attach-*.raw.txt`.

**Rule:** always pass the short id. pty-host additionally checks that the id is the exact `id` of one session in `claude agents --json --all`, and that no session name contains it.

### Q4. `claude attach <id8>` in a PTY: **confirmed, with caveats**

- **Render and input:** the full TUI renders, and typed text plus CR reach the session (`q4-demo-default.log`, `rework/q4-demo-default-rework.log`).
- **Latency:**
  - On an already-running session, the first byte arrives in **120–320 ms**, and the first settled frame (300 ms of quiet) takes **125–440 ms**.
  - **Waking a stopped session** through attach takes **242–664 ms** to the first byte and up to **~921 ms** to settle (6 wakes in `rework/q6-race-trial{2..6}-attach-wake.log`).
  - Waking a *stopped* session through attach first prints `Waking session <id>… Attaching…` and `Session is starting — showing its transcript until it appears. Ctrl+Z to detach`.
- **Resize:** SIGWINCH via the PTY makes the TUI clear the screen (`CSI 2J`) and redraw it fully. That was 2.3 KB at 60x20 and 3.3 KB at 140x40. There's no scrollback, because the TUI runs in the alternate screen.
- **Smallest usable size is 40x10.** At 30x8 the status line is gone, and at 20x5 the prompt box no longer fits (`q4-min-sizes.txt`). pty-host **rejects** sizes under 40x10.
- **Keys:**
  - **Esc** reaches the session. At an idle prompt nothing happens. At a permission prompt it cancels (status went `waiting` → `idle`, and the file wasn't written). The client stays attached (`q4-perm-esc.log`).
  - **Ctrl+C** reaches the session.
    - While it's generating, it interrupts: `Interrupted · What should Claude do instead?` (`q4-ctrlc-busy2.raw.txt`).
    - At an *empty idle* prompt, the first press shows `Press Ctrl-C again to detach (session keeps runing)` [sic], and a second press switches to the **agent view** (`q4-key-ctrlc.raw.txt`, `q4-key-ctrlc2.raw.txt`).
  - **`←` on an empty prompt** switches to the **agent view**. The prompt footer says `← for agents` (`q4-size-100x30.raw.txt`, `q4-session-ended.raw.txt`); the switch itself is in `q4-key-left.raw.txt`.
  - **Ctrl+Z** detaches: the attach client exits with code 0 and no output (`q4-key-ctrlz.raw`), and the session keeps running.
  - **Esc in the agent view** exits the attach client (code 0) after restoring the terminal modes.
- **Agent view (risk):**
  - It lists **every background session of the user**, other agents included, with their task summaries (redacted in the evidence).
  - Its footer reads `enter to return · space to reply · ctrl+x to delete · ? for shortcuts`, and its input reads `describe a task for a new session`.
  - So from inside one agent's panel, the owner can open, reply to or **delete another agent's session**, or dispatch a session that orgd neither started nor locked.
  - The window title goes to `claude agents` or `1 awaiting input · claude agents` there (`q4-agent-view-titles.txt`). In a session it's `<spinner> <name>`.
  - **Safety note for the panel and for testers:** never send Enter, space or ctrl+x into an agent view while real sessions are listed. This spike entered it only twice (`←`, and Ctrl+C twice), sent only Esc there, and didn't enter it again in the rework.
- **Killing the client:** SIGHUP to its process group (pty-host's `close`) and SIGKILL both leave the session running, and leave no attach process behind (`q4-sigkill.log`).
- **Re-attach repaints everything:** the conversation tail, the prompt box, the status line, and a **pending permission dialog** (`q4-perm-attach1.screen.txt`, `q4-perm-attach2.screen.txt`). No replay buffer is needed on our side.
- **Two clients at once** work: both get the stream, and input from either shows in both. **But the session has one size**, and the last client to attach or resize wins. A 120-column client got 80-column frames (`q4-two-A/B`). Re-send the size when a panel gets focus.
- **Session stopped while attached:** attach prints `Resume this session with: claude --resume <uuid>` then `Session <id8> has exited.`, and exits **0** (`q4-session-ended.log`, `q4-stop-while-attached.raw.txt`). pty-host reports `session_ended` only when `Session <id8> has exited` (the real id8) appears after the last `ESC[?1049l`.
- **Terminal modes the TUI turns on** (`q4-terminal-modes.txt`):
  - DEC private modes: alternate screen `?1049`, bracketed paste `?2004`, focus reporting `?1004`, synchronized output `?2026`, colour-scheme reports `?2031`, cursor `?25`.
  - Mouse: `?1000` + `?1002` + **`?1003` (any-motion)** + SGR `?1006`. It resets `?1016`.
  - Keyboard: kitty keyboard protocol (query `CSI ?u`, pop `CSI <u`) and modifyOtherKeys (`CSI >4;Nm`).
  - It sends **DA1 queries** (`CSI c`).
  - Because of any-motion mouse tracking, xterm.js text selection needs Shift (Option on macOS).
- **OSC sequences seen:** OSC 0 (title) and a private `OSC 7501;?`. No OSC 8, 9, 777 or 52 appeared in these runs. That's not exhaustive: we didn't try `/copy` or links.

### Q5. `claude agents --json`

- **Interactive session fields:** `pid, cwd, kind:"interactive", startedAt, sessionId, name, status` (no `id`, no `state`).
- **Background session fields:** the same plus `id` (8 hex) and `state`, and `waitingFor` while waiting (`q5-agents-baseline-redacted.json`).
- **`status` values seen:** `busy`, `idle`, and **`waiting` with `waitingFor: "permission prompt"`** (`q5-permission-status.txt`).
- **`state` values seen (background only):** `working`, `done`, `blocked` (no prompt yet, or waiting on the user), `stopped`. `state` is the job's last outcome, **not liveness**: a stopped session keeps `done` if its last turn finished.
- **Stopped sessions** disappear from `--json` and show in `--json --all` with no `pid` or `status`. **Liveness means having a `pid` in the list without `--all`.**
- `--cwd <path>` filters background sessions by working directory.

### Q6. Stop, then reopen

| Action | Result | Evidence |
| --- | --- | --- |
| `claude stop <id8>` | `stopped <id8>`, exit 0, ~0.6–0.9 s. `SessionEnd` reason `other`. It's idempotent | `q5-q6-stop.txt`, `marker.log` |
| `claude attach <id8>` on a **stopped** session | **Works.** New pid, **same sessionId**. `SessionStart` source is `resume` when the session already has a conversation (370c5656, 8217c384, a6a31bb3), and `startup` when it had none: a session started without a prompt and woken again (77633acd's first wake, and all three 9d3e2529 runs in `rework/q7-path-test.txt`). The same holds for `--bg --resume` wakes. Agent, name and mods are re-applied, and the conversation is kept (the model recalled the first prompt) | `q6-attach-stopped.raw.txt`, `marker.log` |
| `--bg --resume <uuid>`, no flags, session **settled** after stop | **Works.** `note: woke session 8217c384 with its saved options (--agent, --name, --plugin-dir, --model).` Same id | `rework/q6-race-trial3.json` (6 s wait), `trial10–13-poll.json` |
| `--bg --resume <uuid>` **plus any flags** | **Forks.** `note: background session 370c5656 keeps its own saved options, so the flags you passed started a copy as 33596af9. Without flags, the same command continues 370c5656 itself.` (`SessionStart` source `fork`) | `bg-starts.txt`, `marker.log` |
| `--bg --resume <uuid>` on a **running** session | **Duplicate confirmed.** `note: session 77633acd is already running in the background, so this started a copy as 4ab5c202. \`claude attach 77633acd\` opens the original.` | `bg-starts.txt` |
| `--bg --resume <uuid>`, no flags, **immediately after `stop` returns** | **Race, re-run cleanly (item 10).** 7 trials of `stop <id8>` → `--bg --resume <uuid>` with no flags and nothing in between, each capturing every command, `agents --json --all` before and after, and `state.json`. **5 of 7 started a copy** (`note: session <id8> is already running in the background, so this started a copy as …`). That happened on fresh, attach-woken and bg-woken sessions alike (trials 2, 5, 6, 7, 8). **2 of 7 woke the session** correctly (trials 1, 9). Every copy has **`respawnFlags: []`** and **no marker hook events**, so it loaded **no mods**; 4 of 5 got an auto name such as `agent-name-identity-check` | `rework/q6-race-trial{1,2,5,6,7,8,9}.json`, `rework/q6-race-copies-marker-events.txt` |
| Same, but one `agents --json --all` call (~0.13 s) between stop and resume | **5 of 5** woke the session correctly. The list already showed no pid on the first poll | `rework/q6-race-trial4.json`, `rework/q6-race-trial{10..13}-poll.json` |

**Consequences:**

- Resume is `claude attach <id8>` in the panel. It attaches when the session is running, wakes it when stopped, and never duplicates.
- A headless wake is `--bg --resume <uuid>` with no flags, only after `agents --json --all` shows no pid. The ~0.13 s that check takes was enough in 5 of 5 trials. That's not proof, so orgd should also re-check for a copy afterwards.
- Saved options pin the `--plugin-dir` paths, and changing any flag means forking. So the app's mod folder needs a **path that stays the same across app updates**.

### Q7. Environment and `claude logs`

- **cwd:** the session runs in the caller's cwd (`cwd` in the list, `pwd` in the hooks).
- **Env (broken for §6's plan):**
  - **The caller's environment isn't passed.** `AC_AGENT=zz-from-caller-env` set on the `claude --bg` call never reached the hooks: it's `null` in every `marker.log` line that isn't from the settings-env session.
  - **`PATH` comes from the client that last started or woke the session.** In the first pass, `caller_PATH_reached_hook` (`marker.log`) was `true` for sessions started or woken by our `--bg` launcher, which prepended `/tmp/ac-spike-s2/tools`. That directory wasn't in the spare process's start-up env. It was `false` for 8 events: 370c5656's first run (an early launcher that didn't prepend it) and the sessions woken by `attach` (370c5656, 77633acd, `SessionStart` source `resume`), whose callers didn't prepend it either.
    - To separate the two cases, the final round ran one session (9d3e2529) with a different directory prepended by each client (`rework/q7-path-test.txt`, `rework/marker2.log` field `spike_PATH_dirs`):
      - started with `--bg` → `pathA-bg-start`
      - stopped, then woken by `attach` (pty-host) → `pathB-attach`
      - stopped, then woken by `--bg --resume` → `pathC-bg-resume`
    - So a woken session doesn't keep the original starter's `PATH`; it takes the waker's. **For S3:** a GUI app's minimal `PATH` would apply to every session woken from the panel, unless orgd runs `attach` and `--bg` with the captured login-shell `PATH`.
  - **Everything else comes from the daemon.** That's the env captured when the daemon was first spawned by some `claude --bg` caller. Here that was **Ada's interactive session** (`rework/daemon-now.txt`: `--spawned-by {"label":"claude --bg","cwd":"~/projects/agentic-consulting/agents",…}`, `CLAUDE_CODE_AGENT=ac-staff-engineer-ada`).
  - **Sessions without `--agent` keep that value in their hooks.** Their hooks see `CLAUDE_CODE_AGENT=ac-staff-engineer-ada`: zz-spike-a and zz-spike-ab in `marker.log`, and zz-spike-env (7e69126b) in `rework/marker2.log`, captured again with only that variable logged. With `--agent`, Claude Code overrides it with the agent's name.
  - **`--settings '{"env":{"AC_AGENT":"…"}}'` works.** Hooks saw `AC_AGENT=zz-from-settings-env` (67894d6a in `marker.log`). `--settings` is also saved in `respawnFlags` (`rework/q12-respawnflags.txt`), so it survives a wake.
  - **The login-shell env (§8, S3) can't be set per session.** It can only come from the daemon's env, or from `--settings env`.
- **Inherited session vars:** this spike's shell runs inside an agent's background session, so it carries `CLAUDE_CODE_SESSION_ID`, `CLAUDE_JOB_DIR`, `CLAUDE_CODE_CHILD_SESSION=1` and the messaging socket and token.
  - An interactive `claude` started that way showed `⚠ Transcript saving is off — inherited CLAUDE_CODE_CHILD_SESSION marker`, plus the inherited agent name in its status line (`trust-raw.bin.txt`, name redacted).
  - orgd and `org` must strip these vars before spawning `claude`. pty-host and `kit/tools/cc` strip every `CLAUDE*` variable except `CLAUDE_CONFIG_DIR`, plus `AI_AGENT`.
- **`claude logs <id8>`** prints the session PTY's recent raw output (ANSI, as rendered at the daemon's 200x50), not a transcript. That was ~8–15 KB, exit 0, and stderr `Session <id8> · <name>` when it matched by name (`q7-logs-67894d6a.*`). It's fine for a preview, but it isn't a stable format.

### Q8. Worktree isolation (`bgIsolation`), added in the rework

Tests ran in `/tmp/ac-spike-s2/ws`, a git repo with one commit, using `--permission-mode acceptEdits`. The prompt was "Write `<ws>/iso-*.txt`":

| Setup | Result |
| --- | --- |
| No setting | **Blocked.** The Write tool returned `<tool_use_error>This background session hasn't isolated its changes yet. Call EnterWorktree first so edits land in a worktree instead of the shared checkout, then retry this edit using the worktree path (a path inside a linked git worktree, including one you create with \`git worktree add\`, is accepted). (To disable this guard for this repo, set "worktree": {"bgIsolation": "none"} in .claude/settings.json.)</tool_use_error>`. No file was written, and **no `PreToolUse` fired** (`rework/q12-iso-a-write-error.txt`, `rework/q12-marker-events.txt`) |
| `--settings '{"worktree":{"bgIsolation":"none"}}'` | **Allowed.** `File created successfully at: …/iso-b.txt`. The flag is saved in `respawnFlags` (`rework/q12-iso-b-c-results.txt`, `rework/q12-respawnflags.txt`) |
| `ws/.claude/settings.local.json` with `{"worktree":{"bgIsolation":"none"}}` | **Allowed.** `File created successfully at: …/iso-c.txt` (same files) |

**What orgd must do:** either the workspace carries `"worktree": {"bgIsolation": "none"}` (the agents repo now sets it in `.claude/settings.local.json`), or orgd adds it to the `--settings` JSON it already passes for `AC_AGENT`. Passing it in `--settings` is the robust choice: it doesn't depend on a local, untracked file, and it's saved for wakes.

Note that the guard fires before hooks, so a blocked edit is invisible to the presence mod. Agents would just look busy and then stop.

We didn't test the no-commit case: our earlier sessions in the repo with no commits never tried Edit or Write.

## pty-host, as built (reworked)

- **Crate:** `pty-host/` is a standalone crate (`portable-pty` 0.9, `libc`, `serde_json`), not part of any workspace.
- **Offline tests:** `cargo test` passes 4 unit tests and 15 tests that use fake `claude` scripts (`evidence/pty-host-tests-offline.txt`).
- **Live tests:** `PTY_HOST_TEST_SESSION=<id8> cargo test -- --ignored --test-threads=1` passed 3 of 3 with the **final code**, against a fresh throwaway session (a0fc75bb, since stopped and removed), run from this worktree (`evidence/pty-host-tests.txt`).
- **Clippy:** `cargo clippy --all-targets` reports no warnings.

```rust
pub fn open(opts: OpenOptions /* session_id: id8, cols ≥ 40, rows ≥ 10, claude_bin */,
            on_frame: impl FnMut(Frame) -> Result<(), DeliveryError> + Send + 'static) -> io::Result<AttachHandle>;
pub enum Frame {
    Data(Vec<u8>),        // 8-byte BE seq (0, 1, 2, … no gaps) + 1..=16 KiB raw PTY bytes
    Title(String),        // OSC 0/2 window title, deduplicated, sent after the Data frame that completed it
    Exit { code: u32, signal: Option<String>, reason: ExitReason },   // always last, exactly once
}
pub enum ExitReason { Closed, SessionEnded, AttachExited }   // "closed" | "session_ended" | "attach_exited"
impl AttachHandle { fn write(&self, &[u8]); fn resize(&self, cols, rows); fn close(&mut self); fn pid(&self) -> u32; fn signals_sent(&self) -> u32 }
```

The proposed Tauri side: `pty_open({session_id, cols, rows})` returns a handle plus a stream of raw data frames, `{kind:"title", text}` and `{kind:"exit", code, reason}`. Alongside it, `pty_write(bytes)`, `pty_resize(cols, rows)` and `pty_close()`.

**Opening:**

- It rejects anything but an 8-hex id.
- It **rejects** (doesn't clamp) sizes under 40x10, on open and on resize.
- It runs `claude agents --json --all` first, with a **3 s timeout**. It refuses unless exactly one session has that `id` (compared case-insensitively) and no *other* session's name contains it (also case-insensitive), because attach also matches names by substring.

**Environment:** every `CLAUDE*` variable except `CLAUDE_CONFIG_DIR` is removed, plus `AI_AGENT`. TERM is set to xterm-256color, COLORTERM to truecolor, LANG to en_US.UTF-8.

**Signals:**

- They go to the client's process group (portable-pty uses setsid).
- A signal is sent only while holding the `Child` lock, after `try_wait()` shows the child hasn't been reaped, and never to a pid ≤ 1. So a pid that has already exited and been reused is never signalled.
- `close()` after the client exited on its own sends nothing (tested offline and live).

**`close()`:**

- Sends SIGHUP. A SIGKILL watchdog fires after 2 s, and reading stops about 1 s later.
- The reader polls the master every 50 ms and checks the SIGKILL and give-up deadlines **on every pass**, not only on poll timeouts. That covers a client that ignores SIGHUP and stays silent (about 3 s), and one that ignores SIGHUP and **keeps printing every 10 ms** (about 3 s).
  - `close_finishes_when_client_ignores_sighup_and_keeps_printing` proves the fix: with the earlier `continue` after a read put back, `close()` didn't return within 4 s and the test failed. With the fix, it passes in 2.8 s (`rework/revert-check.txt`).
  - The grandchild test only shows that Exit comes promptly. On macOS the master sees EOF anyway, so that test passes with a blocking read too.
- It's idempotent, `Drop` calls it, and it returns after the Exit frame. Against a live session it took 2–40 ms.

**Delivery failure:** if the callback returns `Err` (for example the webview is gone), the handle hangs up its own client and reports `Closed`.

**Exit reason:** `session_ended` only when `Session <id8> has exited` appears after the last `ESC[?1049l`.

**Leaks:**

- A client spawned before a failed open is killed and reaped.
- A **panicking callback** counts as a delivery failure (`catch_unwind`), so the client is closed (tested).
- `Drop` on the shared state kills and reaps any client still running, as a backstop.
- `kill_and_reap` never signals or waits on a pid ≤ 1.
- `close()`'s worst case is about 3.5 s: 2 s to SIGKILL, 1 s draining, and up to 0.5 s to reap.

**Demo:** `target/debug/pty-demo <id8> [--cols N --rows N --dump F] [wait:S send:T enter key:esc|ctrl-c|ctrl-z|left resize:CxR expect:T:S close waitexit:S]` logs seq continuity, byte counts, titles and time to first byte (`rework/q4-demo-default-rework.log`).

**Notes for Riley:**

1. macOS PTY reads are at most **1024 bytes**, so frames are small and frequent: roughly 50–100 in a 10 s exchange. Coalesce in the Channel if the IPC overhead shows.
2. `session_id` is the 8-hex short id.
3. `attach_exited` covers a user detach (Ctrl+Z, or Esc in the agent view, both exit 0) as well as a bad id or a crash. To tell them apart, check `claude agents --json` afterwards.
4. Titles change with the spinner about once a second while the session works. Deduplicate them, or only react to their suffix.

## Known gaps (pty-host)

These are known and deliberately not fixed in the spike. Carry them into the real implementation:

- **Bounded close assumes a well-behaved caller.** `close()` and `Drop` finish in bounded time only if `on_frame` returns promptly, so the callback must not block. A `write()` blocked on a full PTY input queue holds the writer lock.
- **`verify_id` can wait forever on stdout.** Its `reader.join()` has no deadline if a child of `claude agents` keeps stdout open after the 3 s timeout kills `claude` itself.
- **Signalling the process group may reach the daemon.** `kill(-pid)` reaches attach's whole process group. If attach ever spawns the Claude daemon without `setsid` (for example when it wakes a session and no daemon is running), `close()` could kill the daemon. This is untested and must be verified in S6; if it's true, signal only the pid.
- **Exit race.** A rare race at exit can lose the final bytes, and with them the `Session <id8> has exited` line, so `session_ended` is misreported. The fix is to drain with `poll(fd, 0)` before deciding.
- **Close wins over session end.** `close()` called before the Exit frame reports `Closed`, even if the session had already ended (`SessionEnded`).
- **Pid 0 isn't reaped.** A child that reports pid 0 is never reaped.

## What this means

**ADR-0004:** the **chosen design stands**. The fallback's job, repaint and session lifetime, is already handled by Claude Code's daemon and its PTY host.

Amend §6 as follows:

- There's no orgd-assigned uuid. Store `id8` and `sessionId`.
- Reopen with `attach <id8>`.
- Pass `--settings` with `env.AC_AGENT` and `worktree.bgIsolation: "none"`.
- Spawn with stdin set to `/dev/null`, with the Claude Code variables stripped.
- Check workspace trust first.
- Guard the agent view (mitigation below).
- Re-send the size when a panel gets focus.
- Use a minimum panel size of 40x10.

**ADR-0003:**

- `AC_AGENT` from the process env never reaches background sessions. Use the hook input's `agent_type` from the main thread's `SessionStart` only (inside a subagent it is the subagent's type, per S1), or `--settings env`.
- **Don't trust `CLAUDE_CODE_AGENT`.** It leaks from whoever spawned the daemon when there's no `--agent`.
- `claude agents --json` reports `status: "waiting", waitingFor: "permission prompt"` for background sessions: a "Needs you" fallback when the presence mod isn't there.
- `Notification`/`permission_prompt` fired **6.07 s** after `PreToolUse` (`marker.log`, 09b420de: 1794066213.915 → 1794066219.989).
- `claude stop` sends `SessionEnd` with reason `other`, which maps to Offline.
- Liveness means having a pid, not `state`.
- `PreToolUse` doesn't fire for mod-registered tools or for calls Claude Code blocks itself.

**One live session per agent:** `--bg --resume` duplicates a running session, and usually also one that was stopped a moment ago, and that copy has no mods. `attach` doesn't, so use it for Resume.

## Open questions and follow-ups

- **No opt-out for the agent view or the detach keys.** `claude attach --help` shows no options at all: `Usage: claude attach <id|name>`, plus one paragraph. The panel's mitigation:
  - Swallow Ctrl+Z in the UI.
  - When a `Title` ends with `claude agents`, or attach exits 0 while the session is still live in `claude agents --json`, close the client and re-attach the same id8.

  This shows the agent view for at most a moment. **Decision: accepted for v1** by the staff engineer, as a UX issue and not a security one, since the owner owns every session listed there. **Follow-up: a feature request to Claude Code** for an attach flag or setting that turns off the agent view (and ideally the detach keys).
- **Daemon environment ownership (decided, goes to S3/S6):** the daemon is per user, shared with the owner's own `claude --bg`, and transient (it exits when its last client disconnects), so whoever spawns it first sets everyone's env. The decision:
  - orgd makes sure the Claude daemon is running.
  - When orgd starts it, it uses the captured login-shell env.
  - If a daemon is already running, orgd probes it with a test session: git must be on PATH, and `AC_AGENT` must arrive through `--settings env`.
  - If the probe fails, orgd warns the owner and offers a daemon restart once no sessions are live.
- **Mod path stability:** woken sessions reuse the saved `--plugin-dir` paths.
- **Env for orgd: allowlist, not denylist.** pty-host strips `CLAUDE*` and `AI_AGENT` (a denylist), which is enough for the spike. orgd should build the env from an **allowlist**: the captured login-shell env minus terminal- and tool-specific variables it doesn't want to pass on. Examples: `GIT_EDITOR` (an editor that blocks a headless git), `COREPACK_ENABLE_AUTO_PIN`, and `TERM_PROGRAM` / `WARP_*`, which describe the owner's terminal, not the panel.
- **`PreToolUse` gaps:** it doesn't fire for mod-registered tools or calls Claude Code blocks. S1 should confirm.
- **Not tested:**
  - `claude respawn <id>` (re-launch on a new binary)
  - an interactive session's `waiting` status
  - OSC 52, 8 and 9 in real use
  - the bgIsolation guard in a repo without commits
- **xterm.js:** it answers DA1. Without the kitty keyboard protocol, the TUI falls back to modifyOtherKeys.

## Sessions created

23 sessions, all stopped and then removed with `claude rm` (`evidence/created-uuids.txt`). The removals are in `evidence/rework/rm-output.txt` (21 sessions), the previous round's run for 9d3e2529, and `evidence/rework/rm-final-round.txt` for a0fc75bb, which also shows `claude rm 9d3e2529` now returning `No job matching '9d3e2529'`. The final `claude agents --json --all`, filtered to `/private/tmp/ac-spike-s2`, is `[]` (`evidence/final-listing.txt`).
