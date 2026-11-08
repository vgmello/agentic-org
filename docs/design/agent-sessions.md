# Design: agent sessions (the office runs the agents)

Status: **proposed** (ada, 2026-11-08), from the user's request:
- start Claude Code, Codex and Antigravity agents
- pass them their instructions
- send them work and get their responses
- resume them in their previous workspace
- manage their processes

It builds on:
- ADR-0001: each office owns its runtime side, meaning sessions, live status and local queues.
- ADR-0003: messaging through the office.
- `offices-hq-and-satellites.md`
- Eden's run-modes spec: always on, on call, scheduled, one-off.

The research behind it, with exact flags, protocols and what was verified, is in `docs/research/agent-runtimes-control.md`.

## Decisions
1. **Headless-first.** The office drives every agent as a child process over the runtime's structured JSON-lines protocol:
   - **Claude Code:** `claude -p --input-format stream-json --output-format stream-json --verbose`, one long-lived process per session.
   - **Codex:** `codex app-server`, JSON-RPC with threads. One app-server per office hosts many threads.
   - **Antigravity:** `agy acp` (the Agent Client Protocol). `agy` stream-json is the fallback if the spike shows ACP isn't ready.

   We don't use Claude's `--bg` daemon + `claude attach` (spike S2) as the main path: it has no programmatic prompt channel, no structured events and no usage stream, and the office needs all three. A person can still work with an agent directly (see "Taking over").
2. **One abstraction, one adapter per runtime.**
   - A small port crate, `org-runtime`, defines `AgentRuntime` and `Session`.
   - `org-runtime-claude`, `org-runtime-codex` and `org-runtime-agy` implement it.
   - The adapters depend only on `org-runtime` and their CLI: never on each other, on `org-office` or on storage.
   - `org-office-server` (the composition root) wires them in. The volatile third-party CLIs sit behind a stable port, so a runtime's protocol change touches one adapter.
3. **The office owns the run, the runtime owns the conversation.** The runtime keeps its transcript in its own store (e.g. `~/.claude/projects/…`). The office keeps:
   - the **session record**: agent, runtime, runtime session id, workspace, the full launch spec, state
   - the **runs**: each wake, with its trigger, start and end, outcome and usage
   - a **bounded normalized event log** per run, for Activity and the session panel

   Prompt and response bodies stay in the runtime's store and in the office's encrypted `office.db`. They are never written to logs.
4. **The full launch spec is replayed on every start and resume.** Claude's `--resume` doesn't restore `--settings`, `--mcp-config`, `--plugin-dir` or `--add-dir`, Codex filters threads by cwd, and Antigravity's conversations are tied to the cwd. So the office stores the whole spec and always resumes in the original workspace.
5. **Isolation by default:**
   - The child gets an **environment allowlist**, not the office's environment. That means `PATH`, `HOME`, `LANG`, `ORG_AGENT=<agent>`, `ORG_OFFICE_HOME` and the runtime's own auth variables only. `CLAUDE_CODE_*`, `AI_AGENT` and similar are stripped.
   - Per-runtime config isolation, so the user's own MCP connectors and plugins don't leak into agents:
     - Claude: `--strict-mcp-config`, `--setting-sources project,local`.
     - Codex: `CODEX_HOME` per office.
   - Spawns are argv-only (no shell), with one process group per session and stdin, stdout and stderr all piped.
6. **Approvals go to the person, never silently denied.** In headless mode all three runtimes deny or soft-deny actions that need approval, so without this an agent silently fails. The adapters bridge approval requests into the office:
   - Claude: `--permission-prompt-tool stdio` and `can_use_tool` control requests.
   - Codex: `requestApproval`.
   - Antigravity: ACP `session/request_permission`.

   The office answers from the agent's **permission policy**, which sets what's allowed without asking (by tool and path), and asks the owner in **Needs you** for anything else. Each runtime's own allow rules and sandbox are set from the same policy as a first line of defence.
7. **Instructions are composed by the office** from:
   - the agent's identity and memory: its home `CLAUDE.md` / `AGENTS.md` and its profile
   - the project's instruction files in the workspace
   - the **wake guidance** for the moment (`docs/.guidance/<moment>.md`, Eden's agent-guidance)

   Each runtime receives them its own way:
   - Claude: `--agent <id>` + `--append-system-prompt`.
   - Codex: `developerInstructions` on `thread/start` / `thread/resume`.
   - Antigravity: a custom agent file `.agents/agents/<id>.md` + `--agent`.

   Claude freezes the system prompt at a session's first request, so instructions that change mid-session are sent as a message ("Your instructions changed: …"), not by restarting.

## The model
- **SessionSpec** `{ agent_id, runtime, model?, workspace (cwd), extra_dirs, instructions, permission_policy, env_allowlist, mcp (office-provided only), mods }`
- **Session** (one live conversation of one agent, at its office) `{ id, agent_id, runtime, runtime_session_id, spec_hash, state, started_at, last_turn_at }`
  - States: `starting` · `idle` · `busy` · `needs_approval` · `needs_input` · `stopping` · `stopped` · `crashed`.
  - **At most one live session per agent**, and the office holds a lock: one writer per conversation.
- **Run** `{ id, agent_id, session_id?, trigger (message | ask | task | schedule | owner | one_off), started_at, ended_at, outcome (done | idle_timeout | max_time | interrupted | failed | crashed), usage }`
  - A run is one wake. Activity and Eden's Runs tab show runs.
- **Usage per turn**: tokens in, out and cached; `cost_usd`, marked reported or estimated; the model.
  - Claude reports `total_cost_usd`, so the adapters turn its cumulative counters into per-turn deltas.
  - Codex and Antigravity report tokens only, so the office prices them from a price table **at API prices in US dollars** (the user's rule) and marks them *estimated*.

## How a run goes
1. **Wake.** A trigger arrives:
   - a message, ask or task for the agent at its office
   - its schedule
   - the owner pressing Wake
   - a one-off job
2. **Start or resume.** If the agent has a session with a `runtime_session_id` and the runtime can resume it (within the resume window, 7 days by default), the office **resumes it** with the replayed spec. Otherwise it starts a **fresh session**, which reads the agent's memory and instructions files.
3. **Send.** The office sends the wake prompt: the guidance for this moment, plus what woke the agent (e.g. "2 new messages; task T-14 assigned"). Message bodies are fetched by the agent through `org-agent` (ADR-0003), not pasted into the prompt.
4. **Work.** Events stream into the office, which:
   - updates the agent's presence (busy, idle, needs you), replacing the hook-based signal for office-run sessions
   - records each turn's usage
   - routes approvals
5. **More input.** A new message for a **busy** agent is queued as its next turn (or *steered* into the current one where the runtime supports it, i.e. Codex). An **idle** agent gets it at once.
6. **Sleep.**
   - An *on-call* or *scheduled* agent's run ends after it has been idle for **5 minutes** (default) or after **30 minutes** of total time. The office closes the session gracefully and keeps the `runtime_session_id` for the next wake.
   - An *always-on* agent stays up, and the office restarts it with backoff if it crashes.
7. **Office restart.** Live sessions die with their office (same as the office servers with the host). On restart, *always-on* agents are resumed, and the others wait for their next trigger.

## Run modes (Eden's spec)
- `run_mode` and `schedule` are org data on the agent's profile (HQ). The **agent's office** fires schedules. Offices run under the login service, so schedules work with the app closed.
  - A missed run runs once.
  - Three failed wakes in a row go to the owner's Needs you.
- **Asleep is derived, not stored:** an on-call or scheduled agent with no live run is asleep, and an always-on agent with no live session is offline.
- A **one-off** is a run of a role or template with a **temporary workspace** (a worktree from `org-git`), attached to its work item and with no member. Its workspace is removed afterwards; the run record and outcome stay.

## Taking over (a person works with the agent directly)
- **Codex:** the app's session panel runs the Codex TUI with `codex --remote unix://<office app-server socket>`, attached to the same thread. Native, so nothing stops.
- **Claude and Antigravity:** take-over.
  1. The office interrupts and stops the headless process (taking the session lock).
  2. The app's terminal panel runs the interactive CLI with `--resume <id>` / `--conversation <id>` in the same workspace, under the pty host from spike S2.
  3. When the person closes the panel, the office resumes headless.

  The agent's presence shows **"With you"** meanwhile.
- The session panel shows the normalized event stream (messages, tool calls, approvals) whether or not anyone has taken over.

## Messaging (ADR-0003) with office-run sessions
- For a session the office runs, the office **is** the wake path. It sends "you have messages" as a turn, at a natural break, using the same urgent/normal triage rules. The inbox hooks (codex-dev's adapters) stay for sessions a person started interactively, such as ours today.
- `org-agent` works the same in both: agents read, answer and defer through it.

## Processes and safety
- **One supervisor per office** (in `org-office-server`), with:
  - a cap on concurrent live sessions per office (4 by default, configurable)
  - quota awareness: each runtime's rate-limit events pause new wakes when a window is nearly used up
- **Spawn:** argv only; the environment allowlist; the cwd is the agent's workspace; its own process group; stdout carries the protocol, and stderr goes to `<home>/logs/agents/<agent>.log` (0600, capped and rotated, never bodies).
- **Stop:** graceful first (close stdin / unsubscribe), then SIGINT, then SIGTERM after 10 s, then SIGKILL to the group.
- **Pinned runtime versions:**
  - The office records each runtime's version at every start.
  - Each adapter has contract tests against **recorded fixtures** of its protocol.
  - A version outside the tested range shows a warning, and a start fails clearly instead of misparsing.
- **Auth:** each runtime uses the user's own login on this Mac (or an API key the user configures). The office never stores runtime credentials. Shipping to other users needs the vendors' terms checked: the research found that Anthropic doesn't allow third-party products to offer claude.ai login without approval, so an API-key path per runtime is required before any release beyond our own use.

## Contract (org-proto, additive)
- **Calls:**
  - `agent.wake { member_id, prompt? }` → `{ run_id }`
  - `agent.stop { member_id }`
  - `agent.interrupt { member_id }`
  - `agent.send { member_id, text }`: the owner talks to an agent directly
- **Runs:**
  - `run.list { member_id?, since? }`
  - `run.get { run_id }`
  - `run.events { run_id, from_seq }`, then live through `subscribe`
- **Approvals:**
  - `approval.list`
  - `approval.answer { approval_id, decision: allow_once | allow_always | deny, note? }`
- **Take-over:**
  - `session.take_over { member_id }` → the resume command for the panel
  - `session.hand_back { member_id }`
- **Events:**
  - `agent.state_changed`
  - `run.started`
  - `run.finished`
  - `approval.requested`
  - `approval.answered`
- **Errors:** `unavailable` (the runtime isn't installed or signed in), `conflict` (the session is held), `not_found`, plus the existing codes.

## Where the code lives
| Crate | What | Owner |
| --- | --- | --- |
| `crates/org-runtime` (new, port) | `AgentRuntime`, `Session`, `SessionSpec`, normalized `SessionEvent`, `Capabilities`, usage types. No I/O. | River |
| `crates/org-runtime-claude`, `-codex`, `-agy` (new adapters) | the CLI drivers, each with fixture-based contract tests | River: Claude (with the supervisor). codex-dev: Codex and Antigravity (they already know their hooks and protocols) |
| `org-office` | runs, sessions, approvals and policies as domain data; the wake rules | server-dev |
| `org-office-server` | the supervisor (spawn, lock, restart, caps, quotas), the wiring, the handlers | server-dev, with River's supervisor library |
| `apps/desktop` | the session panel (events), take-over (pty host), Needs you approvals, the Runs tab | app-frontend-dev |
| `org-proto` | the calls and events above | ada |

## Plan
| Step | What | Who | Depends on |
| --- | --- | --- | --- |
| S0 | Spikes, one per runtime:<br>• **Claude:** the `can_use_tool` approval round trip; take-over and hand-back; resume with the spec replayed; memory per idle process.<br>• **Codex:** the app-server over a Unix socket with two threads in two cwds; approvals; interrupt; `--remote` TUI joining.<br>• **Antigravity:** ACP versus stream-json; interrupt; resume; permission requests. | River (Claude), codex-dev (Codex, Antigravity) | — |
| S1 | `org-runtime` port + the Claude adapter + the supervisor library, with fixture tests and a fake runtime | River | S0 Claude |
| S2 | Contract in org-proto | ada | S0 |
| S3 | Office: sessions, runs, approvals, wake rules, the supervisor wired in, schedules | server-dev | S1, S2, W2 |
| S4 | The Codex and Antigravity adapters | codex-dev | S0, S1 |
| S5 | App: the session panel, approvals in Needs you, Runs, take-over | app-frontend-dev | S2, S3 |
| S6 | End to end: the office wakes an agent on a message, the agent answers through `org-agent`, the office restarts, and the agent resumes in its workspace (all three runtimes; Claude in CI with a cheap model, the others as manual checks) | River + server-dev | S3, S4 |

Tests at every step use a **fake runtime** (a test binary that speaks each recorded protocol), so CI never calls a model, except for one opt-in smoke test per runtime.

## Open questions for the user
- **U-S1:** We'd run your own Claude, Codex and Antigravity logins for our own team. Before Agentic Org goes to anyone else, the vendors' terms need checking (and an API-key path per runtime is required). OK to proceed on that basis?
- **U-S2:** The default caps: up to 4 live agents per office at once, an on-call run ends after 5 minutes idle or 30 minutes total. OK?
