# Design: agent sessions (the office runs the agents)

Status: **proposed, rev 2** (ada, 2026-11-08), from the user's request:
- start Claude Code, Codex and Antigravity agents
- pass them their instructions
- send them work and get their responses
- resume them in their previous workspace
- manage their processes

Rev 2 resolves the independent review of rev 1. It decides where sessions run (a home session plus one session per task), adds a supervisor crate, makes every new call app-bound, issues the session token ADR-0003 promised, defends against a hostile workspace and puts a lease on take-over.

It builds on:
- ADR-0001: each office owns its runtime side, meaning sessions, live status and local queues.
- ADR-0002: storage (`office.db`, encrypted).
- ADR-0003: messaging through the office. This design amends it (see "ADR-0003 amendments").
- `offices-hq-and-satellites.md`
- `projects-git-github.md`: a worktree per task.
- Eden's run-modes spec: always on, on call, scheduled, one-off.

The research behind it, with exact flags, protocols and what was verified, is in `docs/research/agent-runtimes-control.md`.

"Spike S1" and "spike S2" always mean the earlier v1 spikes. This design's own plan steps are S0 to S7.

## Decisions
1. **Headless-first.** The office drives every agent as a child process over the runtime's structured JSON-lines protocol:
   - **Claude Code:** `claude -p --input-format stream-json --output-format stream-json --verbose`, one long-lived process per session.
   - **Codex:** `codex app-server`, JSON-RPC with threads. One supervised app-server per office. Each Codex session is a thread on it.
   - **Antigravity:** `agy acp` (the Agent Client Protocol). `agy` stream-json is the fallback only if the spike shows ACP isn't ready, and then approvals follow decision 7's fallback.

   **This replaces the `--bg` + `claude attach` choice** of the unmerged ADR-0004 draft (`0004-session-panel-embedded-terminal`), which spike S2 confirmed. `--bg` has no programmatic prompt channel, no structured events and no usage stream, and the office needs all three. A person can still work with an agent directly (see "Taking over"). ADR-0004 is rewritten to match before it merges.
2. **One port, one adapter per runtime, one supervisor.**
   - `org-runtime` is the **port**: `AgentRuntime`, `Session`, `SessionSpec`, `LaunchPlan`, the normalized `SessionEvent`, `Capabilities` and usage types. It's a contract, like `org-proto`. It does no I/O.
   - The port is **synchronous**: blocking calls plus `std::sync::mpsc` channels for events. No async runtime.
   - `org-runtime-claude`, `org-runtime-codex` and `org-runtime-agy` implement it. An adapter **builds a launch plan** (argv, env additions, files to write) and **speaks its protocol over the child's pipes**. It never spawns.
   - `org-supervise` is the **one place** that spawns, owns process groups, applies the env allowlist, runs the stop sequence, reaps children and cleans up orphans.
   - Adapters depend only on `org-runtime`: never on each other, on `org-supervise`, on `org-office` or on storage. `org-office-server` (the composition root) wires them together.
   - The volatile third-party CLIs sit behind a stable port, so a runtime's protocol change touches one adapter.
3. **The office owns the run, the runtime owns the conversation.** The runtime keeps its transcript in its own store (e.g. `~/.claude/projects/…`). The office keeps, in `office.db`:
   - the **session record**: agent, kind (home or task), runtime, runtime session id, workspace, the full launch spec, state, and the child's pid, process group id and start time
   - the **runs**: each wake, with its triggers, start and end, outcome and usage
   - the **approvals**
   - a **bounded normalized event log** per run, for Activity and the session panel

   Prompt and response bodies stay in the runtime's store and in the office's encrypted `office.db`. They're never written to logs.

   **Transcripts are plaintext.** Each runtime writes its transcript unencrypted in its own store, outside ADR-0002's encrypted `office.db`. That's accepted for phase 1 as a stated exception to ADR-0002's encryption at rest, and the user is told (U-S3). ADR-0002 gets a matching note.
4. **Sessions and workspaces.** An agent has:
   - a **home session** for wakes that aren't about a task (messages, schedules, owner pings). Its cwd is the agent's home folder.
   - **one task session per task.** Its cwd is that task's worktree (`projects-git-github.md`). The agent's home is added **read-only**, for its memory:
     - Claude: `--add-dir <home>`, with the permission policy denying writes there.
     - Codex: the home is readable but left out of the thread's writable roots (`thread/start`).
     - Antigravity: `--add-dir <home>`, with the policy denying writes there.

   **At most one live process per agent.** Wakes for the same agent are serialized, and a task session and the home session never run at once.

   **If the workspace is gone** (the worktree was removed, the transcript was pruned, or the resume failed), the office starts a fresh session with a short summary of the last run's record, and the run records why.
5. **The full launch spec is replayed on every start and resume.** Claude's `--resume` doesn't restore `--settings`, `--mcp-config`, `--plugin-dir` or `--add-dir`, a `-p` resume starts in Manual permission mode, Codex filters threads by cwd, and Antigravity's conversations are tied to the cwd. So the office stores the whole spec and always replays it, in the session's own workspace.
   - The spec includes the permission mode and the sandbox mode. Claude always gets `--permission-mode`, and Codex gets `approvalPolicy` and `sandbox` on both `thread/start` and `thread/resume`.
   - `spec_hash` covers the whole spec. Claude keeps the system prompt it recorded at the first request (`--system-prompt-snapshot`), so when a resume's `spec_hash` shows changed instructions, the office sends the change as a message ("Your instructions changed: …").
   - Bypass modes are banned everywhere: Claude `bypassPermissions`, Codex `danger-full-access` and `--yolo`, Antigravity `--dangerously-skip-permissions`.
6. **Isolation by default.**
   - **Environment allowlist.** The child gets only:
     - `PATH` (captured once from the user's login shell), `HOME`, `LANG`, `TMPDIR`, `USER`, `LOGNAME`, `SHELL`, and `TERM` for the take-over PTY
     - `SSH_AUTH_SOCK`, only if the agent's policy allows git over ssh
     - `ORG_AGENT=<agent>`, `ORG_OFFICE=<office id>`, `ORG_OFFICE_HOME`
     - `ORG_SESSION_TOKEN`: a random token per start and resume, held only in office memory, required on `msg.bind` (see "Messaging"), never in argv or a settings file, never logged
     - office-set runtime switches such as `DISABLE_AUTOUPDATER=1`

     Everything else is dropped, including `CLAUDE_CODE_*`, `AI_AGENT` and any API key.
   - **Runtime binaries** are run by their recorded absolute paths, never looked up on `PATH`.
   - **No personal config reaches agents:**
     - Claude: `--setting-sources` loads no user, project or local file. Settings come only from the office's `--settings <file>` (0600). `--strict-mcp-config` with the office's `--mcp-config <file>`. The spike confirms the exact `--setting-sources` value, and whether the user's `~/.claude/CLAUDE.md`, skills and agents still load. If they do, it tries `--bare` (if sign-in still works), else documents the limit.
     - Codex: the user's normal Codex home, so the user's sign-in works (no `CODEX_HOME` override). Isolation comes from per-thread config (`thread/start` config overrides) and `-c` flags on the app-server, and from `--ignore-user-config` / `--ignore-rules` if they keep the sign-in. The Codex spike lists what still leaks (e.g. `~/.codex/AGENTS.md`, MCP servers).
     - Antigravity: `--gemini-dir <office dir>` if sign-in still works there. **Known limit:** otherwise the user's `~/.gemini` rules, hooks, MCP servers and plugins load into every Antigravity agent. We document it and tell the user.
   - **Generated runtime files** (settings, MCP config, instruction files, Antigravity's custom agent file) live **outside the workspace**, in `<office home>/agents/<agent id>/runtime/` (0700, files 0600), where `<office home>` is `<home>/offices/<office id>/`. If a runtime can only read them from the workspace, they're git-excluded through `.git/info/exclude`.
   - **Runtime sandboxes are on by default:** Codex `workspace-write`, Claude's bash sandbox (in the office's settings), Antigravity `--sandbox`.
   - **Auth stays in each runtime's own store.** The office never puts API keys in an agent's environment. API-key support, when it's added, goes through each runtime's own credential mechanism.
7. **Approvals go to the person, never silently lost.** In headless mode all three runtimes deny or soft-deny actions that need approval. The adapters bridge approval requests into the office:
   - Claude: `--permission-prompt-tool stdio` and `can_use_tool` control requests.
   - Codex: `item/*/requestApproval`.
   - Antigravity: ACP `session/request_permission`.

   The office answers from the agent's **permission policy** (what's allowed without asking, by tool and path), and asks the owner in **Needs you** for anything else. Each runtime's own allow rules and sandbox are set from the same policy as a first line of defence.
   - Needs you shows the **structured tool input**. The agent's own justification is shown too, labeled as untrusted.
   - An unanswered approval **expires after 30 minutes** (configurable) and counts as a deny. Waiting doesn't count toward the run's time limit.
   - Approvals are office data in `office.db` and survive an office restart. If their session died, answering one returns `not_found`.
   - `allow_always` is scoped to (agent, tool, path pattern) and stored in the agent's permission policy.

   **Fallback:** if a runtime's approval bridge doesn't work, its actions that need approval are **denied**, the run ends as `blocked`, and a Needs-you item says what was denied. Skip-permissions modes are never used.
8. **Instructions are composed by the office** from:
   - the agent's identity and memory: its home `CLAUDE.md` / `AGENTS.md` and its profile
   - the project's instruction files in the workspace
   - the **wake guidance** for the moment (`docs/.guidance/<moment>.md`, Eden's agent-guidance)

   Each runtime receives them its own way:
   - Claude: `--agents <json>` defines the agent inline (name, description, a one-line prompt) and `--agent <id>` selects it. The instructions go in `--append-system-prompt-file`, so they don't show in `ps`.
   - Codex: `developerInstructions` on `thread/start` / `thread/resume`.
   - Antigravity: a generated custom agent file `<runtime dir>/.agents/agents/<id>.md`, read through `--add-dir <runtime dir>`, selected with `--agent <id>`.

   Instructions that change mid-session are sent as a message, not by restarting.
9. **Office-run sessions load no mods and no inbox hooks.** Not presence, inbox or goals: the office does presence, the wake path and the run rules itself. The inbox hooks stay for sessions people start interactively.

## The model
- **SessionSpec** `{ agent_id, kind (home | task), task_id?, runtime, runtime_path, model?, workspace (cwd), extra_dirs (read-only), instructions, permission_policy, permission_mode, sandbox, env_allowlist, mcp (office-provided only), runtime_files }`
  - `workspace` is the agent's home folder for the home session, and the task's worktree for a task session.
- **Session** (one conversation of one agent, at its office) `{ id, agent_id, kind, task_id?, runtime, runtime_session_id, workspace, spec_hash, state, pid?, pgid?, child_started_at?, started_at, last_turn_at }`
  - States: `starting` · `idle` · `busy` · `needs_approval` · `with_you` · `stopping` · `stopped` · `crashed`.
  - **One live process per agent.** The office holds a lock per agent. It's released only after the child is reaped.
- **Run** `{ id, agent_id, session_id?, triggers: [trigger], queued_at, started_at?, ended_at?, busy_ms, outcome?, fresh_start_reason?, usage }`
  - A trigger is `message | ask | task | schedule | owner | one_off`, with its id. A run records a **list** of them, because triggers that arrive while it's live are merged into it.
  - Outcomes: `done` · `idle_timeout` · `max_time` · `interrupted` · `blocked` · `quota` · `failed` · `crashed` · `interrupted_by_restart`.
  - A run is one wake. Activity and Eden's Runs tab show runs.
- **Approval** `{ id, agent_id, run_id, session_id, tool, input (structured), justification (the agent's own words, untrusted), requested_at, expires_at, state (pending | allowed | denied | expired), decided_by? }`
  - Office data in `office.db`, so it survives an office restart.
- **Permission policy** (per agent, office data) `{ allow: [(tool, path pattern)], deny: [(tool, path pattern)], git_over_ssh: bool }`, plus the fixed denies under "Security".
- **Desired state** (per agent): `running` or `stopped`, set by `agent.wake` and `agent.stop`.
- **Usage per turn:** tokens in, out and cached; `cost_usd`, always marked *estimated*; the model.
  - Per-turn fields where they exist: Claude `model_usage_since_last_result`, Codex `thread/tokenUsage/updated.last`.
  - Otherwise (Antigravity's cumulative `usage`): a baseline at the first report after any start or resume, then deltas.
  - Claude's `total_cost_usd` is shown as "estimated (list price)", never as reported. Codex and Antigravity are priced by the office from a price table **at API prices in US dollars** (the user's rule).

## How a run goes
1. **Wake.** A trigger arrives:
   - a message, ask or task for the agent at its office
   - its schedule
   - the owner pressing Wake
   - a one-off job
2. **Queue.** Each agent has **one wake queue**.
   - Triggers that arrive while the agent is `starting`, `busy`, `stopping`, `needs_approval` or `with_you` are **merged** into the next turn's wake prompt, and added to the live run's trigger list.
   - A schedule that fires during a live run is merged, not a new run.
   - Over the office cap (4 live agents by default), wakes wait in order: urgent first, then oldest. Always-on agents may hold at most cap − 1 slots, so a wake can always get through.
3. **Preflight** for the runtime: installed (the recorded path exists), its version (see "Processes and safety"), signed in (`claude auth status`, Codex `account/read`, the Antigravity equivalent or its first-request error). If the runtime is unavailable on a scheduled or triggered wake, it goes to **Needs you** at once.
4. **Start or resume.** The session is the home session or the task's session (decision 4). If it has a `runtime_session_id` and the runtime can resume it (within the **resume window, 7 days by default**), the office **resumes it** with the replayed spec. Otherwise, or if the resume fails, it starts a **fresh session** with a summary of the last run's record, and records why.
   - Why 7 days: Claude prunes transcripts after 30 days (`cleanupPeriodDays`), the shortest documented retention of the three. 7 days leaves a wide margin, and past a week a fresh session with a summary costs less than a long, stale context.
5. **Send.** The office sends the wake prompt: the guidance for this moment, plus what woke the agent (e.g. "2 new messages; task T-14 assigned"). Message bodies are fetched by the agent through `org-agent` (ADR-0003), not pasted into the prompt.
6. **Work.** Events stream into the office, which:
   - updates the agent's presence (see "Messaging")
   - records each turn's usage
   - routes approvals
   - stores **coarse** events in the run's log (turns, tool calls, approvals, errors). Text deltas are streamed live from memory (`run.event`) and never written one by one, which would cost a `fullfsync` batch each (ADR-0002).
7. **More input.** A normal message for a **busy** agent waits for the turn's end and goes into the next turn. An **urgent** one is steered into the current turn where the runtime supports it (Codex `turn/steer`, Antigravity ACP's steering prompt). Elsewhere (Claude) the office interrupts, then sends. An **idle** agent gets it at once.
8. **Time limits count busy time.** Defaults: **30 minutes** per home-session run and **2 hours** per task-session run, configurable per agent. Time waiting for an approval doesn't count.
   - At the limit, the office interrupts and sends a "checkpoint: summarize your state and stop" turn with a **2-minute grace**, then stops the session. The run ends `max_time`.
9. **Sleep.**
   - An *on-call* or *scheduled* agent's run ends after it has been idle for **5 minutes** (default). The office closes the session gracefully and keeps the `runtime_session_id` for the next wake.
   - An *always-on* agent stays up. If it crashes, the office restarts it with backoff. **Crash-looping is limited:** after 5 restarts in 10 minutes it stops and goes to Needs you.
10. **Desired state.** `agent.stop` sets `stopped` and `agent.wake` sets `running`. An always-on agent that's been stopped stays stopped across office restarts.
11. **Office restart.** At startup, before anything else:
    1. The office kills surviving child groups from the session records (see "Processes and safety").
    2. Runs still open are closed as `interrupted_by_restart`.
    3. Their triggers are re-queued. Always-on agents (desired `running`) resume. On-call agents get their re-queued trigger as a fresh wake.

## Quota
- **A share of each runtime window is reserved for the user's own interactive use: 20% by default**, configurable. New wakes pause above 80% of a window and wait in the queue.
- Sources: Claude's `rate_limit_event`, Codex's `account/rateLimits/updated`. Antigravity has no in-stream event, so the office polls its usage command (`agy -p "/usage" --output-format json`, supervised like any child) **at most every 10 minutes**.
- A turn that hits the limit mid-turn ends the run as `quota`, and its triggers are re-queued for after the reset.
- **Known limit:** each office sees only what its own sessions report, so between reports its view of the window is stale.

## Run modes (Eden's spec)
- `run_mode` and `schedule` are org data on the agent's profile (HQ), changed with an org command (`member.set_run_mode`). The **agent's office** fires schedules. Offices run under the login service, so schedules work with the app closed.
  - Schedules are in the **owner's time zone**, and the office stores each schedule's **last fired time**.
  - A missed run runs once.
  - Three failed wakes in a row go to the owner's Needs you.
- **Asleep is derived, not stored:** an on-call or scheduled agent with no live run is asleep, and an always-on agent with no live session is offline.
- **One-offs.** A one-off is a run of a **temporary agent identity**: an ephemeral Member with `kind: agent` and `profile.status: ephemeral`, created by the office for the job and removed after. `org-agent` can bind as it (with its session token), and it's hidden from the org chart. Its run record and outcome stay.
  - Its workspace is a worktree from `org-git`, attached to its work item. The worktree is removed **only after its branch is pushed through the office** (`projects-git-github.md`). Otherwise it's kept and flagged in Needs you.

## Taking over (a person works with the agent directly)
- **The office spawns the interactive process itself**, through `org-supervise`, with the full replayed spec and the same environment allowlist (a fresh session token included):
  1. The office interrupts and stops the headless process, keeping the agent's lock.
  2. It starts the interactive CLI (`--resume <id>` / `--conversation <id>`) in the session's workspace, in a PTY it owns.
  3. The app attaches to the office's PTY stream, using spike S2's pty-host frames.
- **Lease.** The app sends a heartbeat every **10 s**. If none arrives for **30 s**, the office ends the interactive process and hands the agent back.
- **Hand-back.** For an always-on agent the office resumes headless. For an on-call agent it just releases the lock.
- **Codex:** joining the running thread with `codex --remote unix://<socket>` is a spike item. Until it's proven, Codex take-over works like the others. The app-server's socket lives in a 0700 directory, and its path is never given to agents.
- The agent's presence shows **"With you"** meanwhile.
- The session panel shows the normalized event stream (messages, tool calls, approvals) whether or not anyone has taken over.

## Messaging (ADR-0003) with office-run sessions
- For a session the office runs, the office **is** the wake path. It sends "you have messages" as a turn, at a natural break, using the same urgent/normal triage rules. The inbox hooks (codex-dev's adapters) stay for sessions a person started interactively, such as ours today.
- **Session token.** An agent with a `run_mode` is office-run, and `msg.bind` as it requires the live session's `ORG_SESSION_TOKEN` (ADR-0003 M-A, amended). `org-agent` reads it from its environment. Agents people start interactively keep ADR-0003's accepted risk.
- **Don't block.** In office-run mode the wake guidance tells agents not to block in `msg wait` or `msg ask --timeout` for longer than **60 s**: the office wakes them.
- **Presence** (ADR-0003, amended): `busy`, `idle`, `needs_you`, `with_you`, `asleep` (derived) and `offline`. An agent whose connection holds an `org-agent` wait (`msg.wait` or `msg.ask`) counts as **idle**, though the time still counts toward the run's busy time.
- `org-agent` works the same in both: agents read, answer and defer through it.

## Processes and safety
- **`org-supervise`** (one per office server) is the only code that spawns. It owns:
  - **Spawn:** argv only (no shell); the env allowlist; the cwd from the spec; the recorded absolute binary path; its own process group; stdin, stdout and stderr piped; every other file descriptor closed on exec.
  - **The record:** the child's pid, process group id and start time go into the session record before the first byte is sent. It also writes a small 0600 children file (`<office home>/run/children`, process group ids and start times only).
  - **Stop**, with a timeout at every step: close stdin, wait 30 s; SIGINT, wait 10 s; SIGTERM, wait 10 s; SIGKILL the group. Then reap.
  - **Reaping:** the agent's lock is released only after the child is reaped.
  - **Orphans:** at office startup, before any resume, it kills each recorded group that's still alive (checking the leader's start time, so a reused pid is never hit). As a watchdog for a crashed office, `org-office-host` kills the groups in that office's children file when it sees the office server die, before restarting it.
  - **The Codex app-server** is one supervised process per office, in its own group. Its socket lives in a 0700 directory, and a stale socket is removed at start. Codex sessions are threads on it: stopping one is `turn/interrupt` and closing the thread. If the app-server itself hangs, the stop sequence runs on its group and every Codex session ends `crashed`, which is the trade-off of sharing one process.
  - **Codex per-agent identity.** The app-server's own environment holds no agent identity. `ORG_AGENT`, `ORG_OFFICE` and `ORG_SESSION_TOKEN` are set per thread (`shell_environment_policy` in the thread's config). The spike checks that this works and that the thread config isn't written to the rollout files. If either fails, Codex falls back to one app-server per session over stdio.
- **The stdin protocol:** every line the office writes is built by `serde_json`, never by string formatting. Tests cover `\n`, U+2028 and U+2029 in user text.
- **The stdout reader** splits on `\n` only and caps a line at **4 MiB**. A longer line ends the session as `failed`.
- **stderr** is filtered line by line (control characters dropped, each line length-capped) before it goes to `<office home>/logs/agents/<agent>.log` (0600, capped and rotated, never bodies).
- **Versions:**
  - The office records each runtime's version at every start, and **warns** when it's outside the tested range. It doesn't block.
  - Auto-update is off for office-run processes where possible (Claude `DISABLE_AUTOUPDATER=1`).
  - Each adapter has contract tests against **recorded fixtures** of its protocol.
- **Auth:** each runtime uses the user's own login on this Mac, in the runtime's own store. Shipping to other users needs the vendors' terms checked (U-S1).

## Security
- **The workspace is hostile.** An agent can write anything in its worktree, so nothing there may change how the next session runs:
  - Settings come only from the office (decision 6).
  - The permission policy always **denies writes** to `.claude/**`, `.agents/**`, `.codex/**`, `.gemini/**` and `.git/hooks/**` in the workspace. Repository hooks and settings aren't trusted.
- **Secrets stay out of argv, settings files, logs and the event log.** The office redacts token patterns (GitHub, Anthropic, OpenAI, Google keys) in tool output before it stores an event or streams it. Tool output in the runtime's own transcript (for example `org-agent gh-token`) is covered by the plaintext-transcript decision (U-S3).
- **What the session token stops:** casual and accidental impersonation between agents. It doesn't stop a hostile process running as the same user, which can read another process's environment on macOS. Real isolation (a separate user or an OS sandbox) is v2, as in ADR-0003 and `projects-git-github.md`. Same for the Codex socket: a 0700 directory keeps it from other users, not from same-user processes.

## Contract (org-proto, additive)
**Authorization:** every new call below is **app-bound only** (`app_token`). An agent-bound connection is refused with `not_authorized`. They're added to ADR-0003 §2.1's authorization table, with a test that refuses each one on an agent-bound connection.
- **Agents:**
  - `agent.wake { member_id, prompt? }` → `{ run_id }` (the live run's id when the wake is merged)
  - `agent.stop { member_id }` (desired state `stopped`)
  - `agent.interrupt { member_id }`
  - `agent.send { member_id, text, urgent? }`: the owner talks to an agent directly
  - `permission_policy.get { member_id }`, `permission_policy.set { member_id, policy, expected_version }`
  - `member.set_run_mode { member_id, run_mode, schedule?, expected_version }`: an org command at HQ, like the other `member.*` commands
- **Messaging:** `msg.bind` gains `session_token?`, required when binding as an office-run agent. Without a valid one it's refused with `not_authorized`. This is the one change open to agents.
- **Runtimes:**
  - `runtime.status { runtime? }` → per runtime: installed, path, version, in tested range, signed in, quota windows
- **Runs:**
  - `run.list { member_id?, since?, cursor?, limit? }` → `{ runs, next_cursor }`
  - `run.get { run_id }`
  - `run.events { run_id, from_seq }`, then live as `run.event` through `subscribe`
- **Approvals:**
  - `approval.list`
  - `approval.answer { approval_id, decision: allow_once | allow_always | deny, path_pattern?, note? }`
    - `allow_always` is scoped to (agent, tool, path pattern) and stored in the agent's permission policy.
    - An unanswered approval expires after **30 minutes** (configurable) and counts as a deny.
    - If its session died, answering returns `not_found`.
- **Take-over:**
  - `session.take_over { member_id }` → `{ lease_id, stream }` (the office's PTY stream)
  - `session.heartbeat { lease_id }`, every 10 s
  - `session.hand_back { lease_id }`
- **Events:**
  - `agent.state_changed`
  - `run.started`, `run.finished`, `run.event`
  - `approval.requested` (with the structured input, and the justification labeled untrusted), `approval.answered`
  - `member.presence_changed` with the amended presence values
- **Errors:**
  - `unavailable` with a reason: `not_installed`, `signed_out`, `version` or `quota`
  - `conflict` with the reason `session_held`
  - `not_found`, `not_authorized`, plus the existing codes
  - So `ApiError` gains an optional `reason` (additive), and `conflict`'s doc widens from "stale `expected_version`" to include "held by someone else".
- New enums (session state, run outcome, presence, reasons) decode unknown values as `Unknown`.

## ADR-0003 amendments
Made in ADR-0003, each marked "(amended 2026-11-08: agent sessions, ada)":
- **M-A:** the session token is now issued for office-run agents and required on `msg.bind`.
- **M-B:** the new calls are app-bound only.
- **Presence** (§2.4, with the enums in §4.1 and §4.2): `busy`, `idle`, `needs_you`, `with_you`, `asleep` (derived) and `offline`; `online` stays for people; a held wait counts as idle; office-run agents don't block longer than 60 s.
- **Contract** (§4.1): the agent-session calls are app-only, and `msg.bind` gains `session_token`.

## Deferred to phase 2
- Satellite relaying: waking, approving or taking over an agent placed in a satellite, from the app at HQ.
- Org-wide run and usage roll-ups.

Both arrive alongside `org-sync-proto`.

## Where the code lives
| Crate | What | Owner |
| --- | --- | --- |
| `crates/org-runtime` (new, contract) | the port: `AgentRuntime`, `Session`, `SessionSpec`, `LaunchPlan`, `SessionEvent`, `Capabilities`, usage types. Sync, no I/O. | ada owns the contract; River writes the first version |
| `crates/org-supervise` (new) | spawning, process groups, the env allowlist, the stop sequence, reaping, orphan cleanup | River |
| `crates/org-runtime-claude` (new adapter) | the Claude driver, with fixture-based contract tests | River |
| `crates/org-runtime-codex`, `crates/org-runtime-agy` (new adapters) | the Codex and Antigravity drivers, with fixture-based contract tests | codex-dev |
| `org-office` | sessions, runs, approvals, policies and desired state as domain data; the wake queue and run rules; quota | server-dev |
| `org-office-server` | the composition root: wires `org-supervise` and the adapters, the scheduler, the handlers | server-dev |
| `org-office-host` | the watchdog: kills a dead office's child groups before restarting it | River |
| the messaging library and `org-agent` | the session token on `msg.bind` | ac-software-engineer-remy (Remy, the messaging owner) |
| `apps/desktop` | the session panel (events), take-over (attach to the office's PTY, the lease), Needs you approvals, the Runs tab | app-frontend-dev |
| `org-proto` | the calls and events above | ada |

## Plan
| Step | What | Who | Depends on |
| --- | --- | --- | --- |
| S0 | Spikes, one per runtime:<br>• **Claude:** the `can_use_tool` round trip; which `--setting-sources` value loads no file, and whether user `CLAUDE.md`, skills and agents still load; resume with the spec replayed; resume after the cwd is gone; memory per idle process.<br>• **Codex:** one app-server over a 0700 Unix socket with two threads in two cwds; per-thread env and whether the thread config is written to rollouts; what leaks from the user's Codex home; approvals; interrupt; `--remote` TUI joining.<br>• **Antigravity:** ACP versus stream-json; interrupt; resume; `request_permission`; a custom agent from `--add-dir`; sign-in with `--gemini-dir`. | River (Claude), codex-dev (Codex, Antigravity) | — |
| S1 | `org-runtime` (first version), `org-supervise` and the Claude adapter, with fixture tests and a fake runtime | River | S0 Claude |
| S2 | Contract in org-proto, and the ADR-0003 amendments | ada | S0 |
| S2b | The session token: `org-agent` sends `ORG_SESSION_TOKEN` on `msg.bind`, and the messaging library requires it for office-run agents | ac-software-engineer-remy (Remy, the messaging owner) | S2 |
| S3a | Office: sessions and runs (the wake queue, preflight, start and resume, limits, desired state, restart, orphans), with `org-supervise` wired in | server-dev | S1, S2, the HQ office server (server-dev's phase 1 storage + server) |
| S3b | Office: approvals, schedules and quota | server-dev | S3a |
| S4 | The Codex and Antigravity adapters | codex-dev | S0, S1 |
| S5 | App: the session panel, approvals in Needs you, Runs, take-over | app-frontend-dev | S2, S3a, S3b |
| S6 | **Security tests** (below) | River + server-dev, codex-dev for its runtimes | S2b, S3a, S3b, S4 |
| S7 | End to end: the office wakes an agent on a message, the agent answers through `org-agent`, the office restarts, and the agent resumes in its workspace (all three runtimes; manual checks, plus the opt-in CI smoke test) | River + server-dev | S6 |

**Security tests (S6):**
- A **hostile workspace**, like G2c's hostile git config: `.claude/settings.local.json` allowing everything, hooks, rules files, `.agents/hooks.json`, `.codex` config, `.git/hooks/*`. None of it may take effect, and writes to them are denied.
- The child's environment and open file descriptors hold no `app_token`, database key or GitHub token. Argv holds no secrets.
- Every new call is refused on an agent-bound connection. `msg.bind` as an office-run agent without a valid session token is refused.
- Killing a session's group kills its grandchildren.
- SIGKILL the office, restart it: no survivor, and a single writer per conversation.
- stdin lines with `\n`, U+2028 and U+2029 in user text stay one line. A stdout line over 4 MiB ends the session.
- Tool output with a token is redacted in the event log.

Tests at every step use a **fake runtime** (a test binary that speaks each recorded protocol), so CI never calls a model. The one smoke test per runtime is **opt-in**: it needs a secret and is off by default.

## Open questions for the user
- **U-S1:** We'd run your own Claude, Codex and Antigravity logins for our own team. Before Agentic Org goes to anyone else, the vendors' terms need checking (the research found that Anthropic doesn't allow third-party products to offer claude.ai login without approval), and an API-key path per runtime is required. OK to proceed on that basis?
- **U-S2:** The defaults: up to **4** live agents per office at once; **30 minutes** of busy time per home-session run and **2 hours** per task-session run; **20%** of each quota window reserved for you; an approval expires after **30 minutes**. OK?
- **U-S3:** Each runtime keeps its transcripts as **plaintext** on disk in its own store (`office.db` stays encrypted). OK for phase 1?
