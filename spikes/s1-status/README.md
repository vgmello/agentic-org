# Spike S1: presence mod → Rust receiver → native notification

Throwaway code. Its value is in the findings. Claude Code 2.1.311, macOS (Darwin 25.6). Test sessions are haiku, run in `/tmp/ac-spike-s1/ws` as `zz-spike-tester-sam`.
This spike proves or disproves ADR-0003 and covers every row of design §5. It includes two rounds of rework after independent reviews (see the end).

**Verdict: ADR-0003 holds, with eight amendments** (see "What it means for ADR-0003").
- Every §5 row except `agent_needs_input` was triggered against real sessions.
- Hook to a macOS banner on screen takes p50 102.9 ms, max 155.0 ms, against the 1 s target.
- Two rows are **broken as specified**: the subagent counter, and `PermissionRequest` in headless `-p`. Fixes are below.

## Layout

| Path | What |
| --- | --- |
| `presence-mod/` | The spike mod: a `classic.*` glob hook plus a `tool.call` hook (`ToolCall`, sequenced).<br>• It forwards no tool inputs or outputs: only tool names, notification type and text, and reasons.<br>• `Stop` and `PermissionRequest` are reported after `await next(e)`, with `blocked`/`decided` and `next.trace` (`chain_below`).<br>• Identity is `AC_AGENT`, else bound **once** at `SessionStart` (`agent_type`, then `session_title`).<br>• Every event is written to a per-session state file, then delivered by a **bounded curl child** (amendment 8). |
| `goals-mod-copy/` | Copy of `agents/mods/goals`.<br>• `GOALS_MAX_BLOCKS` overrides `MAX_BLOCKS`.<br>• When the goal pauses, it sends `{"hook":"GoalPaused"}` to `AC_PRESENCE_SOCKET` through the same bounded curl. It sends no `last_reason`, and nothing when the env var is unset. |
| `receiver/` | Standalone crate `s1-receiver`.<br>• `src/rules.rs`: the §5 rules as a pure state machine keyed by **session id**, with table tests that replay recorded sequences from `tests/fixtures/`.<br>• `src/main.rs`: the Unix socket, refusing to start unless its directory is ours and 0700, and never removing a live socket. Created 0600, checks the peer uid, and applies each event **before** answering 204.<br>• It watches the session pid with kqueue, with a 500 ms crash grace. The pid is the socket peer, or the peer's parent for curl; both come from the kernel.<br>• Notifications run on their own thread. Also `poll-bench`, `notify-test` and the `claude agents --json` reconcile. |
| `harness/` | • `run-p.sh` / `run-i.sh` + `pty_drive.py` + `steps/*.json`: headless and PTY-driven sessions, with the inherited `CLAUDE_*` env stripped.<br>• `elicit_mcp.py`: a stdio MCP server that elicits, in form or URL mode.<br>• `allow-touch-hook.sh`: a settings hook that answers allow.<br>• `hung_receiver.py` + `hung_test.sh` (item 12), `latency.py`, `extract_fixtures.py`. |
| `evidence/` | Redacted logs: `~` for the home path, `<real-agent>` for real agent names.<br>• `receiver-runN-status.log` / `-raw-events.jsonl`. Run 1 used the original mod and receiver; runs 3 and 4 used the reworked ones.<br>• Grep them by session-id prefix. |

Results:
- Build: `Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.35s`
- `cargo test`: `test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s`
- `cargo clippy --all-targets`: no warnings.
- `claude plugin test goals-mod-copy`: `11 pass` / `0 fail` (`Ran 11 tests across 1 file.`). Two of the tests cover `GOALS_MAX_BLOCKS` and the paused signal.
- `claude plugin validate`: passes for both mods.

## §5 table, row by row

Session ids are prefixes: "run1" means `evidence/receiver-run1-status.log`, and so on. "test" names a `cargo test` that replays the recorded events.

| §5 row | Observed event / payload | Receiver status | Verdict · evidence |
| --- | --- | --- | --- |
| `SessionStart` → Idle | `source` (startup/clear/resume), `agent_type` (= `--agent`), `session_title` (= `--name`), `model` | Idle | **verified** · every session |
| `UserPromptSubmit` / `PreToolUse` / `PostToolUse` → Working | The `PreToolUse` e is the tool envelope: **no `session_id`, `agent_type`, `agent_id` or `agentId`**, even inside a subagent (verified live, run5 `d18f81ca` seq 7). The mod takes the loop from `tool.call`'s `agentId` by `tool_use_id` (run5 `2972f56d`) | Working | **verified** for built-in and MCP tools · run1 `130d2229`. Mod-registered tools (goals' `CreateGoal`) raise **no** classic Pre/Post, only `tool.call` (run2 `28266f46`). With goals loaded first, goals answers `CreateGoal` without `next`, so presence never sees it (run4 `59e9fda0`) |
| `PreToolUse` `AskUserQuestion` → Needs you | The same question also raises `PermissionRequest(AskUserQuestion)` 20 ms later and `permission_prompt` 6 s later | Needs you "Asked you a question". Reworked rules notify **once** | **verified** · run1 `09b93d00 seq=10..13`; test `ask_user_question_notifies_once…` |
| `PermissionRequest` after `await next(e)`, no decision → Needs you | `tool_name`, `decided=false` | Needs you "Wants to run Bash" | **verified** interactive and `--bg` · run1 `09b93d00 seq=4`, `81666304 seq=4` |
| …another hook decides → nothing | Settings hook: `decided=true decision="allow"` | stays Working | **verified** · run1 `b57290ef`; test `permission_decided_by_a_hook…` |
| …auto mode decides → nothing | `--permission-mode auto`: no `PermissionRequest` at all | Working | **verified** · run1 `edc1b2ad` |
| …headless `-p` | Fires undecided, then the call is denied at once | Needs you, then cleared, **but the notification went out** | **broken (false positive)** · run1 `3039b8a3` |
| `Notification permission_prompt` (backup) | 6.0 s after `PermissionRequest` | Kept as a backup, no second notification (rework) | **verified** · run1 `09b93d00 seq=5`; test `permission_undecided_notifies_once…` |
| `Elicitation` → Needs you | `mcp_server_name`, `message`, `mode`. In `-p` it is auto-cancelled | Needs you "Asked you a question" | **verified** · run1 `09b93d00 seq=20`, `ab8f9dcb` |
| `Notification elicitation_dialog` / `elicitation_url_dialog` | 6.0 s after `Elicitation` | backup | **verified** · run1 `09b93d00 seq=21`, `615eb535 seq=7` |
| `Notification agent_needs_input` | Not triggered. Binary strings show it is raised about **another** session (its `sessionId`, band `blocked`) | ignored by the reworked rules | **unverified** · `agent-needs-input-binary-strings.txt` |
| (new) `Notification elicitation_response` | When the elicitation is answered or cancelled | clears the question | observed · run1 `09b93d00 seq=22` |
| `Notification idle_prompt` → Idle | 60.0 s after `Stop` | Idle | **verified** · run1 `ad29c9ac seq=7`. That session still had the inherited child-session env; see caveats |
| `Stop`, goals blocked → Working | `blocked=true`, `chain_below=["1:goals:user","2:engine:core"]` | Working | **verified** with presence loaded first · run1 `57a6a89b seq=4` |
| `Stop`, not blocked → Idle | `blocked=false` | Idle | **verified** · run1 `130d2229 seq=5` |
| `Stop` with subagents running → Working (§5: count `SubagentStart`/`SubagentStop`) | `Stop.background_tasks=1 ['subagent']`. Internal forks (goals' evaluator, and one after every interactive turn) emit **`SubagentStop` with no `SubagentStart`** | Reworked: decided from `Stop.background_tasks`, with subagents tracked by `agent_id` and unmatched stops ignored | **broken as specified (§5 counter goes negative); use `Stop.background_tasks`** · run1 `b1357688`, `57a6a89b seq=3,5`, `09b93d00 seq=8,15`; run4 `4d279cdf`; tests `stop_with_background_subagent…`, `unmatched_fork_subagent_stop…` |
| Goal paused → Needs you | goals sends `GoalPaused{blocks_in_a_row:2}` **before** presence's `Stop(blocked=false)` | Needs you, held through `Stop`. A later question **adds** a reason; only `UserPromptSubmit` or `SessionStart` clears it | **verified** · run1 `57a6a89b`, run4 `59e9fda0`; test `goal_paused_is_held…` |
| `StopFailure` → Needs you | `error=model_not_found` (invalid `--model`) and `rate_limit` (account session limit hit, run3 `22c7c00b`). No `Stop` follows | Needs you | **verified** · run1 `03713d34`, run3 `22c7c00b` |
| `SessionEnd` clear/resume → unchanged | `/clear`: `reason=clear`, then `SessionStart source=clear` with a **new id**. `/resume`: `reason=resume`, then `SessionStart source=resume` with the resumed id, same pid | unchanged, then state moves to the successor session | **verified** · run1 `09b93d00`→`202068d2`, `d84714a9`→`09b93d00`; tests `clear_moves_state…`, `resume_successor_then_kill…` |
| `SessionEnd` other → Offline | `prompt_input_exit`, `other` (`-p` end, PTY closed, `claude stop`) | Offline. The later exit counts as clean | **verified** · run1 `202068d2 seq=28`, `81666304`; test `clean_exit_is_not_a_crash` |
| Exits with no `SessionEnd` → "Session crashed" | kqueue `NOTE_EXIT` 21 ms after `kill -9` (run1). Reworked: exit at +9 ms, crash declared after the 500 ms grace, banner at +595 ms | Needs you "Session crashed" | **verified** · run1 pid 78250, run4 `14e7f4fb`; `latency-run4-curl.txt` |
| machine lost (v2) → Unreachable | n/a | n/a | out of scope |

## Measurements

- **Latency** (`evidence/latency.txt`: run1, fetch transport):
  - Hook → receiver: **n=137 p50 1.8 ms, p90 10.3 ms, max 148.4 ms**. The max is a first `SessionStart` paying for the old mod's `$.process.run` pid lookup.
  - Hook → banner, timed from usernoted's own `Presenting` log line: **n=17 p50 102.9 ms, p90 151.0 ms, max 155.0 ms**.
- **Latency with the bounded curl transport** (`latency-run4-curl.txt`):
  - Hook → receiver: **n=29 p50 20.9 ms, p90 61.8 ms, max 67.4 ms**. Events queue behind each other in order.
  - Goal paused → banner: 110.9 ms.
  - Crash: exit → banner 595.1 ms (it includes the 500 ms grace).
  - Still well under 1 s.
- **Notification API:** `notify-rust 4.18.2` → `mac-notification-sys 0.6.15` (NSUserNotification).
  - Unbundled, it defaults to `com.apple.Finder`, and usernoted **denies** it ("Legacy client… Denying message"). Nothing shows, and the call returns `ok`.
  - With `set_application("com.apple.ScriptEditor2")` the banner is presented.
  - `com.apple.Terminal` raised a macOS "allow notifications for Terminal?" prompt on this machine.
  - The call blocks ~2 s off the main thread waiting for a delivery confirmation, while the banner shows in ~100 ms. It now runs on its own notifier thread.
  - `osascript` also works (~118 ms per call).
  - Evidence: `notify-native-vs-osascript-usernoted.log`.
- **Mod ordering: `--plugin-dir` order decides it, and the first one listed is outermost.**
  - Presence first: `Stop` sees `["1:goals:user","2:engine:core"]`, and `blocked=true` when goals blocks (run1 `57a6a89b`).
  - **Goals first, with an active goal and `GOALS_MAX_BLOCKS=2` (run4 `59e9fda0`):** presence's first `Stop` (seq 4) reported `blocked=false` with `chain_below=["2:engine:core"]`, although goals had blocked it. The next `Stop` (seq 6) has `stop_hook_active=true`, and the goal paused after 2 failed checks. So the agent showed a **false Idle for 4.3 s** while it kept working. Goals also hid its `CreateGoal` call from presence's `tool.call`. Test: `goals_loaded_first_hides_its_block_from_presence`.
  - The order isn't alphabetical. Settings (command) hooks sit beneath every module, so `await next(e)` includes them.
- **`claude agents --json`** (20 calls, `poll-bench.txt`): wall **p50 110.9 ms, max 143.8 ms**. CPU **p50 144.3 ms, max 158.3 ms**. That is ~1.4% of one core at a 10 s poll, plus ~142 MB RSS per call while it runs. bg sessions report `status:"waiting", waitingFor:"permission prompt"` (`agents-json-bg-waiting.txt`).
- **§5 clearing rule, amended:** "a permission or question clears on any later event" is wrong. A parallel tool call or a background subagent's call would clear a dialog that is still on screen.
  - A pending permission or question now clears only on `PostToolUse` / `PostToolUseFailure` / `PermissionDenied` **of the same call**: the same `tool_use_id`, else the same tool, else the same MCP server, and always **from the same loop** (main thread or the same subagent).
  - It also clears on the owner's prompt, on `idle_prompt`, on `elicitation_response` (questions), and when the main thread's turn ends (`Stop`).
  - `PreToolUse`, `ToolCall` and other loops never clear it. Test: `pending_permission_is_cleared_only_by_its_own_call`.
- **Identity (item 3):**
  - Payloads carry `session_id`, `cwd`, `permission_mode` and `agent_type`, which is **the subagent's type inside a subagent**. `SessionStart` adds `session_title`. There is no pid.
  - `AC_AGENT` reaches `-p` and PTY sessions, but **not `claude --bg`**. That env comes from a daemon spare process: `ps eww` showed a stale `CLAUDE_CODE_AGENT=<real-agent>` and another repo's `PWD` (`bg-spare-env.txt`). `--settings '{"env":{"AC_AGENT":…}}'` does reach it (run2 `28266f46`).
  - **What the review found was real.** With the old mod and receiver, a session without `AC_AGENT` split into three keys: the `agent_type`, `session:<id>` (PreToolUse) and a phantom `general-purpose` (subagent events). Test `identity_is_bound_once_per_session_without_ac_agent` replays `b1357688` that way.
  - **Fixed and verified live:** run4 `4d279cdf` ran without `AC_AGENT` and with a background subagent. All 14 events carry `agent=zz-spike-tester-sam` (`agent_source=SessionStart`), while `payload_agent_type` says `general-purpose` inside the subagent. The receiver holds one session. Test `live_no_ac_agent_run_with_subagent_is_one_session`.
  - Pid: the socket peer (`LOCAL_PEERPID`) matched the session pid on 137/137 run1 events. With the curl transport the peer is curl, so the receiver takes its parent. Without that, the first curl run raised **false "Session crashed"** (run3 `0dc66c36`).
- **Failure isolation:** with no receiver there is no slowdown. A hung receiver is item 12, amendment 8.
- **Security notes for the design:**
  - Put `events.sock` in a **user-owned 0700 directory** and check it at startup, never a shared `/tmp` path. The receiver now refuses otherwise (`socket-guards.txt`).
  - **Never remove a live socket.** It probes with a connect first, and refuses when a receiver answers.
  - It is created 0600, and `getpeereid` is checked against our uid. The rejection path is **not exercised** (that needs a second uid).
  - It answers only 204 and changes nothing.
  - goals' `last_reason` can quote conversation content, so it is **no longer sent**. If one is ever wanted, treat it like question text.
  - goals sends **nothing** when `AC_PRESENCE_SOCKET` is unset, so `orgd` must pass it to `--bg` sessions with `--settings env`, because the caller's env doesn't reach them.

## What it means for ADR-0003

Confirmed: hook events, plus `await next(e)`, plus kqueue give one correct status for every row tested, well under 1 s. Amendments:

1. **Identity:** take `AC_AGENT` from `--settings env` (the caller's env doesn't reach `--bg`), else bind once from the main thread's `SessionStart`, or from the first main-thread event after a reload. Key state on the session id.
   - Take the pid from the socket peer, or its parent for a child transport, never from the payload.
   - Bind it **only at `SessionStart`**, or at first sight. A curl in flight when its session dies is reparented to launchd (ppid 1), so ppid ≤ 1 is ignored, and no later event can move the pid (test `pid_is_bound_at_session_start…`).
2. **Ordering is a contract:** presence must be the **first** `--plugin-dir`. Otherwise goals' blocks are invisible (a proven false Idle) and so are goals' tool calls. orgd, `org session` and `bin/agent` must emit it first. The mod can check `next.trace` on `Stop` and warn when goals isn't beneath it.
3. **Headless sessions:** an undecided `PermissionRequest` in `-p` is denied at once, which makes a false "Needs you". The mod needs a reliable interactive-or-not signal (`$.session.surface()` was null at `SessionStart`), or orgd ignores it for its own `-p` runs.
4. **Reasons are additive and deduped, and a dialog clears only by its own call:** one `AskUserQuestion` raises Pre(AUQ), `PermissionRequest(AUQ)` and `permission_prompt`. Notify once per new reason, and the 6 s backup notifications never re-notify. A permission or question clears only by its own call's result, from its own loop (the amended §5 rule above). A paused goal or stop failure is kept until `UserPromptSubmit`/`SessionStart`. Sequence numbers carry a per-load `epoch`: on a new epoch (module reload), the receiver resets the seq window and re-binds identity (test `a_new_epoch_resets…`).
5. **Subagents:** use `Stop.background_tasks`, but **count only `type == "subagent"`**. The list also holds `shell`, `monitor` and `workflow` tasks, which can outlive the turn indefinitely, so counting them would leave a lead Working forever. Track `SubagentStop` by `agent_id`, because forks emit unmatched stops. Test: `only_background_subagents_keep_a_stopped_lead_working`.
6. **`agent_needs_input`** is about another session: ignore it, or route it by its own session id.
7. **Mod-registered tools** need `tool.call`. Classic Pre/Post doesn't see them.
8. **A hung orgd must not delay a session's exit. Choice: (a), a bounded child, plus the state file from (b).** The evidence (`hung_test.sh` against `hung_receiver.py`, files `evidence/item12-*.txt`):
   - The mods API has **no** timeout or abort for `$.http.fetch` (`HttpInit` has none). Racing it lets the hook return, but **the session drains every in-flight fetch before `SessionEnd`** (~30 s each against a hung socket).
   - **Before**, the original un-awaited fetch: `Stop` reached the socket at …785601, `SessionEnd` at …815623 (**+30.0 s**), and the run took **35 450 ms**.
   - **Option (b) alone**: a state file plus skipping POSTs while one is pending. Only the `SessionStart` POST went out (5 skipped), and `SessionEnd` still came **30.0 s** after it. The run took **31 039 ms**. It can't help a session that ends within 30 s of a hung POST.
   - **Chosen (`transport=curl`, the default):** each event goes out through `$.process.run(['/usr/bin/curl', '--max-time', '0.3', '--unix-socket', …], { timeoutMs: 1000 })`.
     - One child at a time, in order. Unordered children raced: `ToolCall` seq 3 arrived after seq 4, run3 `0dc66c36`.
     - A curl timeout (exit 28) marks the receiver hung, and POSTs are skipped for **5 s**. **`Stop` and `SessionEnd` are always attempted**, since they're bounded anyway. A first version skipped everything for 120 s, which would have swallowed `SessionEnd` and led to a false crash.
     - Every curl starts `-q --noproxy '*'`, so it ignores `~/.curlrc` and never uses a proxy (goals' too).
     - Every event is written first to `<state dir>/<session>.json` with `$.fs.write`. The state dir is separate from the socket dir (spike: `/tmp/ac-spike-s1/state`) and made 0700 once per load (`mkdir -m 700` + `chmod 700`). `$.fs` can't set a file mode, so files are 0644, and the 0700 directory is what keeps them private.
     - **Before declaring a crash, the receiver reads the session's state file.** If it shows `SessionEnd` (not clear/resume), the exit is clean and the status is Offline (test `session_end_in_the_state_file_means_clean_exit…`).
   - **After** (`item12-after-curl.txt`, 120 s backoff): the run took **6 297 ms**, and the process exited **715 ms after the `SessionEnd` hook**. The state file held `hook=SessionEnd … seq=7 posts_skipped=5`.
   - **Final** (`item12-after-curl-final.txt`, 5 s backoff, `Stop`/`SessionEnd` always attempted): the run took **4 181 ms**. The hung socket got `SessionStart`, `Stop` and `SessionEnd`; the process exited **684 ms after the `SessionEnd` hook**. The state file held `hook=SessionEnd reason=other seq=7 posts_skipped=4`.
   - In an unserialized curl run, `Stop` → `SessionEnd` was 331 ms.
   - Costs:
     - one short process per event (~10-20 ms; hook → receiver p50 20.9 ms against 1.8 ms for fetch);
     - curl must exist (`/usr/bin/curl` on macOS);
     - `$.fs` has no rename, so the state file is a whole-file write, not an atomic replace. orgd must tolerate a torn read.
   - Ask upstream for `HttpInit.timeoutMs` or a signal. With it, go back to fetch with a 200 ms bound and drop the child.

Also relevant:
- **Goal paused:** I recommend goals sends it itself, as built. It arrives before the `Stop`, through the same bounded curl.
- **Notifications:** the app's own signed bundle id avoids the Finder denial. Unbundled `tauri dev` builds may hit it (open question).
- `claude agents --json` is cheap enough at 10 s, and it gives a coarse "waiting" for bg sessions that run without the mod.

Relevant to **ADR-0004 / S2**:
- `claude --bg` ignores `--session-id`, and prints a warning.
- `claude stop` rejects the full uuid and needs the short id.
- Mods run in bg sessions, and `claude stop` gives `SessionEnd reason=other`.
- bg sessions inherit a daemon spare's env.

## Known gaps (documented, not fixed in the spike)

- **Socket trust:**
  - Only the socket's parent directory is checked (ours, 0700).
  - curl can't verify who is listening, so a process that wins the bind could read events. The mods can't check the listener either.
  - The `/tmp/ac-spike-s1/...` defaults are spike-only. The app should use its per-user app-support directory.
- **Untested paths:** the tests don't exercise `session_pid` / `parent_pid` (kernel lookups), or the curl transport's chain, skip window and state-file writes. They were checked only in live runs (run4, run5, `item12-*`).
- **Notification queueing:** there is one notifier thread, and each native call blocks ~2 s waiting for a delivery confirmation. Simultaneous notifications therefore show about 2 s apart. "Under 1 s" holds for an isolated notification only. The app should notify in parallel or asynchronously (UNUserNotificationCenter).
- **presence-mod has no tests.** That's a follow-up for the real `mods/presence`. It needs contract tests for the payloads, the curl chain and skip window, the epoch and the identity binding.
- **Also open after the final review** (spike-only, to carry into the real `mods/presence` and `orgd`):
  - The state directory's owner and mode are never checked: the mod ignores a failed `mkdir`/`chmod`, and the receiver reads state files without checking their owner. Another user who pre-creates it could read them, plant symlinks, or forge a `SessionEnd`. orgd must create it 0700 and check the owner before reading or writing.
  - The 5 s backoff after a slow POST bypasses only `Stop` and `SessionEnd`. A `PermissionRequest`, question, `Elicitation` or `StopFailure` inside that window reaches only the state file, and the receiver reads that file only at process exit. Fix: make those events always-sent, or re-read the state file on the next contact.
  - `PermissionRequest` carries no `tool_use_id`. If it also lacks the agent id at runtime (not verified live), a subagent's request is filed under the main thread, and its own `Post` can't clear it.
  - A pending item raised only by a backup notification (`permission_prompt`, `elicitation_dialog` with no request before it) falls back to "any call from that loop clears it".
  - goals' `GoalPaused` has no `seq` or `epoch`. A late one can re-raise "Needs you" after `SessionEnd` or the owner's next prompt.
  - `kqueue()`'s result isn't checked. If it fails, the watch loop spins.
  - Identity edge cases after a module reload: a skipped Subagent* rebind still records the new epoch, so a placeholder label isn't rebound later in that epoch; and with `AC_AGENT` set, Subagent* events after a reload go out with `agent: null` until the next main-thread event. The real mod should bind from `AC_AGENT` on any event and always rebind a placeholder.

## Caveats

- The first 12 sessions (up to `ad29c9ac`) inherited the parent session's `CLAUDE_*` env. `CLAUDE_CODE_CHILD_SESSION=1` turned their transcripts off and hid them from `claude agents`. Every later session ran with that env stripped, and every row except `idle_prompt` was observed in a stripped session.
- Banner timing matches each notify call to usernoted's `Presenting` lines by time window, using the logged call duration. It is approximate when two notifications fire within ~20 ms.
- Run1/run2 events were made by the original mod (fetch transport, payload `agent_type`). Runs 3-4 used the reworked mod.

## Open questions and follow-ups

- How `--plugin-dir` mods interleave with installed (marketplace) plugins in the `user` tier.
- A reliable interactive-vs-headless signal inside a mod.
- Upstream: a timeout or abort for `$.http.fetch`, and a rename (atomic replace) in `$.fs`.
- What triggers `agent_needs_input` in practice.
- Notifications in `tauri dev` vs the signed app, and UNUserNotificationCenter (`notify-rust` `preview-macos-un`).
- The uid-rejection path (needs a second user), and per-session rate limiting.

## Review rework (items 1-12), where each landed

1. A char-boundary `trunc`, and `parse_body` that never panics; locks recover from poisoning (`lk`). Test `stale_and_bad_bodies_never_panic`.
2. The event is applied before the 204, plus a 500 ms crash grace (`kqueue_loop`).
3. Keyed by session; identity bound once (mod and receiver). Verified live without `AC_AGENT`.
4. Reasons are additive; `ToolCall` is sequenced and never clears.
5. kqueue watches the socket peer, or its parent for curl.
6. Table tests over the recorded sequences (15; 20 after the second round); the goals tests cover `GOALS_MAX_BLOCKS` and the paused signal; clippy is clean; notifications run on their own thread.
7. Numbers taken from `latency.txt`; `latency.py` uses the logged call time.
8. Subagent verdict changed to "broken as specified".
9. Re-run goals-first with an active goal: proven.
10. Security notes, the socket directory check, and no removal of a live socket.
11. Evidence redacted.
12. Bounded curl transport plus a state file (amendment 8).

Second review round:
- A pending dialog clears only by its own call's result, from its own loop (amended §5 rule). The loop comes from `tool.call`, because the `PreToolUse` envelope carries no agent id at runtime.
- Only `subagent` background tasks count.
- The pid binds at `SessionStart` only, and ppid ≤ 1 is ignored.
- Hung backoff is 5 s, `Stop`/`SessionEnd` are always attempted, and the receiver checks the state file before declaring a crash.
- A per-load `epoch` comes with each seq.
- The state dir is separate and 0700.
- `parse_body` rejects JSON that isn't an object.
- curl uses `-q --noproxy '*'`.
- Plan usage/cost, the slash-menu line and third-party (Chrome) notification records are redacted from the evidence.
- Known gaps are documented above.
