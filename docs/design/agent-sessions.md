# Design: agent sessions

How the office runs an agent's vendor tool, keeps it isolated, and lets people work
with it. [Execution](organization-data/11-model-profiles-and-agent-execution.md) owns
the turn, attempt, process, permit and placement contract;
[employees](organization-data/04-agent-structure.md),
[connections and pools](organization-data/14-model-providers-and-connection-pools.md),
[recovery](organization-data/16-recovery-and-office-continuity.md) and
[budgets](organization-data/17-budgets-and-controls.md) own their data. Supervision
of the control loop is in [employee supervision](employee-supervision.md); the Git
paths and their deny rules are in the [Git design](projects-git-github.md). Vendor
tool research is in [agent runtimes control](../research/agent-runtimes-control.md).

The decisions this design applies:

- Agents run headless; people talk to them through chat; there is no terminal in
  the browser ([ADR-0004](../adr/0004-headless-agent-sessions.md)).
- Each agent has a private home and runs in an isolation mode, `same-user` or
  `dedicated-user` ([ADR-0015](../adr/0015-agent-isolation-modes.md)).
- A message never interrupts a busy turn; an agent that cannot continue without an
  answer is suspended and resumed ([ADR-0003](../adr/0003-agent-messaging-office-first.md),
  [ADR-0020](../adr/0020-required-asks-suspend-and-resume.md)).

## Decisions

1. **Headless.** The office drives every agent as a child process over the vendor
   tool's structured JSON-lines protocol:
   - **Claude Code:** `claude -p --input-format stream-json --output-format stream-json --verbose`,
     one long-lived process per session.
   - **Codex:** `codex app-server` over stdio, one process per session with one
     thread. A shared app-server waits until its threads are proven isolated from each
     other and from agents.
   - **Antigravity:** `agy acp` (the Agent Client Protocol), with `agy` stream-json
     as the fallback only if ACP is not ready.
2. **One port, one adapter per runtime, one supervisor.**
   - `org-runtime` is the **port**: `AgentRuntime`, an optional `RuntimeHost`,
     `Session`, `SessionSpec`, `LaunchContext`, `LaunchPlan`, the normalized
     `SessionEvent`, `Capabilities`, usage types, and the one line framer and encoder
     every adapter and the pump share. It is a contract and does no I/O. The port is
     synchronous.
   - `AgentRuntime` is a stateless factory per runtime: its capabilities, the launch
     plan and `Session` for a session, and the launch plans and output parsing of its
     **probes** (preflight, and a usage poll where needed). `RuntimeHost`, a shared
     host process serving several sessions, is added only when a runtime shares a
     process.
   - `Session` is **sans-I/O**: a pure state machine for one runtime's protocol. The
     pump tells it what happened (`start`, `on_line`, `on_truncated`,
     `on_stderr_line`, `on_stdout_closed`, `on_exit`, `on_tick(now)`, and `control`
     for an office command), and it returns outputs in order: `Write` (one line for
     stdin), `Event`, `CloseStdin` or `Abort` (stop the process now with the fast
     stop). It never spawns, reads, writes, sleeps or reads the clock, so it is tested
     from recorded fixtures alone.
   - Adapters (`org-runtime-claude`, `org-runtime-codex`, `org-runtime-agy`) build a
     launch plan (argv, environment additions, files to write) and a `Session`. They
     never spawn or touch a pipe, and depend only on `org-runtime`.
   - `org-supervise` is the one place that spawns agent runtimes and their probes. It
     owns process groups, the environment allowlist, the stop sequence, reaping and
     orphan cleanup, and speaks no runtime protocol. In `dedicated-user` mode it starts
     and stops agents through the privileged launcher (ADR-0015 decision 4).
   - **The lock** on an agent is released only after verified cleanup and reaping of
     its owned processes and their descendants.
   - **`org-office` depends on neither `org-runtime` nor `org-supervise`.** It holds
     the execution rules as domain logic and defines the small trait it needs
     (`SessionControl`: start, send, interrupt and stop a session by ID, and probe a
     runtime). The integration library `org-office-sessions` implements it with
     `org-supervise` and the adapters, maps the office's session record to a
     `SessionSpec` and `LaunchContext`, and runs the **pump**: one implementation for
     every runtime, which owns the child's pipes, frames stdout and stderr, feeds each
     line to the `Session`, applies its outputs in order, calls `on_tick` at the next
     deadline and feeds the facts into `org-office`. It also implements `SessionAuth`
     (see [Messaging](#messaging)). `agentic-org-office` wires it in.
3. **Durable facts, disposable projections.** Organization Git owns employee and
   execution definitions. Protected execution journals own turn, attempt and process
   transitions, immutable inputs, tool receipts, checkpoints and command outcomes;
   HQ's journal owns placement generations, permits and cooldowns. The database
   indexes these. A runtime transcript alone never establishes whether an effect
   completed.
4. **Identity, placement and workspaces are separate.** An employee has one home
   team in `profile.md`; only an agent has `execution.md`, selecting one pool and one
   model. Placement changes independently of identity, with fencing. Task workspaces
   live under `runtime/<office-id>/teams/…/agents/<employee-id>/` as the
   [storage contract](organization-data/README.md#shared-storage-contract) shows, one
   per task and repository. A session's working directory is an approved workspace
   reference, never caller input. One open turn and one main process exist per agent.
   Handoff, process exit and the end of a turn are not task closure: worktrees and
   unpushed work are kept. A missing workspace is a recovery issue, restored only
   from the verified repository and retained task state, with any unpushed loss
   disclosed.
5. **The whole launch spec is replayed on every start and resume.** Document 11
   freezes input, pool, model, workspace and policy per turn. A changed launch digest
   for the next turn needs a controlled cleanup and relaunch. Claude's `--resume`
   does not restore `--settings`, `--mcp-config`, `--plugin-dir` or `--add-dir`, a
   `-p` resume starts in manual permission mode, Codex filters threads by working
   directory, and Antigravity's conversations are tied to it. So the office stores the
   whole spec and replays it, in the session's own workspace.
   - The spec includes the permission mode and the sandbox. Claude always gets
     `--permission-mode`. Codex gets `approvalPolicy`, `approvalsReviewer: "user"` and
     `permissions` (the session's named profile) on every `thread/start`,
     `thread/resume` and `turn/start`, and never the legacy `sandbox` or
     `sandboxPolicy`, which would override the profile.
   - When the stored instructions differ from the new ones on resume, the office sends
     the change as a message ("Your instructions changed: …"), because Claude keeps the
     system prompt it recorded at the first request.
   - **Bypass modes are banned everywhere.** Claude: `bypassPermissions` and
     `--dangerously-skip-permissions`, with `disableBypassPermissionsMode: "disable"`
     in the office's settings. Codex: `danger-full-access`,
     `--dangerously-bypass-approvals-and-sandbox` and its aliases, and
     `--dangerously-bypass-hook-trust`. Antigravity: `--dangerously-skip-permissions`.
6. **Isolation by default.**
   - **Private home.** Each agent has a home at
     `durable/local/offices/<office-id>/agents/<employee-id>/home/`. The office starts
     the agent with `HOME` and each vendor's configuration location pointing into it.
     The vendor folder there holds only the office's settings, the agent's own
     transcripts and the one sign-in the pool selected, copied in for the run and taken
     back when it ends (ADR-0015 decisions 10 and 11). The person's own `~/.claude`,
     Codex home and `~/.gemini` are never read or written by an office-run agent.
   - **Environment allowlist.** The child gets only:
     - `PATH`: the instance's pinned release `bin/` first (it holds `agent-os`, which
       therefore never resolves to a user-writable file), then the `PATH` captured once
       from the service's login shell, without relative entries or entries inside the
       workspace or home. Every user-writable folder on it is a fixed write deny.
     - `HOME` (the private home), `LANG`, `USER`, `LOGNAME`, `SHELL`.
     - `TMPDIR`: the session's own directory (below).
     - `AGENT_OS_SOCKET` (the exact resolved office socket), from the launch.
     - `AGENT_OS_SESSION_TOKEN`: a random token per run (one process's life), held only in
       office memory, required on `msg.bind`, never in argv or a file, never logged.
     - `AGENT_OS_SESSION_MARK`: a random value per session, recorded before spawn, used to
       find orphans. It is not a secret.
     - `AGENT_OS_ORG`, `AGENT_OS_OFFICE`, `AGENT_OS_AGENT_ID`, `AGENT_OS_SESSION_ID` and,
       when the run has them, `AGENT_OS_PROJECT` and `AGENT_OS_TASK`: context for the
       agent's own scripts. They are never identity: the office takes who is calling
       from the binding ([agent command line](agent-os-command-line.md), section 2.8).
     - Git: `GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL=/dev/null`, and through
       `GIT_CONFIG_COUNT`/`GIT_CONFIG_KEY_n`/`GIT_CONFIG_VALUE_n` the commit identity, an
       empty `credential.helper`, `core.hooksPath=/dev/null`, `core.fsmonitor=false`,
       `gc.auto=0` and `maintenance.auto=false`.
     - `GH_CONFIG_DIR=<TMPDIR>/gh/`, an empty folder, so `gh` sees no login. Agents hold
       no GitHub or Git credential; they publish only through `agent-os repo push`
       ([Git design](projects-git-github.md)).
     - Office-set runtime switches such as `DISABLE_AUTOUPDATER=1`.

     Everything else is dropped, including `SSH_AUTH_SOCK`, `GH_TOKEN`, other `GIT_*`
     variables and any API key. None of these values is a file an agent can write.
     Runtimes must pass this environment to their tools: Codex's
     `shell_environment_policy` drops names matching `*TOKEN*` by default, so the office
     pins `shell_environment_policy.ignore_default_excludes=true` and `include_only` to
     the names above.
   - **`TMPDIR`** is `<user temp>/ao/<instance>/<office>/<employee>/<session>/`, each
     component a short random handle allocated exclusively (never derived from a client
     ID), each level `0700`, owned by the service user, not a symlink. `<user temp>` is
     `DARWIN_USER_TEMP_DIR` on macOS and, on Linux, one validated disk-backed private
     directory per OS user (never a shared `/tmp`). The host allocates the instance and
     office handles and passes the office's over the launch rendezvous; the office
     allocates employee handles at hire and a session handle per session. The string is
     at most 71 bytes, so `TMPDIR` plus a 32-byte suffix fits the 103 usable bytes of a
     macOS socket path; a longer one refuses the start. The host and office re-create
     the folders at startup and before each launch, so a reboot or temp cleanup never
     leaves a deny target missing. Orphaned session folders are swept when the office
     starts.
   - **Runtime binaries** run by their recorded real paths, never looked up on `PATH`.
     Writes to them and to every user-writable folder they sit in are fixed denies.
   - **Claude's isolation switches.** `--setting-sources ""` loads no settings or
     instruction file and no user or project skills, agents or commands.
     `autoMemoryEnabled: false` in the office's `--settings <file>` (`0600`), the only
     settings Claude gets. `--strict-mcp-config` with the office's `--mcp-config <file>`.
     The adapter checks every `init`: every `init.plugins` entry is built in and
     `init.mcp_servers` lists only the office's servers; otherwise the start fails, or
     the session is stopped at once with the fast stop.
   - **Generated runtime files** (settings, MCP configuration, instructions) are
     written by the office before every start, so an agent's edits never survive, and
     are read only by the runtime itself. Agents' tools are denied them where the
     runtime allows; whether a runtime can keep them out of a home the agent can write
     is part of the sign-in spike (ADR-0015 decision 7).
   - **Runtime sandboxes are on.** Codex through a named permissions profile, Claude's
     Bash sandbox, Antigravity `--sandbox`.
     - Claude's settings pin `autoAllowBashIfSandboxed: false` and
       `allowUnsandboxedCommands: false`.
     - Writable roots are set by the office: the workspace, the session's `TMPDIR`, and
       for a task session the parts of the repository cache a commit needs (Git
       design). Where a sandbox opens more by itself, the office adds `denyWrite`
       entries.
     - Each sandbox allows exactly the office socket in `AGENT_OS_SOCKET` (Claude
       `allowUnixSockets`, Codex `network.unix_sockets`) and refuses every other socket.
     - **Codex's profile is required and fails closed.** One `-c` value holds the whole
       `permissions.<name>` table, plus `-c features.network_proxy=true`. It extends
       `:workspace`, adds the writable roots and the fixed read and write denies, turns
       network on with no domain allowed, and allows the office socket. The legacy
       `sandboxPolicy` has no read deny, so Codex is not office-run until the profile
       holds every fixed deny in live turns.
   - **Claude's tools are an explicit allowlist:** `Read`, `Edit`, `Write`, `Glob`,
     `Grep`, `Bash` (sandboxed) and the tools of the connectors the office granted
     this agent, nothing else. No product command is an MCP tool, and milestones one
     and two grant no connector, so the MCP configuration is empty
     ([agent command line](agent-os-command-line.md), section 2.10). Claude's
     other tools act outside the office or the sandbox (cross-session messaging,
     scheduling and remote triggers, background and sub-agent work, worktrees, web
     access). The adapter passes the built-in names as `--tools`, the whole list as the
     inline agent's `tools`, and every other known tool in `--disallowedTools`.
     `ToolSearch` is disallowed and `ENABLE_TOOL_SEARCH=false`, so a granted
     connector's tools are never deferred. `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1` turns off Bash's
     background mode. The adapter checks every `init`: `init.tools` must be a subset of
     the allowlist, or the start fails or the session is stopped with the fast stop
     (an in-band `interrupt`, every later `can_use_tool` denied, `Abort`). Remote
     control stays off and a session reporting it active fails. Web access is off. Codex
     and Antigravity follow the same rule for their own risky built-in tools; a runtime
     whose risky tools cannot be turned off is not office-run.
7. **Approvals go to a person, never silently lost.** Headless runtimes deny actions
   that need approval, so the adapters bridge approval requests into the office:
   Claude `--permission-prompt-tool stdio` and `can_use_tool`, Codex
   `item/*/requestApproval`, Antigravity ACP `session/request_permission`. The office
   answers from the agent's **permission policy** and asks a person in **Needs you**
   for anything else.
   - **Escalation.** Every request carries `escalation`: `none`, `sandbox_bypass`,
     `permission_change` or `unknown`. `sandbox_bypass` is always denied.
     `permission_change` and `unknown` always go to a person with a warning. A request
     for a disallowed tool is denied at once and recorded. `approval.answer` refuses
     `allow_always` and `scope` on an escalated approval (`invalid_request`, reason
     `escalated`), and `permission_policy.set` refuses an allow rule for a tool that
     always escalates (reason `escalating_tool`).
   - **The office allows or denies the original call only.** It never sends rewritten
     input (`updatedInput`) or permission updates (`updatedPermissions`), and ignores
     `permission_suggestions`.
   - A deny carries a message the model sees, and the turn goes on. Claude's
     `interrupt: true` is used only when the office means to end the turn.
   - **Expiry.** An unanswered approval expires after 30 minutes (configurable) and is
     a deny; waiting does not count toward run time. A request the runtime withdraws is
     `cancelled`, and so is one pending when its turn or process ends. At startup,
     pending approvals of dead sessions become `expired`. Answering any of these returns
     `not_found`.
   - Needs you shows the structured tool input; the agent's own justification is shown
     labelled untrusted. A policy rule is a tool plus a path pattern, a command prefix
     or an MCP server. `allow_always` is a versioned policy change.
   - **No allow rule reaches managed state.** `permission_policy.set` and
     `approval.answer` refuse an absolute pattern whose normalized prefix could match the
     instance root, another instance's root, the instance registry, credential paths,
     the release payload or the private runtime directory (`invalid_request`, reason
     `managed_state`). `~` is expanded, `..` refused, and the longest existing prefix is
     resolved before testing overlap. Workspace-relative rules cover only the session's
     own workspace.
   - **Every file write reaches the office.** The office never writes an allow rule for
     a file-writing tool into a runtime's own rules, and headless Claude never runs in
     `acceptEdits`. A write is checked at its real path: under managed state and outside
     the writable roots it is denied at once, even when allowed once; a write whose
     literal path matches a rule but whose real path does not (a planted symlink) goes
     to a person. Codex raises no approval inside its writable roots, so for Codex the
     sandbox's own real-path enforcement holds.
   - **The fixed denies override every allow rule.** If a runtime's approval bridge
     does not work, actions needing approval are denied and the run ends `blocked`.
8. **Instructions are composed by the office** from the employee's identity and role
   (`profile.md`), its prompt and tools (`execution.md`), the turn's explicit
   objective, the effective processes and standards
   ([process design](processes.md#32-delivery-to-sessions-proc-7)), built-in `agent-os`
   help, and the project's instruction files read with the office's hardened Git from
   each association's `integration_branch` in a verified cache. Nothing an agent
   writes in its worktree changes what the office tells the next session. Instruction
   files a runtime reads from its working directory on its own (`AGENTS.md`,
   `.agents/rules`) are the task branch's content and steer only that task's sessions;
   Claude was observed to read none under `--setting-sources ""`; a nested
   `CLAUDE.md` loaded lazily is untested and is treated as untrusted. Each runtime
   receives the
   instructions its own way:
   - **Claude:** `--agents <json>` defines the agent inline and `--agent <id>` selects
     it; the instructions go in `--append-system-prompt-file`. The ID is reserved,
     `org-<employee id>`, and the description is the office's own. An unknown `--agent`
     falls back silently, so the adapter sends `initialize` first and fails the start
     unless the list holds the reserved ID with the exact description.
   - **Codex:** `developerInstructions` on `thread/start` and `thread/resume`.
   - **Antigravity:** unavailable until an installed version's fixtures prove how a
     custom agent is loaded, selected and isolated.

   New instructions apply to the next turn; recovery keeps the admitted input.
9. **Office-run sessions load no mods and no inbox hooks.** The office owns input,
   presence and control.

## The model

| Record | Meaning and authority |
| --- | --- |
| Employee configuration | An agent with an `execution.md`: one `model_profile_id`, the model, tools and system prompt. Required before it can run. |
| Model profile (pool) | 1 to 64 connection IDs in contiguous priority order; models come from confirmed discovery. |
| Provider connection | Organization-wide; one per provider subscription, reused across offices and pools. |
| Local binding and observations | A private credential reference and binding revision per office and connection. Authentication and model evidence is fresh for at most 300 s; probes run outside the routing path. |
| Turn | One immutable admitted input, model, pool, workspace and policy, with one employee, office and placement generation. States: accepted, running, switching, blocked, recovery_required, succeeded, failed, cancelled. |
| Attempt | The connection, permit and optional process actually used; failed attempts keep their usage. |
| Process | planned → starting → idle/busy → stopping → gone; uncertainty is `cleanup_required` and keeps the slot. |
| Permit | HQ-coordinated connection capacity bound to employee, office, placement, model and coordinator generation. |
| Approval | A bounded tool request with its attempt; expiry denies it. |
| Holds | Manual, security and budget reasons, independent of employment, task and process state. `stop_pending` is not `held`. |
| Suspension | An agent waiting on a required ask (ADR-0020). |

A vendor conversation is adapter context; it is neither the turn nor a process.

### The adapter contract

- **SessionSpec** (stored with the session and hashed) `{ runtime, kind (home | task), agent (name, description, prompt), model?, effort?, workspace, read_only_dirs, instructions, permission_mode, policy, sandbox (enabled, writable_roots, deny_write, deny_read, allowed_unix_sockets), mcp_servers (the office's only), tools, isolation_mode }`
  - `tools` is required and ordered; there is no runtime default.
  - `isolation_mode` is `same-user` or `dedicated-user` (ADR-0015 decision 1).
  - Ordered collections only and no per-run value, so every start and resume of a
    session has the same `spec_hash`, a 64-bit FNV-1a over the canonical JSON: a change
    detector, not a security check.
  - **Paths are UTF-8 by type.** Every path is a `SpecPath`: absolute and normalized.
    It refuses every control character, U+2028 and U+2029, and every format character
    as of Unicode 17.0 (pinned by a test). `PathPattern` and `CommandPrefix` use the same
    set. Conversion from an office path returns a `Result`; an invalid path is an error.
    Writable roots and allowed sockets resolve their parent to its real path and refuse
    a symlink in the last component. Deny entries are added both as written and in their
    real-path form, resolving the longest existing prefix; a failure to resolve refuses
    the start.
  - Unknown fields fail to decode. Enums the office writes fail closed on an unknown
    value; enums a runtime sends decode an unknown value as `Unknown`.
- **LaunchContext** (per run, not hashed) `{ runtime_path, runtime_version?, runtime_dir, tmpdir, office_env_names, start_denies (deny_read, deny_write), new_session_id?, resume? }`.
  `start_denies` are the denies enumerated at each start (other agents' workspaces and
  homes, other offices, user-writable `PATH` folders). `resume` is the vendor session
  to resume. The adapter sees environment variable names only, never the token or mark.
- **The `Session` contract,** checked by the port's testkit against each adapter's
  fixtures:
  - Input during a turn returns `Busy`; the office keeps it for the next turn boundary.
    A message never interrupts or steers a busy turn. An explicit Interrupt or Stop by
    an authorized person, and security, offboarding, fencing and recovery stops, end
    the turn through `control`.
  - When a call returns `Err`, nothing was emitted and the state is unchanged.
  - `Ended` is the last event, exactly once, from `on_exit`; after it every call
    returns nothing and `control` returns `Closed`.
  - `Output` and the framer's `Frame` are exhaustive, so a new variant breaks the
    pump's build until it is handled.

### Usage

The accounting ledger of document 10 is the source of truth. Adapters report tokens
in, out and cached, and the model, per turn:

- **Claude:** a turn's `result` reports its usage, but a turn aborted mid-stream
  reports zero. The office keeps a running per-message sum from the turn's `assistant`
  events (once per `message.id`), using `message_delta` for the final output count
  (`--include-partial-messages`), and uses that sum when the `result` is zero or
  missing.
- **Codex:** `thread/tokenUsage/updated.last`.
- **Antigravity:** cumulative usage; a baseline at the first report after a start or
  resume, then deltas.

Cost uses the price versions of document 10 in integer micro-USD. A missing report is
never stored as zero. Claude's `total_cost_usd` is the vendor's estimate, labelled as
such if shown.

## How a turn runs

A turn starts only from an explicit instruction, the answer to a required ask, an ask
or a hand-off from another agent to an idle agent
([ADR-0023](../adr/0023-an-agents-ask-starts-a-turn.md)), or a recovery. Other
messages, assignments and schedules never create model work by themselves.
A second open turn returns `BUSY`.

1. **Admit.** Authenticate the caller and scope, validate placement and configuration
   revisions, persist the immutable input and the actor-bound operation receipt before
   acknowledging. A same-ID retry returns that outcome.
2. **Choose a connection** for the same model, traversing the frozen pool once per
   dispatch cycle: enabled definitions, matching local binding, verified installed
   driver, fresh authentication and compatible tools and policy. Skips carry reasons.
3. **Acquire a permit** from HQ. If HQ cannot be reached, the turn is blocked. The
   default is one concurrent turn per connection across all offices and pools.
4. **Reuse or switch.** Reuse a verified idle process only when connection, model,
   driver version, workspace and launch digest match; otherwise verify cleanup of the
   old one first. Record the mark before spawn and the boot, start and PID identity
   before input. Credentials never enter argv or logs.
5. **Execute and account.** Journal attempts and tool intent and results. Streamed
   text is provisional until the final outcome. Record the actual route's usage,
   including failed and partial attempts.
6. **Handle failure.** Retry only eligible quota, rate, authentication or model
   failures. Replay only with proof of no effects, or resume a compatible checkpoint
   with completed receipts; otherwise `recovery_required`, keeping slot, workspace and
   evidence. Never ask a model to "check its state" instead of reconciling.
7. **Finish.** Turn success is separate from task closure. Capacity is released only
   after a verified terminal or idle boundary. A compatible idle process may stay warm
   without a permit.
8. **Recover.** The office's single recovery owner replays journals, validates
   receipts and reconciles marked processes before admitting work. It never closes an
   unknown turn automatically or requeues its effects.

### Quota

HQ journals permits and cooldowns before acknowledging. Permits last 60 s and renew
every 10 s; a renewal failure starts stop and recovery with at least 20 s left. An
expired permit is `release_pending` and still consumes capacity. A 429 applies to the
whole connection unless verified provider evidence narrows it to a model; the cooldown
is Retry-After or the provider reset, else 900 s. Each candidate is tried once per
cycle; an exhausted pool blocks with ordered skip reasons and needs an explicit
ResumeTurn. A cached healthy flag is never quota authority.

Budgets are independent of provider quota (document 17). Known usage at a limit
creates a `stop_pending` hold that lets the current turn finish and stops new
admission; only the office's acknowledgement makes it `held`. Unknown usage is shown
incomplete and warns. Release caps never stop turns.

### Pause, suspension and offboarding

Pause, holds and offline offices follow [employee supervision](employee-supervision.md#2-pause-holds-and-offline-offices).
`StopAgent` stops the agent's active and idle processes and creates no future start.
It is not Pause: the `agent.stop` call is Pause, which records a held manual pause and
then runs `StopAgent`.

A required ask that times out suspends the agent: the office tells it, waits for its
turn to end or the grace period, stops its process and any process it started, and
records the suspension. The answer starts the agent again by resuming the same vendor
session with the answer as its next input, through the normal start checks
(ADR-0020 decisions 5 to 12).

When an agent is offboarded or removed:

- Its sessions stop at once, with no checkpoint turn; the run ends `offboarded`.
- Its session token is void, every connection bound with it is closed, and no new
  token is issued.
- Its pending approvals are `cancelled`.
- Task worktrees are kept until task closure or a verified transfer; committed
  branches are pushed where possible, and uncommitted or unpushed work is flagged.
- Session and run records stay for history; transcripts stay in its private home.
- Control calls on it are refused with `invalid_request` (reason `offboarded`), or
  `not_found` once removed. The employee's identity and history are archived, never
  deleted.

## Taking over

There is no terminal in the browser. A person at the office's machine takes an agent
over with an `agentic-org` command (ADR-0004 decision 4):

1. The office stops the headless run (for Codex, `thread/unsubscribe` first), keeps
   the agent's lock, and ends the run as `taken_over`.
2. It gives the authorized local caller the session's launch description. The
   command starts the vendor tool's interactive resume in the person's own terminal,
   with the agent's private home, the replayed spec, the same environment allowlist and
   a fresh session token: `claude --resume <id>`, `codex resume <thread id>` with the
   same `-c` pins, or `agy --conversation <id>`. In `dedicated-user` mode it goes
   through the launcher.
3. When the person exits, the command hands the agent back and the office resumes it
   headless.

The office owns no terminal and carries no terminal data. The agent's presence shows
**With you** meanwhile, and other control calls get `held`. The browser shows the
session read-only ([REQ-05](../specs/req-05-virtual-console-conversation-log-viewer.md)).
A person in Codex's interactive tool can change its sandbox or approval policy from
inside, and a person in Claude's can switch to `acceptEdits`; the office pins both
again on hand-back, and the command warns about this when it starts.

## Messaging

- **Binding.** An office-run agent binds with `msg.bind` and both the live run's
  `AGENT_OS_SESSION_TOKEN` and **peer ancestry**: the office reads the caller's process
  identity from the socket (on macOS the audit token, so a reused PID does not match)
  and walks its parent chain, which must include the session's recorded child.
  Otherwise `not_authorized`. Tokens are compared in constant time. The port is
  `SessionAuth` in `org-mailbox`, implemented by `org-office-sessions`.
- **It fails closed.** Binding as an offboarded agent, an agent whose status the
  office cannot read, or an unknown employee is refused, and so is a bind when the
  lookup fails.
- When a run's process exits, its token dies and the office closes every connection
  bound with it. A bound connection's descriptor passed to another process keeps
  working until then: ancestry is checked at bind only.
- **Holds are capped.** One held call on a token-bound connection lasts at most 60 s.
  An `ask` waits as ADR-0020 says: `agent-os` renews the hold until the answer, the
  timeout, a required-answer-pending result or a withdrawal.
- **No file queue.** `agent-os` never writes to a local queue; every call goes to
  the office.
- **Delivery** follows ADR-0003: a message is kept for the agent's next turn boundary,
  never interrupts a busy turn, and never wakes a paused agent. A reply that satisfies
  a wait creates no second turn. An ask or a hand-off from another agent starts one
  turn for an idle recipient, under an hourly limit (ADR-0023).
- **Presence:** `busy`, `idle`, `needs_you`, `with_you`, `waiting` (on a required
  answer, naming whom), `asleep` and `offline`. An agent holding an `agent-os` wait
  counts as idle. Presence is not stored durably.

## Processes and safety

The supervisor and session runner are modules of the one `agentic-org-office`
process. A single recovery owner and writer apply the journal transactions.

The mark is recorded before spawn and the boot and start identity before input.
Incomplete ownership and the captured descendant inventory are kept until cleanup is
verified; deadlines, PID or group numbers, a vanished cache or a successful push never
prove an exit. The ownership and recovery policy is the one implemented in
[`org-supervise`](../../crates/org-supervise/README.md): checkpoint before signal,
exact identity checks, bounded scans and its documented containment limits. A failed
cleanup keeps the slot. The host never races the office over a live agent; on a move
to another office, HQ needs verified cleanup of the old process or an explicit machine
fence, then journals the new office and an incremented placement generation, which
every command, permit and attempt carries.

Drivers prove their installed version, local account binding, isolation and policy,
checkpoint and usage support before they are eligible. An unknown version does not
qualify. Tool children receive no provider secret or host key.

- **stdin:** every line comes from the port's encoder (`serde_json`), which also
  escapes U+2028 and U+2029. Every office-sent user line starts with the office's
  header, so its text never starts with `/` (which Claude treats as a slash command);
  the encoder refuses one that does.
- **stdout:** the port's `LineFramer` splits on `\n` and caps a line at 4 MiB; a longer
  line is drained and replaced by a `truncated` event. An unknown event type becomes a
  coarse `unknown` event; a non-JSON line a `protocol_error` event. Only the child's
  exit ends the session.
- **stderr** is filtered line by line, redacted, and written to the agent's log
  (`0600`, capped and rotated, never bodies).

## Security

This section holds in both isolation modes. In `dedicated-user` the operating system
also closes the organization's sources to the agent account (ADR-0015 decision 2,
ISO-1); in `same-user` the vendor sandbox and these denies are the only barrier, and
the product says so.

- **The workspace is hostile.** Nothing in a worktree may change a session's settings,
  permissions, hooks, tools or MCP servers. Settings come only from the office, and
  composed instructions from the integration branch in a verified cache. Workspace
  skills never load under `--setting-sources ""`.
- **Fixed denies,** for every runtime, through its permission policy and its sandbox,
  overriding every allow rule:
  - writes to `.claude/**`, `.agents/**`, `.codex/**` and `.gemini/**` in the workspace;
  - writes to any `.git` entry inside the workspace (so a home or workspace cannot be
    a repository root, and an agent cannot set up submodules);
  - the repository cache paths the [Git design](projects-git-github.md#sandbox-and-deny-rules-for-the-selected-paths)
    lists;
  - every path in the managed-state table below;
  - writes to programs that run outside any sandbox: `~/.local/bin/**`, every
    user-writable folder on the captured `PATH` and the folders its symlinks resolve
    into (the whole Homebrew prefix when it is user-writable), each runtime's real
    path, and the release `bin/`;
  - reads of the service user's Keychain files (`~/Library/Keychains/**`) and its own
    vendor folders (`~/.claude/**`, `~/.claude.json`, `~/.codex/**`, `~/.gemini/**`),
    and writes to its configuration (`~/.gitconfig`, `~/.config/git/**`,
    `~/.config/gh/**`, `~/.ssh/**`, shell start-up files, `~/Library/LaunchAgents/**`);
  - `/tmp/cc-socks/**` and `/private/tmp/cc-socks/**`: reads, writes and connections.
  - A runtime whose sandbox cannot deny reads is not office-run.
- **No deny is an ancestor of a writable root.** The office checks this when it builds
  the lists and refuses the start otherwise.
- **Secrets stay out of argv, settings files, logs and the event log.** Before storing
  or streaming any event, and before writing stderr, the office redacts the live session
  tokens, the values of runtime secret variables it knows by name (each adapter lists
  them, such as `CLAUDE_CODE_MESSAGING_TOKEN`), and token patterns (GitHub, Anthropic,
  OpenAI, Google). A bare printed value of a variable the office never saw is not
  caught.
- **Known limits.** A symlink swapped between the office's answer and the unsandboxed
  Claude CLI's write lands where it points unless a Claude deny rule matches it. In
  `same-user`, and between agents in `dedicated-user`, a hostile process can read
  another process's environment; the session token and ancestry stop casual and
  accidental impersonation, not that.

### Managed state

"Denied" covers reads, searches and writes, from shell and file tools. Paths are
resolved from the instance registry and the selected release. No deny covers the
whole instance or a parent of a granted root.

| Resolved path | Agent's tools |
| --- | --- |
| `durable/` of the organization, except the agent's own private home | Denied |
| The agent's own private home | Read and write in a home session; read-only in a task session. The vendor sign-in inside it is the run's copy (ADR-0015 decision 11) |
| Other agents' private homes and workspaces | Denied; other tasks' worktrees of the same agent are read-only |
| `runtime/<office-id>/office.db` and its side files | Denied |
| Repository caches under `runtime/<office-id>/repositories/` | Only the commit paths the Git design grants |
| Instance configuration, locks, setup journal, credential files, host database, logs and backups | Denied |
| The instance registry and its lock | Denied; `agent-os` receives its endpoint from the launch |
| Other offices' and other instances' roots, runtime directories, credentials and temp folders | Denied, by canonical path |
| Under the instance's temp folder: other offices' folders, and other employees' folders in this office | Denied |
| The web process's logs and temp directories | Denied |
| The instance's private runtime directory | Denied; only a connection to the exact `AGENT_OS_SOCKET` is granted |
| The pinned release directory, `agent-os` and runtime executables | Read and execute; fixed write deny |

Before a newly provisioned private home, office or instance becomes accessible, the
affected running sessions are refreshed or relaunched at a safe point. Grants are
rechecked at every launch.

## Contract

The local API version is 1 and changes in place until v1 ships. A person's calls
arrive through the web process with the person's principal and a host grant, and the
office checks each against the authorization table; an agent's connection is refused
unless a row allows the call. Execution itself uses the internal `RunTurn`,
`ResumeTurn`, `CancelTurn` and `StopAgent` operations of document 11, with operation
receipts, expected revisions and immutable input references.

- **Agents:**
  - `agent.wake { employee_id, session?, prompt }` → `{ turn_id }`: an explicit
    instruction. `session` is `"home"` or `{ "task_id": "…" }`, default `home`.
  - `agent.stop { employee_id }`: Pause, with the stop acknowledged separately.
  - `agent.interrupt { employee_id }`: ends the current turn.
  - `permission_policy.get { employee_id }`, `permission_policy.set { employee_id, policy, expected_version }`.
- **Take-over** (local `agentic-org` only): `session.take_over { employee_id, session? }`
  returns the launch description; `session.hand_back { employee_id }`.
- **Messaging:** `msg.bind` takes `session_token`, required for an office-run agent.
- **Runtimes:** `runtime.status { runtime? }` → per runtime: installed, path, version,
  in tested range, signed in, quota windows.
- **Runs:** `run.list { employee_id?, since?, cursor?, limit? }`, `run.get { run_id }`,
  `run.events { run_id, from_seq }`, then live through `subscribe`.
- **Approvals:** `approval.list`;
  `approval.answer { approval_id, decision: allow_once | allow_always | deny, scope?, note? }`.
- **Events:** `agent.state_changed`, `run.started`, `run.finished`, `run.event`,
  `approval.requested` (with structured input, the justification labelled untrusted,
  and `escalation`), `approval.answered`, `employee.presence_changed`. These go only to
  people's subscriptions; a `subscribe` from an agent's connection never receives them.
- **Errors:** `unavailable` with a reason (`not_installed`, `signed_out`, `version`,
  `quota`, `paused`, `suspended`); `invalid_request` with `offboarded`, `escalated`,
  `escalating_tool` or `managed_state`; `held` while an agent is taken over;
  `conflict` for a stale expected version. `ApiError` carries an optional `reason`. New
  enums decode unknown values as `Unknown`.

## Checks before a runtime is office-run

Each item fails closed: until it holds, that runtime's agents are not run by the
office.

- Tools see `AGENT_OS_SESSION_TOKEN` and `agent-os` binds from a tool shell (Codex with its
  `shell_environment_policy` pins).
- The runtime signs in from the private home with the connection the pool selected, and
  a file search from its tools finds nothing of the service user's vendor folders
  (ISO-8); on macOS a tool cannot read a vendor sign-in from the Keychain.
- Claude: a nested `CLAUDE.md` in a subdirectory of the worktree is not loaded, or is
  shown to steer only that task's sessions.
- Claude: under the isolation switches, no user instruction file, skill, agent,
  command, synced plugin or user MCP server loads; `init.tools` holds only the allowlist
  under `--tools` and `--disallowedTools`; `init.mcp_servers` is empty while no
  connector is granted; no work continues after a `result`; remote control is off.
- Codex: the permissions profile holds every fixed deny in live turns; commits work in
  a worktree under it; managed requirements are tried to lock `danger-full-access` out
  of the interactive tool.
- Antigravity: its sandbox holds the read and write denies, and its agent file and
  `.agents/` content cannot be written by the agent or are not loaded.
- Read denies hold for Bash and the file tools, for searches rooted at an allowed
  parent, and through symlinks planted in the workspace.
- Sandbox deny lists accept the wildcards used, or the office expands them to literal
  paths at start.
- A task commit works at its real path with the full deny table and the Git
  environment; deny targets deleted and re-created during a session stay denied (on
  Linux, mount-based denies).
- An approval left unanswered for 31 minutes gets no answer from the vendor tool, or
  the office's expiry is set below the tool's.
- The office's mark scan finds the runtime's tool processes, including those that left
  the group; a process whose environment cannot be read is never killed by the scan.

## Security tests

Tests use a fake runtime that speaks each recorded protocol; the one smoke test per
real runtime is opt-in.

- A hostile workspace (`.claude/settings.local.json` allowing everything, skills with
  `allowed-tools`, hooks, rules files, `.agents/hooks.json`, `.codex` configuration,
  `.git/hooks/*`) has no effect, and writes to those paths are denied.
- A symlinked dotfile whose target is inside a writable root is still denied. A
  writable root whose last component is a symlink refuses the start.
- Every managed-state path is refused as the table says, from Bash and the file tools,
  even with an `allow_always` rule; searches return nothing from a denied path; writes
  through planted symlinks are refused or go to a person; a rule reaching managed state
  is refused with `managed_state`.
- Claude's `init.tools` holds only the allowlist; a fake runtime whose `init` lists a
  disallowed tool fails the start, and on a later `init` the process is gone within
  the fast stop's 5 s; an unknown tool goes to a person as `unknown`.
- Escalation: `dangerouslyDisableSandbox` is denied as `sandbox_bypass`; a mode switch
  goes to a person; `allow_always` on an escalated approval is refused; the adapter
  never sends `updatedInput` or `updatedPermissions`.
- The service user's configuration, `PATH` folders, runtime paths and the release
  `bin/` cannot be written; `command -v agent-os` resolves to the release `bin/`.
- The child's environment and descriptors hold no database or host key, GitHub token,
  `SSH_AUTH_SOCK` or user Git or `gh` configuration; argv holds no secret.
- `msg.bind` is refused without a valid token, with another run's token, and from a
  process outside the session's tree; connections close when the run's process exits.
- Orphans: stopping a session kills its tools, including tools in their own session,
  and so does cleanup after a SIGKILL; a reused PID or process group without the mark
  is never killed; cleanup never hits the session's next process.
- Offboarding stops the session at once, closes its connections and voids its token.
- SIGKILL the office and restart it: no survivor, one writer, pending approvals
  expired.
- A suspended agent whose office restarts is resumed by the answer (ASK-6).
- A message body starting with `/` reaches Claude as text and runs no command; stdin
  lines with `\n`, U+2028 and U+2029 stay one line; a stdout line over 4 MiB becomes a
  `truncated` event.
- Tool output with a token or a known secret variable is redacted in the event log and
  the stderr log.

## Open questions

- **Vendor terms.** Running agents on a person's own Claude, Codex or Antigravity
  subscription is acceptable for the team's own use. Before Agentic Org is offered to
  anyone else, the vendors' terms must be checked and an API-key path per runtime
  provided.
- **Concurrency.** How many agents an office runs at once by default is not fixed; it
  follows from the permits of document 11 and the machine.

## What the code does today

`crates/org-runtime` implements the synchronous sans-I/O contract: `SessionSpec` with
validated paths and `spec_hash` (`crates/org-runtime/src/spec.rs`), events, framing
and probes. It has no `isolation_mode` yet, and it still has a one-off session
kind, which is not part of v1. `crates/org-supervise` implements piped
spawn, identity ordering, stop and reap and the recovery API. No adapter crate exists;
the vendor protocol fixtures are research evidence. Nothing launches an agent
(implementation plan C1, C3).
