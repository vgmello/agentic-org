# Design: agent sessions (the office runs the agents)

Status: **selected execution integration requirements, rev 5** (2026-11-15).
Documentation only: the complete runner, pool routing and production composition
are not implemented by this design. [Execution](organization-data/11-model-profiles-and-agent-execution.md)
owns the selected turn/process contract; [employees](organization-data/04-agent-structure.md),
[connections/pools](organization-data/14-model-providers-and-connection-pools.md),
[recovery](organization-data/16-recovery-and-office-continuity.md) and
[budgets](organization-data/17-budgets-and-controls.md) own their respective data.
Use the [authority map](documentation-authority.md) for precedence.

This document retains the runtime research, adapter restrictions and security
validation from revisions 3.2–3.7. Exact `work.git`, instance-home, API v1 and native
CLI examples are labeled compatibility evidence where retained. They are not a
second target layout or a claim of implemented launches. The earlier complete
proposal is available in [the pre-reconciliation revision](https://github.com/vgmello/agentic-org/blob/c6e520a36c4dc7c738c68915fac5e7d5d1cbe804/docs/design/agent-sessions.md).
Research fixtures are in [agent runtimes control](../research/agent-runtimes-control.md).

Current executable/source names are agentic-org, agentic-org-office,
agentic-org-web and agent-os; org-host is a library. See
[executable migration](executable-migration.md) for available entry points.
The browser does not own process lifetime. Human controls use authenticated HQ
operations and the placed office. There is no terminal in the browser
([ADR-0004](../adr/0004-headless-agent-sessions.md)): the browser shows a session
read-only and people interact with agents through chat. Interactive takeover is an
authorized local CLI operation.
The macOS Keychain/sandbox evidence below does not establish Linux adapter readiness.

### Managed home contract (standalone v1)

The following is the earlier standalone v2 managed-home/retired-field proposal,
not the selected task tree or a claim that transport v2 has shipped. The selected
launcher still rejects arbitrary caller-set paths and resolves approved workspace
keys; any legacy home metadata is provenance only. Preserve versioned callers and
security checks while the target mapping is implemented.

Resolve every home launch from the configured instance root and stable office/member
IDs. Preflight validates the actual resolved path and filesystem identity against
the fixed denies; stale profile metadata, symlinks and caller input cannot redirect
it. (amended 2026-11-10: X-02 D7) The managed home leaves `AgentProfile`: the owning
office reports this effective path as status, with its office and freshness (X-02 §8,
`who`'s `managed_home`; an approved draft not yet in this repo, see the launch v3 note
in office-host-boundary.md §3), so PROF-4 displays the same home the launcher uses.

Retire `member.update_agent`'s caller-settable `home` and `clear_home`, and refuse
home overrides in all create/update inputs before filesystem access. The first
standalone local protocol is API_VERSION 2 under ADR-0001; remove/update affected
callers and fixtures together. Historical or imported profile paths are provenance
only, never launch authority. Test forged absolute paths, traversal, `clear_home`,
stale placement, and symlink redirection without creating an external directory.

## Decisions
1. **Headless-first.** The office drives every agent as a child process over the runtime's structured JSON-lines protocol:
   - **Claude Code:** `claude -p --input-format stream-json --output-format stream-json --verbose`, one long-lived process per session.
   - **Codex:** `codex app-server` over **stdio**, one process per session, with one thread in it. There's no socket. A shared app-server waits until a spike proves that its threads are isolated from each other and from agents.
   - **Antigravity:** `agy acp` (the Agent Client Protocol). `agy` stream-json is the fallback only if the spike shows ACP isn't ready, and then approvals follow decision 7's fallback.

   **This replaces the `--bg` + `claude attach` choice** of the unmerged ADR-0004 draft (`0004-session-panel-embedded-terminal`), which spike S2 confirmed. `--bg` has no programmatic prompt channel, no structured events and no usage stream, and the office needs all three. A person can still work with an agent directly (see "Taking over"). [ADR-0004](../adr/0004-headless-agent-sessions.md) now records this decision.
2. **One port, one adapter per runtime, one supervisor.**
   - `org-runtime` is the **port**: `AgentRuntime`, an optional `RuntimeHost`, `Session`, `SessionSpec`, `LaunchContext`, `LaunchPlan`, the normalized `SessionEvent`, `Capabilities`, usage types, and the one line framer and encoder that every adapter and the pump share. It's a contract, like `org-proto`. It does no I/O.
     - `AgentRuntime` is a stateless factory per runtime: its capabilities, the launch plan and the `Session` for a session (from a `SessionSpec` and a `LaunchContext`), and the launch plans for its **probes** (preflight, and the usage poll where a runtime needs one) with the parsing of their output.
     - `RuntimeHost` is a shared host process that serves several sessions. It's optional and deferred: River's first version leaves it out, and it's added, additively, when a runtime shares a process. Nothing uses it while Codex runs one app-server per session.
     - `Session` is **sans-I/O**: a pure state machine for one runtime's protocol. The pump tells it what happened (`start`, `on_line`, `on_truncated`, `on_stderr_line`, `on_stdout_closed`, `on_exit`, `on_tick(now)`, and `control` for an office command), and it returns outputs in order: `Write` (one line for stdin), `Event` (for the office), `CloseStdin` or `Abort` (stop the process now: the pump runs the fast stop under "Processes and safety"; rev 3.3, for a failed `init` check of tools, plugins or MCP servers). It never spawns, reads, writes, sleeps or reads the clock (time is an input), so it's tested from recorded fixtures alone. Its contract is under "The model".
   - The port is **synchronous**, with no async runtime.
   - Planned adapters `org-runtime-claude`, `org-runtime-codex` and `org-runtime-agy` will implement it; none of those crates exists in this checkout. Runtime research/spike fixtures are separate evidence, not implemented adapters. An adapter **builds a launch plan** (argv, env additions, files to write) and a `Session` for its protocol. It never spawns and never touches a pipe.
   - `org-supervise` is the **one place that spawns agent runtimes** and their probes. Piped children are implemented; PTYs for takeover remain separate work. It owns process control only: process groups, the env allowlist, the stop sequence, reaping and orphan cleanup. It speaks no runtime protocol. It's also the single spawn path for every other process the host or a server starts (`org-git`'s git, the host's office servers), as `office-host-boundary.md` §4 says (as amended on `ada/docs-followups-3`); only the agent-runtime parts (the env allowlist, marks, the stop plan) are specific to agents.
   - **The lock** on an agent is released only after verified owned-process/descendant cleanup and reaping; a process exit or thread unload alone cannot establish that proof.
   - Adapters depend only on `org-runtime`: never on each other, on `org-supervise`, on `org-office` or on storage.
   - **`org-office` depends on neither `org-runtime` nor `org-supervise`.** It holds admitted execution rules as domain logic (turn state, limits and history; automatic queues remain deferred) and defines the small trait it needs to act on sessions, in its own terms (`SessionControl`: start, send, interrupt and stop a session by id, and **probe** a runtime for preflight or usage). The planned integration library **`org-office-sessions`** implements it with `org-supervise` and the adapters, maps the office's session record to a `SessionSpec` and a `LaunchContext`, and runs the **pump**, one implementation for every runtime. For each live session it owns the child's pipes, splits stdout and stderr with the port's framer, feeds each line to the `Session`, applies its outputs in order, calls `on_tick` at the session's next deadline, and feeds the facts (turn ended, approval requested, usage, exit) into `org-office`. It also implements `SessionAuth` (see "Messaging"). `agentic-org-office`, the composition root, wires it in. See "Where the code lives".
   - The volatile third-party CLIs sit behind a stable port, so a runtime's protocol change touches one adapter.
3. **Durable facts, disposable projections.** Organization Git owns employee and
   execution definitions. Protected execution journals own turn/attempt/process
   transitions, immutable inputs, tool receipts, checkpoints and command outcomes;
   HQ's protected journal owns placement generations, permits and cooldowns.
   SQLite indexes these sources. A bounded event log or runtime transcript alone
   cannot establish whether an effect completed. Transcript plaintext behavior in
   the earlier native CLI spike is a platform/privacy limitation to disclose and
   validate, not permission to put prompts into ordinary logs or organization Git.
4. **Identity, placement and workspaces are separate.** Every employee has one
   required home team and optional role/reporting line in `profile.md`; only an
   agent can have `execution.md`, selecting one pool and canonical model. A person
   has no execution process. The owner security principal is not an execution
   employee; representing the owner/HR as an employee still requires a home team.
   Placement may change independently of identity, subject to fencing.

   The selected runtime paths are:

   ```text
   runtime/<office-id>/
   ├── repositories/<repository-id>.git/        # one office cache per registered repo
   └── teams/<root-team-id>/teams/<child-team-id>/.../agents/<employee-id>/
       ├── projects/<project-id>/tasks/<task-id>/repositories/<repository-id>/
       └── tasks/<task-id>/repositories/<repository-id>/  # projectless team task
   ```

   A project can use several registered repositories; each association requires
   its own integration branch. Workspace records bind office, employee, task and
   repository; a session's cwd is an approved workspace reference, never arbitrary
   caller input. A non-task turn uses an explicitly approved workspace key; this
   does not select automatic home sessions or a memory engine.
   One open logical turn and one not-gone main process slot exist per employee.
   No native delegate/helper exception is selected. Handoff, process exit and turn
   completion are not task closure: preserve task worktrees and unpushed work.
   A missing workspace is a recovery issue; restore only from verified repository
   and retained task state, disclosing any unpushed loss rather than assuming a
   pushed branch contains everything.
5. **The admitted launch restrictions are replayed on every start and resume.** The adapter-specific mappings below are compatibility requirements; document 11 freezes input, pool/model, workspace and policy per logical turn. A changed next-turn launch digest requires controlled cleanup/relaunch rather than changing a live child. Claude's `--resume` doesn't restore `--settings`, `--mcp-config`, `--plugin-dir` or `--add-dir`, a `-p` resume starts in Manual permission mode, Codex filters threads by cwd, and Antigravity's conversations are tied to the cwd. So the office stores the whole spec and always replays it, in the session's own workspace.
   - The spec includes the permission mode and the sandbox mode. Claude always gets `--permission-mode`. Codex gets `approvalPolicy`, `approvalsReviewer: "user"` and `permissions` (the session's named profile) on every `thread/start`, `thread/resume` and `turn/start`, and never the legacy `sandbox` or `sandboxPolicy`, which would override the profile (per S0 Codex, live turns still to verify).
   - `spec_hash` covers the whole spec. The conversation to resume isn't part of the spec (it's in the `LaunchContext`), so a resume hashes the same as the first start. Claude keeps the system prompt it recorded at the first request (`--system-prompt-snapshot`), so when a resume's `spec_hash` differs and the stored instructions differ from the new ones (the hash also changes with the policy's version), the office sends the change as a message ("Your instructions changed: …").
   - **The Claude spike confirmed it:** `--resume` works from any cwd, even after the original cwd was deleted. The system prompt (the agent's prompt and the appended file) is snapshotted, and `--settings` isn't restored, so a resume without it asks for approvals the settings allowed.
   - **Bypass modes are banned everywhere**, headless and interactive:
     - Claude: `bypassPermissions` and `--dangerously-skip-permissions`. The office's settings set `disableBypassPermissionsMode: "disable"`, so the take-over TUI can't switch to it either.
     - Codex: `danger-full-access`, `--dangerously-bypass-approvals-and-sandbox` (and its `--yolo` alias in releases that have it), and `--dangerously-bypass-hook-trust`. The take-over TUI gets the same `-c` pins as the app-server, but a person can still change them from inside it (a known limit under "Security", unless S0 can lock them).
     - Antigravity: `--dangerously-skip-permissions`.
6. **Isolation by default.** The exact instance/temp/`work.git`/native-auth
   mappings below are the earlier adapter restrictions (revisions 3.3–3.7), retained
   as compatibility/security evidence. They do not select jobs, memory or user-global
   authentication for the target. Re-derive target paths and private connection
   bindings before launch, without relaxing any deny or fail-closed fallback.
   - **Environment allowlist.** The child gets only:
     - `PATH`: the instance's pinned release helper folder, `<release dir>/bin/`, first, then the `PATH` captured once from the user's login shell. This folder holds the verified release's `agent-os` helper, so `agent-os` never resolves to a user-writable file. Agents can read it but never write it (a fixed deny). Relative entries, and entries inside the workspace or the agent's home, are dropped from the captured `PATH`.
       - **Codex sessions** also get the folder of the real git that `xcrun --find git` gives, before `/usr/bin`: the `/usr/bin/git` launcher stalled inside Codex's sandbox, and the resolved binary committed (per S0 Codex). The office resolves it at preflight. Like every `PATH` folder, it's a fixed write deny if the user can write it.
     - `HOME` (the runtimes need it), `LANG`, `USER`, `LOGNAME`, `SHELL`, and `TERM` for the take-over PTY
     - `TMPDIR`: the session's own directory, `<user temp>/ao/<instance>/<office>/<member>/<session>/`, each component a short allocated **temp handle** and each level 0700, a writable root. It's removed when the session ends. (Rev 3.7.)
       - **`<user temp>`**: on macOS, `DARWIN_USER_TEMP_DIR`, placed in the environment in its `/var/folders/…` form (grants and denies match canonical paths). On Linux, one disk-backed private directory per OS user that setup validates (owned, 0700, no symlink, within the budget below) and records with the instance registry, shared by every instance of the user, for example under `${XDG_CACHE_HOME:-~/.cache}`; `$XDG_RUNTIME_DIR` (a small RAM-backed tmpfs) only as a fallback, with its free space reported by `doctor`; never a shared `/tmp`. The private runtime directory and `<user temp>/ao/` are disjoint trees.
       - **Handles are allocated, never derived** from client-made ids. Each is random lowercase base32, checked against its owner's records and claimed by creating its directory exclusively (`mkdir`; on `EEXIST` or a recorded clash, draw again). Owners: the instance handle (3 characters) by setup, checked against the instance registry, which records each instance's canonical temp folder; the office temp handle (3) by the host, kept in its registry database and passed to the worker on the launch first line (X-02 §12 Q9; a draft, see the launch v3 note in office-host-boundary.md §3); the member handle (5) by the owning office when the employee is hired, kept in `office.db`; and the session handle (3) per session. At hire the office also allocates the employee's **jobs folder** (5, kept in `office.db`), a sibling of its member folder under the office, where each internal job (checkpoint helper, delegate) runs in `<jobs>/<job handle (3)>/`; a job denies the sibling job folders that exist at its launch (a sibling started later isn't denied to it: an accepted residual within one employee), and a checkpoint helper's snapshot is handed to it as its initial input (stdin): the office never writes it to a temp folder. Each office also has a **runs folder** (5, kept in `office.db`) for one-off runs, which have no member: a one-off's `TMPDIR` is `<runs>/<run handle (3)>/`, and its no-project workspace is the `ws/` folder inside it. A one-off denies the sibling run folders that exist at its launch; a one-off started later isn't denied to it, an accepted limit within one office's runs (see "Workspace" under one-offs). Instance, office, member, jobs and runs handles are never reused while recorded; session, job and run handles are released when their folder is removed.
       - **Until launch v3 carries the office temp handle**, the worker never derives or allocates it, so: no office-run session (one-offs and jobs included) launches; the start fails closed (an employee's session or job: the agent shows Needs you; a one-off: the run ends with that outcome), and there's no fallback to another temp directory. The office still allocates member, jobs and runs handles at hire and creation, checked against `office.db` only (they're unique per office folder, which only this office allocates under); it creates no folder under `<user temp>/ao/` and skips the office-start re-creation and sweep. Once it starts with the handle, it claims and creates them all before any launch. So that offices created before then aren't left failing closed, the host, at startup and before it starts any worker, allocates and claims an office temp handle for every catalog office that lacks one (X-02 §12 Q9; see the launch v3 note in office-host-boundary.md §3).
       - **The folders exist before any session can launch.** Each host re-creates, at startup and before it starts a worker, the temp folder of **every registered instance** (from the registry, not only its own) and every catalog office's folder of its own instance; each office re-creates its runs folder and its roster's member and jobs folders at office start and again before each session launch, and checks that every registered instance's temp folder exists (re-creating it if not) before each launch. Other instances' folders are denied by their registry path in any case. So a reboot or a temp-directory cleanup (`$XDG_RUNTIME_DIR` is emptied at reboot; macOS prunes its temp folder), or hosts starting in any order, never leaves a deny target missing.
       - **Every level** the office or host creates or opens is checked: owned by the user, mode 0700, not a symlink (opened with `O_NOFOLLOW`), walking down from the canonical `<user temp>` (on macOS `/var` is itself a symlink to `/private/var`).
       - **Length budget:** the `TMPDIR` string in the environment, with no trailing `/`, is at most 71 bytes, so `TMPDIR` plus a 32-byte suffix (including its leading `/`) fits the 103 usable bytes of `sun_path` (Python `multiprocessing` needs 32). On a reference Mac it's 69. A job's or one-off's `TMPDIR` has the same depth and length. A start whose `TMPDIR` would exceed the budget is refused with a clear error (an employee's session or job: the agent goes to Needs you; a one-off: the run ends with that outcome); it never falls back to another temp directory.
       - It's outside `<office home>`, so it's never next to a path the fixed denies cover. Orphaned session, job and one-off run folders (a crash) are swept when the office starts.
     - `ORG_AGENT=<agent>`, `ORG_OFFICE=<office id>`, `ORG_OFFICE_HOME`, and `ORG_OFFICE_SOCKET=<resolved private socket>` from the trusted launch contract
     - `ORG_SESSION_TOKEN`: a random token **per run**, held only in office memory, required on `msg.bind` (see "Messaging"), never in argv or a settings file, never logged. A run is one process's life (see "The model"), so every spawn (start, resume or take-over) gets a fresh token, and the token dies when the process exits.
     - `ORG_SESSION_MARK`: a random value **per session**, recorded before the spawn, used to find orphans (see "Processes and safety"). It isn't a secret.
     - git: `GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL=/dev/null`, and `GIT_CONFIG_COUNT`/`GIT_CONFIG_KEY_n`/`GIT_CONFIG_VALUE_n` setting the commit identity (`user.name`, `user.email`, from `projects-git-github.md`), `credential.helper` to empty, `core.hooksPath=/dev/null`, `core.fsmonitor=false`, `gc.auto=0` and `maintenance.auto=false` (see "Git in worktrees")
     - `GH_CONFIG_DIR=<TMPDIR>/gh/`, an empty folder the office makes for the session, so `gh` sees no login of the user's. Agents get `gh` only through `agent-os gh-token` (`projects-git-github.md`).
     - **None of these is a file in the runtime dir** (rev 3.3). Tools can't read any agent's runtime dir, their own included (a fixed deny; see "Security"), so the git settings travel in the environment, which override every config file, and nothing points at a file an agent can write, apart from its own session's `gh` folder, which holds nothing of the user's and no other employee's session sees (sessions of the same employee share its member temp folder, as they share its home).
     - office-set runtime switches such as `DISABLE_AUTOUPDATER=1`

     Everything else is dropped, including `SSH_AUTH_SOCK`, `CLAUDE_CODE_*`, `AI_AGENT`, `GH_TOKEN`, other `GIT_*` variables and any API key. There's no git over ssh: agents never push themselves (`agent-os push`), so they don't need an ssh agent.

     **Runtimes add their own variables to their tools.** Claude puts `CLAUDECODE`, `AI_AGENT`, `CLAUDE_PID`, several `CLAUDE_CODE_*` variables and `GIT_EDITOR` into every tool's environment, after the allowlist. One of them, `CLAUDE_CODE_MESSAGING_TOKEN`, is a secret, so it's redacted (see "Security").

     **The runtimes must pass this environment to their tools.** Claude does: the allowlisted variables reached the tool's `env` in the spike. Codex's `shell_environment_policy` drops variables whose names match `*KEY*`, `*SECRET*` or `*TOKEN*` from tool shells by default, which would drop `ORG_SESSION_TOKEN`, so every office-run Codex agent would fail `msg.bind`. The office pins `shell_environment_policy.ignore_default_excludes=true` and `shell_environment_policy.include_only=[` the names above, every indexed `GIT_CONFIG_KEY_n`/`GIT_CONFIG_VALUE_n` name included `]` with `-c`, on the app-server and on take-over (`codex resume`). S0 Codex saw the token survive these pins in a standalone `command/exec`; a model's tool shell is still to verify. S0 proves that `agent-os` binds from a tool in each runtime (see "S0 checks and fallbacks").
   - **Runtime binaries** are run by their recorded **real paths** (symlinks resolved at preflight), never looked up on `PATH`. The office records the real path with the version at every start. Writes to those files, and to every user-writable folder they sit in, are fixed denies (see "Security").
   - **Superseded for sign-in and vendor folders (owner decision 2026-11-17,
     [ADR-0015](../adr/0015-agent-isolation-modes.md) decisions 10 to 12).** Each agent gets a
     private home that the office controls. The vendor folders (`~/.claude`, the Codex home,
     `~/.gemini`) are redirected into it and hold only what belongs to that agent; the sign-in
     comes from the connection the pool selected. The user's own folders are not used. The
     per-runtime bullets below record the earlier approach and its spike results.
   - **No personal config reaches agents.** HOME stays, but the user's git and `gh` config don't reach agents (above). For the runtimes:
     - Claude: the user's **real `~/.claude`**, the supported path, so the user's sign-in works. Isolation comes from three switches. The spike verified the first two with an isolated config dir only; S1 repeats every check with the real `~/.claude` (the user's `CLAUDE.md`, skills, agents, synced plugins, and the MCP servers in `~/.claude.json`), and Claude isn't office-run until they hold (see "S0 checks and fallbacks"):
       - `--setting-sources ""` (the empty string; `none` is rejected). It loads no settings file and no instruction file (a nested `CLAUDE.md` loaded lazily is untested, decision 8): no `CLAUDE.md`, `AGENTS.md` or rules from the cwd, its parents, an `--add-dir` folder or `~/.claude`. It loads no user or project skills, agents or commands either. Claude's built-in plugins still load. **The account's synced claude.ai plugins** load whenever the `user` source is on; that `""` keeps them out was seen only by hand, in an isolated config dir (the spike's `[hand]` note), so it's **S1-unverified**.
       - `autoMemoryEnabled: false` in the office's `--settings <file>` (0600), the only settings Claude gets. Without it, auto-memory (`MEMORY.md`) still loads under `--setting-sources ""`, and that's an agent-writable instruction channel (decision 8).
       - `--strict-mcp-config` with the office's `--mcp-config <file>`, so the user and local MCP servers in `~/.claude.json` and the claude.ai connectors don't load. The spike's MCP item showed it with controls: `--setting-sources ""` alone kept a config-file server out but not the account's connectors, and `--strict-mcp-config` kept out both. But it ran in an isolated config dir, so the real `~/.claude.json` was never in play: **to verify in S1 with the real `~/.claude`**.
       - **The adapter checks every `init`:** every `init.plugins` entry is a built-in (`@builtin`), and `init.mcp_servers` lists only the office's servers. Otherwise the start fails (on the first `init`) or the session is stopped at once (on a later one: the same fast stop as for the tools check, below), and the agent goes to Needs you. So a synced plugin or a user MCP server that S1 missed never gets a tool call allowed. That limits the damage but doesn't replace S1: a plugin's start-up hooks may already have run (unsandboxed) by the first `init`.

       The CLI itself isn't sandboxed: it reads and writes `~/.claude` (transcripts, settings; the sign-in is in the Keychain). The agent's tools get a fixed read deny on `~/.claude/**`, so an agent can't read other agents' transcripts or the user's (see "Security", fixed denies). Claude's sandbox holds Bash to it; the file tools (Read, Grep, Glob, Edit) run in the CLI, so the permission policy's deny rule holds them. S1 verifies both. The office doesn't use `CLAUDE_SECURESTORAGE_CONFIG_DIR`: it's undocumented. It also doesn't use `--safe-mode` (every settings file's `env` still applies, so it isn't isolation) or `--bare` (it never reads the user's sign-in and needs an API key; not tested). A **per-agent `CLAUDE_CONFIG_DIR`** is later hardening, once `CLAUDE_SECURESTORAGE_CONFIG_DIR` is documented, or with an API key or a `claude setup-token` per organization (U-S1).

       **The shell snapshot.** Claude sources a snapshot, `~/.claude/shell-snapshots/snapshot-*.sh`, before every Bash command. It's built from the user's `$SHELL` with the real `HOME`, so the user's rc aliases and functions could reach agents (not observed; the spike's snapshot held none). S1 checks two things: that Bash still works under the read deny on `~/.claude/**` (fallback: allow reading `~/.claude/shell-snapshots/` only), and whether `SHELL=/bin/sh` or a clean `ZDOTDIR` keeps the user's rc files out. Until then it's an open risk.
     - Codex: the user's normal Codex home, so the user's sign-in works (no `CODEX_HOME` override). Isolation comes from `-c` overrides on the app-server and the thread's config: hooks off, the user's MCP servers off (`-c` / `--disable`), `approvalsReviewer: "user"`, the approval policy and the session's permissions profile pinned. (`--ignore-user-config` and `--ignore-rules` exist only on `codex exec`, so they don't apply.) The Codex spike lists what still leaks from `~/.codex` (e.g. `AGENTS.md`, other `config.toml` keys).
     - Antigravity: `agy acp --gemini-dir <runtime dir>/gemini` (each agent's own, under its runtime dir) if sign-in still works there. The spike also tries `--include-default-customizations=false` and `--app-data-dir`. **Known limits:** if neither works, the user's `~/.gemini` rules, hooks, MCP servers and plugins load into every Antigravity agent. Take-over (`agy --conversation`) has no `--gemini-dir` at all, so it loads them unless the spike finds a way. We document both and tell the owner when they enable an Antigravity agent.
   - **Generated runtime files** (settings, MCP config, instruction files, Antigravity's custom agent file) live **outside the workspace**, in `<office dir>/agents/<agent id>/runtime/` (0700, files 0600). Agents' tools can neither read nor write any agent's runtime dir, their own included (fixed denies under "Security"): only the runtime itself reads them, outside its sandbox and its tool permissions. S0 checks that for each runtime (Antigravity reads its agent file through `--add-dir <runtime dir>`). If a runtime can only read them from the workspace, or can't read them under the deny, the office writes them there itself before every start (so an agent's edits never survive), and git-excludes them through `work.git/info/exclude`, which every worktree shares and agents can't write (a `denyWrite` entry; see "Git in worktrees").
   - **Runtime sandboxes are on by default:** Codex's sandbox through a named permissions profile, Claude's bash sandbox (in the office's settings), Antigravity `--sandbox`.
     - Claude's settings pin `autoAllowBashIfSandboxed: false` and `allowUnsandboxedCommands: false`. With the first one on, sandboxed Bash can run without `can_use_tool`, so the permission policy might not see it. (The spike's runs had it on together with an exact allow rule for the command, so they don't show which of the two skipped the request. The office turns it off either way.)
     - Each sandbox's **writable roots** are set by the office: the workspace, the session's `TMPDIR`, and for a task session the parts of `work.git` that commits need (see "Git in worktrees"). Where a sandbox opens more by itself (Claude's does, in `work.git`), the office adds `denyWrite` entries.
     - Each sandbox **explicitly allows the agent's exact resolved office socket** from `ORG_OFFICE_SOCKET` in the private instance runtime directory, so the matching release's `agent-os` works. It no longer derives a path from `<office home>/run/` or the current directory; endpoint authentication and session binding still apply: Claude `allowUnixSockets`, Codex through its profile (below), Antigravity as its spike finds (S0). Claude's list admits exactly the listed path (even one created after the CLI started) and refuses every other socket, Claude's own messaging socket included.
     - **Codex: a named permissions profile** (per S0 Codex, live turns still to verify). The office defines one profile per session in a **single** `-c` value holding the whole `permissions.<name>` TOML table (the CLI's dotted-key parser splits a `.` inside a quoted socket path), plus `-c features.network_proxy=true`. The profile extends `:workspace` (the verified form) and adds the session's writable roots, the fixed denies (reads and writes), network on with no domain allowed, and `network.unix_sockets` with the exact office socket path set to `allow`. S0 Codex showed that a profile like it admits exactly the listed socket and refuses other sockets and direct TCP, and that a file it marks `deny` can't be read or written. The full git and office deny lists under a profile aren't verified yet. `:workspace` also brings its temporary-folder defaults as writable roots; whether a profile can drop them (not extending it, or overriding them) is unverified, and until then they stay writable (they're outside `<office home>`, and `/tmp/cc-socks` is a fixed deny). Threads select it with `permissions: "<name>"`. The legacy `sandbox`/`sandboxPolicy` is never sent with it (the legacy policy overrides the profile, and the schema forbids combining them). **The profile is required, and it fails closed** (rev 3.3): the legacy `sandboxPolicy.workspaceWrite` has no read deny and no subtractive write deny, so under it Codex tools could read other organizations' offices, every agent's runtime dir, `~/.claude/**` and the Keychain files. So Codex isn't office-run until the profile holds every fixed deny in live turns, and its agents stay run by hand.
   - **Claude's tools: an explicit allowlist** (rev 3.3). Claude's default toolset (28 tools in the spike's `init`) includes tools that run in the CLI, which isn't sandboxed, and that act outside the office: `SendMessage` and `ListAgents` reach other Claude sessions on this Mac (the user's own included, whose sockets share `/tmp/cc-socks`), `RemoteTrigger`, `CronCreate`/`CronDelete`/`CronList` and `ScheduleWakeup` make wakeups or cloud routines on the user's account outside the office's run rules and quota, `Monitor`, `Task`, `TaskStop` and `Workflow` start or steer background and sub-agent work, `EnterWorktree`/`ExitWorktree` run git in `work.git` from the unsandboxed CLI, and `WebFetch`/`WebSearch` reach the network. So:
     - **Delegation amendment (rev 3.5):** the allowlist/disallowed list below remain the fallback. Employee-owned delegate tools may be added only after the registration, permission, budget and cancellation checks in [`employee-supervision.md` §6](employee-supervision.md#6-capability-checks-implementation-slices-and-validation) pass for the installed runtime. This is a narrow capability extension, never general background or cross-session access. Every `init` is checked against the resulting verified list.
     - **The baseline allowlist** is `Read`, `Edit`, `Write`, `Glob`, `Grep`, `Bash` (sandboxed) and the office-provided MCP tools (`mcp__<office server>__<tool>`), and nothing else. It's the spec's required `tools` (see "The model"). The adapter passes its built-in names as `--tools` (in the spike, a non-empty `--tools` list ran only with `--restricted`, and gave `init.tools` exactly the listed tools, where `--restricted` alone left 24; `--tools ""` alone removed every built-in tool; a non-empty list alone is for S1 to confirm; `--tools ""` didn't hide MCP tools, and MCP names in `--tools` are untested), and the whole list, MCP names included, as the inline agent's `tools` (which also restricted the main thread's tools in the spike). Claude 2.1.311's default set has no separate `Glob` or `Grep`; they appeared in `init.tools` when listed (and under `--restricted` alone), so listing them is harmless.
     - **Tool search is off.** `ToolSearch` is disallowed, so the office's MCP tools must never be deferred behind it: an office-set switch turns tool search off (`ENABLE_TOOL_SEARCH=false`). S1 checks that the office's MCP tools appear in `init.tools` and can be called. Fallback: a Claude agent that needs an office MCP tool isn't office-run until they can.
     - **The disallowed list:** `--disallowedTools` names every other tool in the spike's recorded default set: the cross-session and messaging tools (`SendMessage`, `ListAgents`, `PushNotification`), triggers and scheduling (`RemoteTrigger`, `CronCreate`, `CronDelete`, `CronList`, `ScheduleWakeup`), remote and sync tools (`DesignSync`, `Workflow`, `ReportFindings`), web access (`WebFetch`, `WebSearch`), agent spawning and background work (`Task`, `TaskStop`, `Monitor`), worktrees and plan mode (`EnterWorktree`, `ExitWorktree`, `EnterPlanMode`, `ExitPlanMode`), and the rest (`AskUserQuestion`, `NotebookEdit`, `Skill`, `ToolSearch`). `--disallowedTools` wasn't tested in the spike, so it's a second layer, and S1 checks it.
     - **The adapter checks every `init`** (Claude re-sends it on every turn): `init.tools` must be a subset of the allowlist. Otherwise the start fails (first `init`) or the session is stopped (a later one), and the agent goes to Needs you. `init` arrives only after the first user line, about 40 ms after it in the spike and before the model's first reply. On a failed check the stop is **fast**, not the graceful stop sequence: the adapter at once sends an in-band `interrupt` and answers every `can_use_tool` from then on with a deny, and the `Session` returns `Abort`, so the pump runs the **fast stop** (no checkpoint turn, no wait on stdin; see "Processes and safety"). S1 confirms the order of `init` and the first reply. So a Claude release that adds a tool, or ignores a flag, never gets a tool call allowed with it.
     - **Remote control stays off** (rev 3.4): Claude's `initialize` reports `remote_control_available: true` in every run. The adapter never sends a request that turns remote control on, and the start fails if the session reports remote control as active. S0 finds the setting or environment variable that turns it off, and the office sets it.
     - A `can_use_tool` for a disallowed tool is always denied. One for a tool the adapter doesn't know is escalation `unknown` (decision 7).
     - **Web access is off by default.** A later per-agent policy may enable `WebFetch` and `WebSearch` with a domain allowlist; that's not in phase 1.
     - Bash's own background mode (`run_in_background`) is turned off with an office-set switch, `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1`, which S1 verifies (see "S0 checks and fallbacks").
     - Codex and Antigravity get the same rule where they have such tools: S0 lists each runtime's built-in tools (Codex's web search, say), and the adapter pins its allowlist the runtime's way. Fallback: a runtime whose risky built-in tools can't be turned off isn't office-run.
   - **Auth stays in each runtime's own store.** The office never puts API keys in an agent's environment. API-key support, when it's added, goes through each runtime's own credential mechanism.
7. **Approvals go to the person, never silently lost.** In headless mode all three runtimes deny or soft-deny actions that need approval. The adapters bridge approval requests into the office:
   - Claude: `--permission-prompt-tool stdio` and `can_use_tool` control requests. The flag is required: without it no request arrives, and the action is denied.
   - Codex: `item/*/requestApproval`.
   - Antigravity: ACP `session/request_permission`.

   The office answers from the agent's **permission policy** (what's allowed without asking), and asks the owner in **Needs you** for anything else. Each runtime's own allow rules and sandbox are set from the same policy as a first line of defence, except allow rules for file-writing tools, which the office applies itself so that every write reaches its check (rev 3.3, below).
   - **Escalating approvals (contract review of `org-runtime`, 2026-11-08):** every approval request carries a required `escalation`: `none`, `sandbox_bypass` (e.g. a Codex command asking to run unsandboxed), `permission_change` (Claude `ExitPlanMode`, an ACP mode switch) or `unknown`. The office ALWAYS DENIES `sandbox_bypass`: the fixed denies win, with no policy or owner override in phase 1. It NEVER answers `permission_change` or `unknown` from policy: they always go to the owner in Needs you with a clear warning (a request for a disallowed tool is denied at once instead; see Claude's mapping below). Adapters must set the field; an unrecognised approval kind is `unknown`.
     - **It's carried end to end** (rev 3.3): the Approval record and `approval.requested` carry `escalation`, so the app can show the warning.
     - **No standing rule for an escalated approval.** `approval.answer` refuses `allow_always` and `scope` on an approval whose escalation isn't `none` (`invalid_request`, reason `escalated`): the owner can only allow it once or deny it. `permission_policy.set` refuses an allow rule for a tool that always escalates (`ExitPlanMode`, `EnterPlanMode`, a mode switch; reason `escalating_tool`). And the office never writes such a tool into a runtime's own allow rules, so a runtime can't allow it by itself, without a request.
     - **Claude's mapping** (every Claude request is a `can_use_tool`, so it's by tool name and input): `Bash` with `input.dangerouslyDisableSandbox: true` is `sandbox_bypass` (`allowUnsandboxedCommands: false` already refuses it; this is the second line); `ExitPlanMode` and `EnterPlanMode` are `permission_change`; a tool on the disallowed list is denied at once, and **that beats the mapping**: the plan-mode tools are disallowed, so a request for one is denied at once and recorded with escalation `permission_change`, never sent to Needs you; any other tool name outside the adapter's known list is `unknown`; the allowlisted tools are `none`.
   - **The office allows or denies the original call only. It never rewrites tool input or permissions.** Claude runs a rewritten input (`updatedInput`) without telling the model, so the port has no way to send one. It also has **no way to send `updatedPermissions`**: a `can_use_tool` carries `permission_suggestions` (add rules to `localSettings`, add directories), and accepting one would make the unsandboxed CLI write `.claude/settings.local.json` into the workspace or widen its directories. The adapter ignores them. `allow_always` is the office's own policy change (below), never the runtime's.
   - **A deny carries a message**, which Claude shows the model, and the turn goes on. Claude's `interrupt: true` hides the message, so the office uses it only when it also means to end the turn.
   - **The office owns expiry.** Claude didn't time out an approval within 150 s in the spike, so the office answers an expired one with a deny. Longer is untested: S1 leaves one unanswered for 31 minutes or more. If the CLI turns out to time out first, the office's expiry is set below the CLI's limit, and a CLI-side timeout is handled like a withdrawal (`cancelled`), so the two never race (see "S0 checks and fallbacks").
   - **The runtime can withdraw a request.** When a turn is interrupted, Claude sends `control_cancel_request` for the pending approval, and the office marks it `cancelled`.
   - **Policy scopes:** a rule is a tool plus one of: a path pattern, a command prefix (for shell tools), or an MCP server.
   - Needs you shows the **structured tool input**. The agent's own justification is shown too, labeled as untrusted.
   - An unanswered approval **expires after 30 minutes** (configurable) and counts as a deny. Waiting doesn't count toward the run's time limit.
   - Approvals are office data in `office.db`. When a session's process ends, or its turn ends, with an approval still pending (a stop, a switch, a time limit, a crash), the office marks it `cancelled` at once. At office startup, pending approvals whose session is dead are marked `expired`. Answering an expired or cancelled one returns `not_found`.
   - `allow_always` adds a rule to the agent's permission policy. It's a **versioned policy change**, like `permission_policy.set`, and shows in the policy's history. For a one-off it adds the rule to **that run's** policy only, never to the role's default (see "One-offs").
   - **The fixed denies (under "Security") override every allow rule**, `allow_always` included.
   - **No absolute allow rule reaches managed state (rev 3.6).** `permission_policy.set` and `approval.answer` refuse any absolute path pattern whose normalized literal prefix could match the selected instance root, any registered peer instance root, the instance registry, credential paths, release payloads or private runtime directories listed under "Standalone-v1 managed state and grants" (`invalid_request`, reason `managed_state`). Expand `~`, reject `..` components, and resolve the longest existing prefix before testing both ancestor and descendant overlap, so a symlink or `/**` cannot bypass the rule. Workspace-relative rules (`./…`) may cover only the session's own workspace; their real-path targets are checked on every request, fixed denies still win, and they never become native file-write allow rules. The adapter emits the runtime's explicit absolute/relative syntax. No rule grants a whole instance or shared employee-home parent.
   - **No write under managed state outside the writable roots, even when allowed once.** The file tools run in the unsandboxed CLI, so the office checks every write request itself (a Claude `can_use_tool` for `Edit` or `Write`, a Codex or ACP file-change approval): if the path, resolved to its real path, is under managed state and outside the session's writable roots, the office denies it at once, without asking, and `allow_once` can't override it. So other projects' `work.git`, an office created mid-session and every other unnamed path there stay read-only. This check sees only requests that reach the office, so **every file write must reach it**: the office never writes an allow rule for a file-writing tool (`Edit`, `Write`) into a runtime's own rules, and headless Claude never runs in `acceptEdits` mode (the spec's `permission_mode` for Claude can only be `default` or `manual`; `acceptEdits`, `auto` and the bypass modes fail to decode). So a write through a symlink the agent planted in its worktree is checked at its real path, whatever the policy allows. Policy rules for those tools are applied by the office when it answers, against the request's **real path**: a path rule allows a write only if the real path matches it. A write whose literal path matches but whose real path doesn't (a planted symlink to `~/Documents/…` or another repository, say) goes to the owner in Needs you. This holds for headless Claude, with one gap: the office resolves the real path when it answers, and the unsandboxed CLI writes afterwards, so a detached tool process could swap a path for a symlink in between (a known limit under "Security"). Codex raises no approval for a write inside its writable roots, so for Codex it's the sandbox's own real-path enforcement that holds (an S0 Codex check). The same check covers siblings of `office-state/`, external credential/runtime paths and peer instance roots. As a second line, this office's other projects' `work.git/` and `worktrees/` are also named write denies at start (`start_denies`), and the runtime's own rules are checked as above. S1 checks that `Edit` and `Write` through a planted symlink are refused, either by Claude's own deny rule or by the office at `can_use_tool`, and records which, and that one to a target outside every deny (`~/Documents/…`) reaches `can_use_tool` and goes to Needs you. Fallback: Claude isn't office-run until they are. Inside managed state, only the writable roots and the reads the fixed denies leave open are reachable (see "Security").

   **Fallback:** if a runtime's approval bridge doesn't work, its actions that need approval are **denied**, the run ends as `blocked`, and a Needs-you item says what was denied. Skip-permissions modes are never used.
8. **Instructions are composed by the office** from:
   - the admitted employee identity/role from `profile.md` and prompt/tools from `execution.md`; automatic private memory loading remains deferred
   - the explicit turn/task objective and authorized constraints (automatic employee-goal delivery remains deferred), bounded effective processes ([`processes.md` §3.4](processes.md#34-delivery-to-sessions-proc-7-proc-12)), and concise `agent-os` help; standards are discoverable references retrieved as needed
   - the project's instruction files, **read with hardened office Git from each project/repository association's required `integration_branch` in a verified cache**; there is no empty-branch fallback. Never trust agent-movable worktree refs as instruction authority
   - explicit approved input guidance; the earlier editable automatic **wake guidance** (RUN-11) remains a scheduler proposal. If that feature is later selected, project guidance must use each association's required integration branch

   In the earlier `mirror.git`/`work.git` layout (A57), instruction files, guidance files and project processes ([`processes.md`](processes.md)) are all read from the integration branch as fetched into `mirror.git`, so all three follow merged work. Reading in `mirror.git` instead of `work.git` follows `tasks-projects-docs.md` §4.9 ("reads only in mirror.git"): agents can move refs in `work.git`, and once processes gate enforcement, a moved integration ref there would let one agent change what every session reads.

   So what an agent writes in its worktree never changes what the office tells the next session. Codex and Antigravity also read instruction files from the cwd by themselves (`AGENTS.md`, `.agents/rules`). That's the task branch's own content, so it can steer only that task's sessions, and it's treated as untrusted (see "Security"). **Claude doesn't:** under `--setting-sources ""` it read none of them in the spike, and auto-memory is off (decision 6). A nested `CLAUDE.md` that Claude might load lazily, when the agent reads files in a subfolder, wasn't tested, so it's still treated as untrusted.

   Each runtime receives the office's instructions its own way:
   - **Deferred native identity proposal (rev 3.5):** provision a protected replica file in the supported harness's native agent folder, from the canonical employee profile. Register/select that same version explicitly where isolated launch settings disable file discovery. A delegated replica needs its own persona/process content; it cannot rely on the main thread's appended prompt. See [`employee-supervision.md` §2](employee-supervision.md#2-native-harness-definitions) for researched formats and gates. The Claude inline path below remains a supported registration mechanism, not a substitute for the generated artifact.
   - Claude: `--agents <json>` defines the agent inline (name, description, a one-line prompt, and `tools`, the allowlist) and `--agent <id>` selects it. The instructions go in `--append-system-prompt-file`, so they don't show in `ps`.
     - **The id is reserved:** `org-<member id>` for an agent and `org-run-<run id>` for a one-off, **with the ULID lowercased** (rev 3.4: the id is an `AgentName`, `[a-z0-9][a-z0-9-]{0,63}`, so `org-01m6…`). The description keeps the id's original case. Claude's built-in agents (`claude`, `Explore`, `general-purpose`, `Plan`, `statusline-setup`) are listed in every `initialize`, and no built-in starts with `org-`, so an id can't pass the check below by colliding with one. The description is the office's own (`Agentic Org: <member id>`, or the run's).
     - **An unknown `--agent` falls back silently** to Claude's default agent, and the `system/init` event doesn't name the active agent. So the adapter sends the `initialize` control request first (it lists the loaded agents with their descriptions, at no model cost), and fails the start unless that list holds the reserved id **with the exact description the office set** (the adapter compares both, never the id alone). The `init.tools` check (decision 6) is a second sign that the inline definition loaded.
   - Codex: `developerInstructions` on `thread/start` / `thread/resume`.
   - Antigravity: the earlier `<runtime dir>/.agents/agents/<id>.md` / `--add-dir` / `--agent` mapping is **unverified research**, not an established native format. Keep it unavailable until installed-version fixtures prove loading/selection and isolation; do not synthesize a format.

   New instruction revisions apply to the next logical turn; recovery preserves the admitted input/policy. An explicit authorized new input is not a silent mutation of the frozen turn.
9. **Office-run sessions load no mods and no inbox hooks.** The office owns admitted input, presence and control. This does not select automatic wake/goal behavior. Existing inbox hooks stay for sessions people start interactively until their separate transport cutover.

## The model

| Record | Selected meaning and authority |
| --- | --- |
| Employee/config | Existing agent employee plus optional `execution.md`; config is required before runnable, with one `model_profile_id`, canonical `model_id`, tools and system prompt. |
| Model profile/pool | 1–64 connection IDs in contiguous priority order; models derive from confirmed discovery, not a handwritten map. |
| Provider connection | Organization-wide definition; a provider subscription is registered once, reused across offices/pools and retains its identity when disabled. Creation also creates one generated single-connection default profile. |
| Local binding/observations | Private credential reference and binding revision per office/connection. Auth/model evidence is fresh for at most 300 s; startup/explicit refresh and 60 s in-use probes run outside the routing hot path. |
| Logical turn | One immutable admitted input/model/pool/workspace/policy snapshot with one employee/office/placement generation. States: accepted, running, switching, blocked, recovery_required, succeeded, failed, cancelled. Blocked/recovery-required turns retain the open slot. |
| Attempt | Actual selected connection, permit and optional process for that turn; skipped candidates do not fabricate running attempts. Failed attempts retain consumed usage. |
| Process instance | planned → starting → idle/busy → stopping → gone; uncertainty means cleanup_required and retains the exclusive slot. Compatible idle processes may serve later turns. |
| Permit | HQ-coordinated connection capacity, bound to employee/office/placement/model/coordinator generation. Expired permits remain release_pending until verified completion/cleanup. |
| Approval | Bounded authorized tool request with attempt context; expiry denies it and replacement attempts cannot reuse it. Fixed security denies always win. |
| Budget/manual/security controls | Independent durable reasons, separate from employment, current task/assignment and observed process state. `stop_pending` is not `held`. |

A native conversation/session is adapter context; it is neither the logical turn
nor a process-lifetime Run. UI grouping may retain run history without renaming
turn/attempt/process identities or introducing a release-style R- prefix. Stable
IDs and legacy IDs remain valid; new stable entity IDs follow document 06.

### Existing adapter contract mapping

The following `org-runtime` types are the existing adapter-side contract, not the
new durable data schema. Preserve their validation and hash semantics. In
particular, `spec_hash` is a legacy change detector; it cannot substitute for the
selected SHA-256 input, command, checkpoint and launch evidence. The session runner
must map admitted routes/bindings and immutable inputs into these types before
launch. Changes to public types require their normal versioned contract review.

- **SessionSpec** (what the office asks the runtime for; stored with the session and hashed) `{ runtime, kind (home | task | one_off), agent (name, description, prompt), model?, effort?, workspace (cwd), read_only_dirs, instructions, permission_mode, policy, sandbox (enabled, writable_roots, deny_write, deny_read, allowed_unix_sockets), mcp_servers (office-provided only), tools }`
  - **`tools` is required** (rev 3.3): the tool allowlist, ordered. There's no "the runtime's default" (decision 6, Claude's tools). Each adapter also knows its runtime's disallowed list and checks the tools the runtime reports.
  - `workspace` is the agent's home for the home session, and the task's worktree for a task session.
  - **Deterministic:** ordered collections only, and no per-run value. So every run of a session, its first start and every resume, has the same `spec_hash`.
  - **`spec_hash`** is a 64-bit FNV-1a over the spec's canonical JSON. It's a change detector, not a security check.
  - **Paths are UTF-8 by type.** Every path is a `SpecPath`: absolute, normalized (no `.`, `..` or empty parts, no trailing `/` except `/` itself). It refuses every Cc control (NUL, line breaks, tab, ESC, DEL, C1), U+2028 and U+2029, and every Cf format character as of Unicode 17.0 (bidi controls, zero-width characters and joiners, the soft hyphen, invisible operators, number marks, tags; 170 code points, pinned by a test). `PathPattern` and `CommandPrefix` use the same set. The crate is the exact reference. Patterns, names and tokens are validated types too, so none of them can break out of the argv, file or rule it's written into. Free text (the instructions, the agent's description and prompt, MCP server arguments) reaches argv and files as is, so the office keeps secrets out of it. `org-office-sessions` converts the office's paths at that boundary, and the conversion returns a `Result`: a path that isn't valid is an error, never a lossy copy. The spike tested only real paths, so the office resolves symlinks before it builds the sandbox lists:
    - **Writable roots and allowed sockets** (what widens access): the parent folder is resolved to its real path (e.g. `/private/tmp`), and the last name is appended as it is. A symlink in the last part is never followed: agents can write some of the literal paths (a home's `.git/index.lock`, `HEAD`, …), so a symlink there would widen the writable roots. If one is a symlink, the office refuses the start, and the agent goes to Needs you.
    - **Deny entries:** both the pattern as written and its real-path form, so a symlinked dotfile (`~/.zshrc` from a dotfile manager, say) is still denied. Many deny entries are patterns, or don't exist yet, so the office resolves the longest existing prefix (stopping at the first wildcard) and appends the rest unchanged. If resolving fails for a reason other than "not found", or the real-path form isn't a valid `PathPattern` (an iCloud `~` or a `(` in it, say), the office refuses the start, and the agent goes to Needs you with the path named.
  - **Unknown fields fail to decode** (`deny_unknown_fields`) in every part of the spec, because a field this build doesn't know may be a restriction. Enums the office writes (the permission mode, the session kind) **fail closed** on a value they don't know. Enums that come from a runtime (events, usage, rate limits) are **`Unknown`-tolerant**: they decode an unknown value as `Unknown`.
- **LaunchContext** (per run, never part of the spec or its hash) `{ runtime_path, runtime_version?, runtime_dir, tmpdir, office_env_names, start_denies (deny_read, deny_write), new_session_id?, resume? }`
  - `start_denies` holds the deny entries the office enumerates at each start (rev 3.3), split like the spec's `sandbox` into `deny_read` and `deny_write`. Reads and writes: other offices' folders. Writes only: other tasks' worktrees and their `work.git/worktrees/<id>/` admin folders, this office's other projects' `work.git/` and `worktrees/`, user-writable `PATH` folders and the folders their symlinks resolve into, the Homebrew prefix. They change from start to start, so they're here, and the spec (and its hash) stays deterministic. Every adapter adds them to the sandbox and the permission policy like the spec's own denies.
  - `resume` is the runtime session id to resume. It lives here, not in the spec, so a resume replays the same spec.
  - `runtime_path` is the real path recorded at preflight, and `tmpdir` is the session's `TMPDIR`, which every adapter adds as a writable root.
  - **The adapter sees environment variable names only** (`office_env_names`: `ORG_SESSION_TOKEN`, `ORG_AGENT`, …), for runtimes that must pass them to their tools by name (Codex `shell_environment_policy.include_only`). The values (the session token, the mark) never reach it.
- **The `Session` contract.** Every adapter keeps it, and the port's testkit checks it against each adapter's fixtures:
  - **Normal input waits for the turn.** A normal `Send` during a turn returns `Busy`, and the office queues it. An urgent one steers the turn where the runtime can, and otherwise returns `NeedsInterrupt`: the office interrupts, then sends.
  - **An error changes nothing.** When a call returns `Err`, nothing was emitted (no write, no event), and the session's state is as before.
  - **`Ended` is always the last event**, exactly once, from `on_exit`. After it every call returns nothing, and `control` returns `Closed`.
  - **`Output` and the framer's `Frame` are exhaustive.** They instruct the pump, which can't safely skip one it doesn't know, so a new variant breaks the pump's build until it's handled.

### Runtime usage evidence

Accounting in document 10 owns the durable ledger: preserve actual consumer,
route/attempt metadata, partial/unavailable counts and immutable corrections.
The earlier native observations below inform normalization; they do not select
an aggregate process-lifetime cost counter as the source of truth.

- **Legacy adapter usage per native turn:** tokens in, out and cached; estimated `cost_usd` and model. New ledger events use document 10's integer micro-USD, price versions and explicit completeness; never persist a missing report as zero.
  - **Claude:** a turn's `result` reports its usage, and so did a turn interrupted during a tool. **A turn aborted mid-stream (`terminal_reason: aborted_streaming`) reported zero usage**, and its `total_cost_usd` didn't grow. So the office keeps a running per-message sum from the turn's `assistant` events, once per API message (one message can arrive as several events with the same `message.id`). It uses that sum when the `result`'s usage is all zero, and when no `result` comes at all (a crash, a kill, a stop mid-turn). Such a turn's cost isn't in `total_cost_usd`, so the cost estimate undercounts it (a known limit). The `assistant` copy of `usage` is the message's start snapshot, so it can undercount output tokens; the final count is in `message_delta`. The Claude adapter launches with `--include-partial-messages` (live text deltas need it too), so it uses `message_delta` where it has one.
  - Codex: `thread/tokenUsage/updated.last`.
  - Otherwise (Antigravity's cumulative `usage`): a baseline at the first report after any start or resume, then deltas.
  - Claude's `total_cost_usd` (cumulative per process) is native estimation evidence, not a canonical ledger aggregate; any permitted display labels its basis/completeness. Codex and Antigravity are priced by the office from a **price table** at API prices in US dollars (the user's rule). The price table is plan step S2e.

## Sessions and triggers

The selected first release accepts explicit authorized turns only. Messages,
assignments, schedules and goal reminders do not implicitly create model work.
A caller resolves visible input and an approved workspace before `RunTurn`.
A second open turn returns BUSY; there is no hidden scheduling queue. Native
`Session::control` may return Busy/NeedsInterrupt, but that adapter result does
not authorize a second logical turn or replay an uncertain effect.

A future run-mode scheduler may retain per-workspace queues, reply-to-origin
routing, turn-boundary switching, urgency, minimum slices and starvation
prevention from the earlier proposal. Those policies and crash-trigger requeues
remain unselected. During an authorized takeover, exclusivity still forbids
concurrent office input. A scheduler must use the same execution admission and
recovery service rather than becoming another process owner.

## How a run goes

1. **Admit explicit input.** Authenticate caller/scope and validate placement/config
   revisions. Persist the immutable input and actor-bound operation/digest receipt
   before acknowledging. Same-ID retry returns that outcome; different payload
   conflicts. An unknown acknowledgement is resolved with the original ID.
2. **Choose the same model.** Traverse the frozen ordered pool once per dispatch
   cycle. Require enabled definitions, matching local binding, verified installed
   driver, fresh auth/entitlement and compatible tools/policy/context. Skip with
   concrete reasons; do not discover models, fetch Git or authenticate on this path.
3. **Acquire quota.** Obtain an HQ permit before work. Coordinator unavailability
   blocks the turn. The default is one concurrent turn per connection, shared by
   all offices and pools; a local office cap cannot substitute for it.
4. **Reuse or safely switch.** Reuse verified idle context only when connection,
   model, driver/version, workspace and launch digest match. Otherwise verify old
   cleanup before replacing. Persist a fresh mark before spawn, then process
   boot/start/PID identity before input; local credentials never enter argv/logs.
5. **Execute and account.** Journal attempts, tool intent/results and coarse state;
   keep streamed text provisional until a single committed final outcome. Record
   actual-route usage including failed/partial attempts before final completion.
   Prompts/private content do not enter ordinary logs.
6. **Handle failure safely.** Retry only eligible quota/rate/auth/model failures.
   Prove no effects before replay, or a compatible checkpoint with completed tool
   receipts before resume. Unknown effects, missing receipts or unsupported
   portability mean RECOVERY_REQUIRED; retain the slot, workspace and evidence.
   Never ask a fresh model merely to “check its state” in lieu of reconciliation.
7. **Finish.** Turn success is separate from task closure. Release capacity only
   after a verified terminal/idle boundary and no outstanding work. A compatible
   idle process may stay warm without a permit. Cancellation remains nonterminal
   while effects/cleanup are unknown.
8. **Recover.** The office's single recovery owner replays protected journals,
   validates digests/receipts and reconciles marked processes before admitting
   work. It never automatically closes unknown turns or requeues their effects.
   Cache loss cannot clear holds, grants, used extensions or placement authority.

## Quota

HQ journals connection permits and cooldowns before acknowledgement. Permits last
60 s and renew every 10 s; renewal failure starts stop/recovery with at least 20 s
remaining. Expiry moves to release_pending and still consumes capacity. HQ restart
reconciles old-generation permits before issuing new ones. A 429 is connection-wide
unless verified provider evidence narrows it to a model; use Retry-After/reset or
900 s by default. Each candidate is tried at most once per cycle; exhausted pools
block with ordered skip reasons and require explicit ResumeTurn for another cycle.
Quota probes may remain unknown; fresh entitlement plus bounded admission is still
required. A cached healthy flag is never quota authority. The earlier proposal to
reserve 20% of a runtime-wide five-hour window is not selected by this contract.

Budgets are independent of provider quota. Document 17 selects task and agent
budget controls and release warning caps; evaluators/stop coordination remain
unimplemented. Known usage at a limit creates its own stop_pending hold, finishes
the current turn and stops affected admission. Only worker acknowledgement makes
it held. Daily/monthly/task/manual/security holds coexist; raising/clearing/extending
one policy releases only satisfied holds. Durable one-time extension decisions and
period boundaries survive cache rebuild. Unknown usage is visibly incomplete and
warns; it is neither zero nor an automatic stop. Release caps never stop turns.

## Run modes (Eden's spec)

Automatic always-on/on-call/scheduled behavior, memory extraction and native
helpers remain earlier UX proposals, individually marked in
[RUN](../specs/agent-run-modes-ux.md). Existing `AgentProfile.run_mode`, `schedule`
and `member.set_run_mode` in the v1 wire contract are compatibility fields; they
do not turn a target employee profile into scheduler state or prove an engine.
Unknown run modes/statuses still fail closed under the current authentication
contract. Employee identity remains in profile.md, execution in execution.md,
placement in HQ authority, and scheduler import must preserve legacy values
without making up a target scheduling schema.

Pause records desired/manual state independently of observed cleanup, employment
active/inactive and budget/security holds. HQ may durably accept Pause while the
office stop is pending. Reconnect applies the latest revision; stale delivery must
not override a later Resume/offboard. Resume clears only its authorized manual
reason and is not permission to clear any other hold or launch automatically.
`StopAgent` stops current active/idle processes and creates no future wake policy.
The existing legacy `agent.stop` → Pause semantics are retained only in the
versioned transport sketch below; they are not a renaming of `StopAgent`.

### Offboard and remove
Current v1 `member.offboard`/`member.remove` are legacy transport behavior, not
permission to delete the selected durable employee identity. Import maps offboarded
to inactive, retains the handle/stable ID/history and preserves missing timestamps.
Target removal must archive referenced identity and historical work. On applied
offboard/removal authority:
- **Sessions stop at once**, with no checkpoint turn: the agent no longer works for the organization. The live run ends `offboarded`.
- **Tokens and bindings go.** The run's session token is void, and every connection bound with it, or bound as that agent, is closed. No new token is ever issued for it, so `msg.bind` as it is refused with `not_authorized`.
- **The schedule is dropped**, and so are its queued triggers. The agent's pending approvals are `cancelled`, as for any session that stops (decision 7).
- **Worktrees:** preserve task workspaces through offboard/handoff until task closure or explicit safe migration. Push committed branches where possible; retain and flag uncommitted or unpushed work. A successful push alone does not authorize recursive deletion.
- **The session records and runs stay**, for history. Transcripts stay in the runtime's store.
- **Calls on it:** `agent.wake`, `agent.send`, `agent.interrupt`, `agent.stop`, `session.take_over` and `permission_policy.set` on an offboarded agent are refused with `invalid_request` (reason `offboarded`), as `v1` does for a member in the wrong state. On a removed one they're `not_found`.
- Messages to it follow ADR-0003 and the work-items design.

## One-offs

A role-based one-off remains an earlier UX/integration proposal, distinct from an
agent employee and from a native delegate. Accounting supports `actor_kind: one_off`
without an employee ID; this does not select launch, messaging or a new quota bypass.
RUN-12 retains display labels such as “Reviewer (one-off run 12 · Studio Mac)”
without R- identifiers. The v1-era wire sketch below preserves run-scoped token,
peer-ancestry, job-scoped messaging, fixed denies and control requirements if this
feature is later implemented. No current one-off runner is claimed.

A future implementation must admit its model work through the reviewed shared
connection/permit service (or an explicitly reviewed extension for its non-employee
identity), including coordinator outages. It must not advertise offline launch
merely because a local run record can be written. Record actual usage, preserve
uncertain outcomes for recovery and never automatically repeat a crashed one-off.
Task-attached workspaces survive run completion/handoff until task closure;
throwaway non-task temp inputs can be removed only after verified process cleanup.
Definitions, leases, tokens, approvals and descendants remain bounded and scoped;
one-off policies never widen role defaults. No per-office run counter is added to
the selected entity-numbering model by these illustrative labels.

## Taking over (a person works with the agent directly)

> **Owner decision 2026-11-17 ([ADR-0004](../adr/0004-headless-agent-sessions.md)).** There is
> no terminal in the browser. A person interacts with an agent through chat, and the
> browser shows the session read-only. Step 1 below stays. Step 2 changes: the local
> `agentic-org` command, not the office, starts the interactive process, in the
> person's own terminal, and hands back when it exits (ADR-0004 decision 4). Step 3, the lease that carries terminal frames to the app, and
> the `session.input`, `session.resize` and `session.output` calls are **not v1** and are
> kept only as a record of the earlier proposal. The session view is specified by
> [REQ-05](../specs/req-05-virtual-console-conversation-log-viewer.md) and questions to
> people by [REQ-04](../specs/req-04-channel-messaging-and-question-components.md);
> they replace this section's app behaviour.

- **Every runtime takes over the same way: stop, then resume interactively.** The office spawns the interactive process itself, through `org-supervise`, with the full replayed spec and the same environment allowlist (a fresh session token included):
  1. The office interrupts and stops the headless process, keeping the agent's lock. Its run ends `taken_over`. For Codex it unloads the thread (`thread/unsubscribe`) first, then stops the app-server.
  2. It starts the interactive CLI in the session's workspace, in a PTY it owns, as a new run (trigger `owner`): `claude --resume <id>`, `codex resume <thread id>` with the same `-c` isolation pins, or `agy --conversation <id>`.
  3. The app attaches to the office's PTY, using spike S2's pty-host frames. Each frame carries at most **64 KiB** of terminal data, well inside `MAX_FRAME_BYTES`.
  - The Claude spike confirmed the round trip: headless, then `claude --resume` in a PTY, then headless again keeps the same session id and one transcript, and the conversation carries both ways. With the user's real `~/.claude` (decision 6), there's no onboarding to pre-seed. Claude's trust dialog is per folder, though, so it may still show for a worktree or home the user never opened (untested). Then the person accepts it. The office doesn't record the trust itself: that would write the user's `~/.claude.json`, which the user's own sessions write too.
- **The frames travel only on the lease holder's connection.** There's no separate stream or socket. The app-bound connection that called `session.take_over` holds the lease:
  - Output reaches only that connection, as `session.output` events. They're never sent to any other subscriber, app-bound or agent-bound.
  - Input and resizes come only from it (`session.input`, `session.resize`, with the `lease_id`). The same calls from another app connection are refused with `held`, and from an agent-bound connection with `not_authorized` (they're app-only). So no agent can type into the person's terminal, or answer its permission prompts.
  - If that connection closes, the office hands the agent back at once, as when the lease runs out.
- **Codex joining a live thread** (`codex --remote`) isn't used. It would need a listener that agents can't reach: a spike can try `--listen ws://127.0.0.1:0 --ws-auth capability-token --ws-token-sha256 <hash>`, with the token kept in office memory and given only to the TUI through `--remote-auth-token-env`. Until that's proven, Codex takes over like the others.
- **Antigravity's take-over** loads the user's `~/.gemini` (decision 6) unless the spike finds a way around it.
- **Only agents are taken over.** One-offs aren't (see "One-offs").
- **Which session:** `session.take_over` names one (`session: "home" | { task_id }`). The default is the live session, or the home session when nothing is live. Taking over a session that isn't live while another one is stops the live one first, as a switch (at once, with no checkpoint turn, because the owner asked).
- **Codex's TUI** can change its approval policy or sandbox from inside (`/approvals`). See the known limit under "Security".
- **Lease.** The app sends a heartbeat every **10 s**. If none arrives for **30 s**, the office ends the interactive process and hands the agent back.
- **Hand-back.** For an always-on agent the office resumes headless. For an on-call agent it just releases the lock.
- The agent's presence shows **"With you"** meanwhile.
- The session panel shows the normalized event stream (messages, tool calls, approvals) whether or not anyone has taken over.
- **An office-run agent that a person starts by hand** (`bin/agent`, a terminal) has no session token, so it can't bind to messaging as that agent. To work with an office-run agent, a person uses take-over. That's documented for the owner.

## Messaging (ADR-0003) with office-run sessions

**Target delivery policy (rev 5).** Apply [Chat's noninterrupting delivery
requirements](../specs/chat-ux.md#42-system-notices-and-delivery). Message urgency
orders delivery and badges; it never authorizes urgent steering, cancellation or
injection into a busy turn. Queue messages for the next permitted boundary;
paused agents remain paused and hand-run agents read through their own message
checks. A reply can satisfy an existing wait without creating a second turn.
The earlier `Session::Send` urgent/NeedsInterrupt behavior below is runtime-port
compatibility evidence and remains available only through an explicitly
admitted control path, not automatically from message priority. Explicit owner
Interrupt/Stop and mandatory security, offboarding, fencing and recovery stops
retain their selected authority. A queued message does not admit an automatic
wake or bypass an independent hold/permit.

- For a session the office runs, the office **is** the wake path. It sends "you have messages" as a turn, at a natural break, using the same urgent/normal triage rules. The inbox hooks (codex-dev's adapters) stay for sessions a person started interactively, such as ours today.
- **Session token.** An agent with a `run_mode` is office-run. `msg.bind` as it requires **both**:
  - the live run's `ORG_SESSION_TOKEN` (ADR-0003 M-A, amended). `agent-os` reads it from its environment.
  - **peer ancestry:** the office reads the caller's audit token (`LOCAL_PEERTOKEN`: the pid plus its pid version, so a reused pid doesn't match) and walks its parent chain. The chain must include the session's recorded child pid.

  A missing or wrong token, or a failed ancestry check, is refused with `not_authorized`. Agents people start interactively keep ADR-0003's accepted risk.
- **It fails closed.** A token is required for any agent with a `run_mode`, `Unknown` included. Binding as an offboarded agent, an agent whose status this office can't read (`Unknown`), or a member that isn't found, is refused with `not_authorized`. If the office can't tell (a lookup fails), the bind is refused too.
- **Where it's checked.** The messaging library (`org-mailbox`) owns a narrow port, `SessionAuth` (verify a token for a member or a one-off run, with the caller's audit token). `org-office-sessions` implements it from the office's live runs and the OS process table, and `agentic-org-office` wires it in. Tokens are compared in constant time.
  - When a run's process exits, its token dies and the office **closes every connection bound with that token**, so a leftover `msg wait` can't keep acting as the agent.
  - **Residual risk:** ancestry is checked at bind only. A bound connection's file descriptor that's passed to another process (inherited, or sent over a socket) keeps working there until the run ends and the office closes it.
- **Waits are capped.** On a token-bound connection the server caps `wait_ms` and `timeout_ms` at **60 s**. `agent-os` lowers its own defaults to match when `ORG_SESSION_TOKEN` is set.
  - An `ask` keeps its full deadline (`ttl_ms`, up to 24 h for a person). After the 60-second hold, `agent-os msg ask` exits `2` with `"pending": true`, and the reply wakes the agent when it arrives.
- **No spool for office-run agents.** They die with the office, so they never need it. `agent-os` with `ORG_SESSION_TOKEN` set exits `5` instead of spooling.
- **Spool ingest accepts only active agents run by hand.** The spool is the one path into messaging that skips `msg.bind`, and any process of the user's can write a file into it. So at ingest, the office resolves the spooled send's `from` (at its applied org position) and accepts it only if it's a member of kind agent, with status `active` and no `run_mode`. Everything else goes to `spool/dead/` with a log line (its id, the reason, never the body):
  - a person, `owner` or `office`
  - a run address
  - an office-run agent (any `run_mode`, `Unknown` included)
  - an offboarded or paused agent, or one whose status is `Unknown`
  - an id or handle that resolves to no member

  So **M-A is closed for office-run agents**, and **forging a person's or the owner's message through the spool is closed for everyone**. A hand-run agent can still spool as another hand-run agent: that's ADR-0003's accepted risk for them. The one-time `import-v1-old-spool` at cutover is a separate path, run by the owner, and imports Phase A's history as it is.
- **Presence** (ADR-0003, amended): `busy`, `idle`, `needs_you`, `with_you`, `asleep` (derived) and `offline`. An agent whose connection holds an `agent-os` wait (`msg.wait` or `msg.ask`) counts as **idle**, though the time still counts toward the run's busy time.
- `agent-os` works the same in both: agents read, answer and defer through it.

## Git in worktrees

Document 04's target paths above use one cache per registered repository/office
and task/employee worktrees. Document 02 owns project/repository associations and
their required integration branches. Before launch, recompute real-path grants,
Git common/admin-directory permissions, config/hooks/credential denies and all
other-task/employee/state isolation for that topology. A renamed prefix is not
security proof. Preserve unpushed work and task workspaces until closure; safe
assignment/placement migration must verify previous ownership and workspace state.

### Legacy `work.git` security evidence (revisions 3.3–3.7)

The following exact layout and probes describe the earlier `mirror.git`/`work.git`
adapter proposal and inspected spike. They remain required regression evidence
for that layout, not the target folder tree or proof of target sandbox readiness.

This applies `projects-git-github.md` (a bare `work.git` and a worktree per task) to sessions the office runs.
- **What agents can write.** A task session's writable roots include only:
  - its worktree, `worktrees/<task id>/`
  - `work.git/objects/` (except `objects/info/`), `work.git/refs/`, `work.git/logs/` and `work.git/worktrees/<id>/` (its own worktree's admin folder, except its `commondir`, `gitdir` and `config.worktree` files), which `git commit` needs

  These are the only writable roots inside `<office home>` (see "Security"). `work.git/config`, `work.git/hooks/`, `objects/info/` (alternates, commit-graph) and everything else in `work.git` are **read-only** to agents. The office's own git also runs with `core.commitGraph=false`, and refuses a `work.git` that has `objects/info/alternates`.
- **Explicit `denyWrite` entries, for every runtime** (rev 3.3). Being outside the writable roots isn't enough: Claude's sandbox adds the git common dir by itself (below), and a release could change what it opens, while versions only warn. So every runtime's sandbox and permission policy get a `denyWrite` (a fixed deny) for `work.git/config`, `work.git/config.worktree`, `work.git/hooks/`, `work.git/worktrees/*/config.worktree`, `work.git/objects/info/`, `work.git/HEAD`, `packed-refs`, `info/`, `description`, `FETCH_HEAD`, each worktree's `commondir` and `gitdir` (`work.git/worktrees/*/commondir`, `…/gitdir`), and the admin folders of the other worktrees that exist at start (`work.git/worktrees/<other id>/`, named). The entries other than the named admin folders are wildcarded over every project's `work.git` (`<office home>/offices/*/workspace/projects/*/work.git/config` and so on), not only the session's own project's. None of them is an ancestor of a writable root. S1 asserts each one under Claude's sandbox, and S6 for every runtime. A filter or diff driver planted in `work.git/config` would run in other agents' sessions and in the office's own fetch (which runs in `work.git`, `projects-git-github.md`), so these never depend on a runtime's defaults.
- **Claude's sandbox opens more of `work.git` by itself.** For a linked worktree, the spike's probes show it adds the git common dir with its own denies (`config`, `config.worktree`, `hooks/`, `objects/info/`, `commondir`), but it allows writes to `work.git/HEAD`, `packed-refs`, `info/exclude`, `description`, `FETCH_HEAD` and `worktrees/<id>/gitdir`. The `denyWrite` entries above cover all of them. The spike showed that `denyWrite` overrides the sandbox's own allows, and that a commit still works with them.
- **Agents have no writable git config** (rev 3.3). With `work.git/config` and every `config.worktree` denied, `git config` and `git config --worktree` both fail in every runtime (Claude's sandbox already refused `--worktree`). Agents' git settings come only from the environment. So the office no longer sets `extensions.worktreeConfig` (it existed only for per-worktree config): `work.git` keeps `core.bare` in its own config, and git never reads a `config.worktree`.
- **The office sets the config that matters through the environment** (decision 6): `GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL=/dev/null`, and, through `GIT_CONFIG_COUNT`/`KEY_n`/`VALUE_n` (which override every config file), the commit identity (from `projects-git-github.md`), an empty `credential.helper`, `core.hooksPath=/dev/null`, `core.fsmonitor=false`, `gc.auto=0` and `maintenance.auto=false`. Without the last two, an automatic gc would try to write outside the writable roots.
  - So a hostile `config.worktree` or a planted hook in agent A's worktree never runs in agent B's sessions.
- **The office's own git near a worktree.** In `work.git/worktrees/<id>/`, the `commondir` and `gitdir` files are read-only to agents (left out of the writable roots). Before any office git command touches a worktree (`worktree remove`, re-creating one, the check for uncommitted work), the office deletes that worktree's `config.worktree` and rewrites `commondir` and `gitdir` to their known values. Agents can't write the worktree's own `.git` file (a fixed deny), but as a second line of defence the office also deletes it (file, folder or symlink) and writes it again with its known value, and it never lets git discover the repository through it: it sets `GIT_DIR=work.git/worktrees/<id>` and `GIT_WORK_TREE=<the worktree>` explicitly. (`git worktree remove` checks that file, so rewriting it also keeps removal from getting stuck.) Then it runs with its hardening (`projects-git-github.md`): no global or system config and hooks off. So no filter driver, hook or redirected repository that an agent planted can run in the office's git.
- **A task that changes hands.** When a task's worktree passes to another agent, the office deletes any `config.worktree` first (agents can't write one, so this is a second line of defence), so agent A's settings (filters, `diff.external`, a pager) never run with agent B's token.
- **A missing worktree** is re-created from the pushed task branch through `org-git` (one operation per repository at a time). If the branch was never pushed, the agent goes to Needs you, and the item flags the unpushed work that may be lost.
- **Known limit:** objects and refs are shared in `work.git`, so a hostile agent could move another task's branch. The office's push check and review catch it, and real isolation is v2 (`projects-git-github.md`).
- **Home sessions in a git repository.** An agent's home is often a folder inside a repository (ours are in the agents repository), and the agent commits its memory and journal there. It can't be the repository's root (see "Security"). Its writable roots then also include, in that repository's `.git/`, only what a commit needs: `objects/` (except `objects/info/`), `refs/` and `logs/`, plus these files by their literal paths (they may not exist yet): `index`, `index.lock`, `HEAD`, `HEAD.lock`, `ORIG_HEAD`, `COMMIT_EDITMSG`, `packed-refs`, `packed-refs.lock`, `AUTO_MERGE` and `AUTO_MERGE.lock`. (Without the last two, a commit works with a harmless lock error.) Everything else in `.git/` stays read-only, including `config`, `hooks/`, `info/`, `commondir`, `worktrees/` and `modules/`. With the fixed deny on writing any `.git` entry inside the workspace (under "Security"), an agent can't point git at a config it wrote. The same environment applies, so that repository's own hooks don't run in office-run sessions.
  - S0 checks that each sandbox can allow a literal path that doesn't exist yet. Claude's can (verified: the commit worked with several of them missing). If one can't, it keeps `config`, `hooks/`, `info/` and `objects/info/` denied and also denies `commondir`, `worktrees/`, `modules/`, `config.worktree`, `rebase-merge/`, `rebase-apply/` and `sequencer/`.
  - **Known limit:** the agent can move any ref in that repository, including other agents' and the user's own branches. Real isolation is v2.
- **Tests:** S0 checks that `git commit` works inside each runtime's sandbox with these roots, for a task worktree and for a home inside a repository, and notes any harmless lock errors (a commit can try `work.git/packed-refs.lock`). Done for Claude: a task commit works with `work.git/HEAD`, `packed-refs`, `info/`, `description`, `FETCH_HEAD` and `gitdir` denied (one run), and with `objects/info/`, `commondir` and `gitdir` denied (another run); the combined list wasn't run, so S1 runs it. A home commit works with the design's roots; the office's git environment applies inside the sandbox (the run with the full deny list didn't pass that environment). **The spike's commits ran in its own layout under `/tmp`, not under `<office home>`**, so S1 commits from a task worktree at its real path (`<office dir>/workspace/projects/<project id>/worktrees/<task id>/`), with the full deny table under "Security" in force and the rev 3.3 git environment (`GIT_CONFIG_GLOBAL=/dev/null`, `core.hooksPath=/dev/null`). The spike probed only listed paths in `work.git` and only the agent's own worktree folder, so S1 enumerates every path in `work.git` under Claude's sandbox (other worktrees' folders, `shallow`, `modules/`, lock files, …) and builds the `denyWrite` list from that. Until then, more of `work.git` may be writable than this section says. S6 checks that a hostile `work.git` config or hook planted by agent A doesn't run in agent B, and that a hostile `config.worktree` with a `.gitattributes` filter, or a worktree `.git` file pointed at a planted repository, doesn't run anything during the office's worktree removal or re-creation, and that removal and re-creation still succeed. And when a home session tries to write `.git/commondir`, `.git/worktrees/*/commondir` or a new `<home>/.git`, those writes are denied, and the user's `git status` runs nothing.

## Processes and safety

The agent supervisor and session runner are modules inside the one
agentic-org-office process; org-supervise is an OS-control library. There is no
per-employee daemon or second model supervisor. Pure domain rules stay in
org-office; org-office-sessions composes adapters and OS primitives. A single
worker recovery owner and SQLite writer apply protected journal transactions.

Persist mark before spawn and boot/start identity before input. Keep incomplete
ownership and captured descendant inventory until verified cleanup; deadlines,
PID/group numbers, cache disappearance and a successful push never prove exit.
Use the implemented ownership/recovery policy in
[org-supervise](../../crates/org-supervise/README.md), including checkpoint-before-
signal, exact identity checks, bounded scans and its documented containment limits.
A zombie-only macOS EPERM is accepted only under that crate's verified conditions,
not as a generic “already gone” signal. Failed cleanup retains the exclusive slot.
Native orphan recovery exists as a library API, but automatic owned-stop recovery,
host/server wiring, PTYs and real-runtime admission are separate integration work.

The host must not race the live worker's ownership. The private launch-pipe child
inventory is current lifecycle evidence, not durable employee placement authority.
On transfer HQ requires verified old cleanup or an explicitly verified machine
fence, journals the new office/incremented placement generation atomically, and
includes it in every command/permit/attempt. Missing authority evidence blocks
starts; no timeout-only reassignment or restored PID signaling is allowed.

Drivers must prove their installed versions, local account binding, required
isolation and policy/checkpoint/usage support before eligibility. Warning about
an unknown version does not qualify it to run. Credentials are office-local
connection bindings with private auth directories/vault references; the earlier
shared user-login spike is not the selected credential-isolation design. Tool
children receive no provider/API secret or host/database key.

- **The stdin protocol:** every line the office writes is built by the port's one encoder (`encode_line`, with `serde_json`), never by string formatting, and it **escapes U+2028 and U+2029 explicitly** (`serde_json` leaves them raw). Tests cover `\n`, U+2028 and U+2029 in user text.
  - **No slash commands from the office** (rev 3.4): Claude treats a user line whose text starts with `/` as a slash command, which could run outside `--tools`. Every office-sent user line starts with the office's own header (who it's from and why), so its text never starts with `/`, and the encoder refuses a user line that does. A message body that starts with `/` is sent as data after the header.
- **The stdout reader** is the port's one framer (`LineFramer`), which the pump uses for every runtime. It splits on `\n` only and caps a line at **4 MiB**. A longer line (for example a large image) is drained up to the next newline and replaced by a `truncated` event with its size. The session goes on.
  - An event of a type the adapter doesn't know is kept as a coarse `unknown` event (its type name, no body). A line that isn't valid JSON is a `protocol_error` event. Neither ends the session; only the child's exit does.
- **stderr** is filtered line by line (control characters dropped, each line length-capped) and goes through the same **redaction** as events, before it goes to `<office dir>/logs/agents/<agent>.log` (0600, capped and rotated, never bodies).
## Security

> **Amended by [ADR-0015](../adr/0015-agent-isolation-modes.md) (2026-11-17).** The
> statements below describe the `same-user` isolation mode. A second mode,
> `dedicated-user`, runs agents as an OS account that cannot read the organization
> data root; in that mode start, stop and cleanup go through a privileged launcher.
> The mode is part of the launch contract and of the launch digest.

The restrictions in this section remain required. Exact `<office home>`,
`work.git`, `mirror.git`, macOS and native-CLI rows below are **legacy layout
security evidence**, not the selected runtime layout. Preserve them for existing
adapter regression tests and derive/test equivalent or stronger restrictions at
the target roots before launch. These observations do not prove end-to-end
implementation or permit changing signed identities/key namespaces.

- **The workspace is hostile.** An agent can write anything in its worktree, so nothing there may change a session's **settings, permissions, hooks, tools or MCP servers**:
  - Settings come only from the office (decision 6), and the office's composed instructions come from the project's integration branch as fetched into `mirror.git` (decision 8; amended 2026-11-08: A57).
  - Instruction files that a runtime reads from the cwd by itself (`AGENTS.md`, `.agents/rules`; Claude reads none under `--setting-sources ""`, decision 8) are the task branch's own content. They can steer only that task's sessions, never another agent's or the office's, and they reach the integration branch only through review (amended 2026-11-08: A57). We accept that.
  - **A workspace skill's `allowed-tools` does pre-approve tools** in Claude, whenever the project setting source is loaded (the spike ran one with no approval request). Under `--setting-sources ""` workspace skills don't load at all, so that source is never turned on.
- **Fixed denies**, for every runtime, through its permission policy and its sandbox. They override every allow rule:
  - writes to `.claude/**`, `.agents/**`, `.codex/**` and `.gemini/**` in the workspace
  - writes to any `.git` entry (file, folder or symlink, at any depth) inside the workspace, so an agent can't make a nested repository or redirect a worktree's `.git` file.
    - Commits in a home need writes to its repository's `.git` (the commit paths under "Git in worktrees"). The deny above blocks those when `.git` is inside the home, so a home can't be a repository's root. Preflight refuses one, and the agent goes to Needs you.
    - **Known limit:** `git submodule update --init` writes `<path>/.git` inside the worktree, so agents can't set up submodules. The office can do it for them later if a project needs it.
  - writes to `work.git/config`, `config.worktree`, `hooks/**`, `objects/info/**`, `HEAD`, `packed-refs`, `info/**`, `description`, `FETCH_HEAD`, and every worktree's `config.worktree`, `gitdir` and `commondir` (explicit `denyWrite` entries for every runtime; see "Git in worktrees")
  - the named paths under **`<office home>`** in the table below (reads, writes or both, as it says)
  - **Every runtime must hold the read denies** (rev 3.3). They now cover more than encrypted databases and sockets (other organizations' offices, every agent's runtime dir, `~/.claude/**`, the Keychain files), so a runtime whose sandbox can't deny reads isn't office-run. Per S0 Codex, Codex's legacy `sandboxPolicy` can't, and a permissions profile can (a file deny was verified; the full lists are still to verify). S0 checks Antigravity.
  - writes to the **user's own config**, which runtimes and the user's tools load: `~/.claude/**`, `~/.claude.json` (the user's and local MCP servers), `~/.codex/**`, `~/.gemini/**`, `~/.agents/**`, `~/.config/gh/**`, `~/.gitconfig`, `~/.config/git/**`, `~/.ssh/**`, the shell start-up files (`~/.zshenv`, `~/.zprofile`, `~/.zshrc`, `~/.bash_profile`, `~/.bashrc`, `~/.profile`; `PATH` is captured from them), and `~/Library/LaunchAgents/**`. The runtimes write their own stores there themselves; this denies it to the agent's tools. The sandboxes' writable roots leave them out too.
  - **reads of `~/.claude/**` and `~/.claude.json`** by the agent's tools. `~/.claude.json` holds the user's MCP server configs (which can carry tokens) and per-project history. Office-run Claude agents use the user's real `~/.claude` (decision 6), so it holds every agent's transcripts and the user's own, and the user's settings and history. The Claude CLI itself isn't sandboxed and still reads it; its tools can't (the sandbox holds Bash, the policy's deny rule holds the file tools). A runtime whose sandbox can't deny these reads isn't office-run (above).
  - **`/tmp/cc-socks/**`** and **`/private/tmp/cc-socks/**`** (macOS's `/tmp` is a symlink, and the spike tested only the real path), reads, writes and connections. Each Claude process listens on `/tmp/cc-socks/<pid>.sock`, a fixed, machine-wide folder shared with the user's own interactive sessions, so the session's `TMPDIR` doesn't cover it. With `allowUnixSockets` listing only the office socket, an agent's tools can't connect to any of them, its own Claude's included. What a holder of a messaging token could do over such a socket wasn't tested, so no exception is made.
  - writes to **programs that run outside any sandbox**: `~/.local/bin/**` (it holds `codex`, `agy` and `agent-os` on this Mac), every folder on the captured `PATH` that the user can write (e.g. `/opt/homebrew/bin`, which holds `claude`), checked at every session start, and each runtime's recorded real path. **The folders those entries resolve into** are denied too (rev 3.3): a `PATH` symlink's target folder, checked at every start, and, when the Homebrew prefix (`brew --prefix`, e.g. `/opt/homebrew`) is user-writable, as it is on Apple Silicon, the whole prefix (`Cellar/`, `Caskroom/` and the rest). A replaced `agent-os` or runtime binary would run inside a victim agent's process tree, pass the ancestry check and get its token, so none of them may be written.
  - **reads of the user's Keychain files** (`~/Library/Keychains/**`) by the agent's tools, so a tool can't read the runtimes' sign-ins (Claude's `Claude Code-credentials` item) or anything else of the user's there. A Keychain item can also be read through the system's security service rather than the file, so this deny alone may not hold: S1 tests `security find-generic-password -s "Claude Code-credentials" -w` from a tool, and Claude isn't office-run until it fails without asking the user anything (see "S0 checks and fallbacks").
- **Known limit, Claude take-over** (rev 3.3): in a take-over, `claude --resume` runs as the interactive TUI, and its permission prompts go to the person, not to `can_use_tool`. So the office's own write check (decision 7) doesn't run, and the person can switch to `acceptEdits` (or `auto`), which allows edits, through a symlink an agent planted too, without a prompt. The fixed and named denies still apply as Claude's deny rules. Whether those rules match a symlink's real path is isolated by an S1 check ("S0 checks and fallbacks"); if they match only literal paths, the take-over warning says so. A person is at the keyboard, so we accept it, and the app says so when a person takes over a Claude agent. S1 checks whether a Claude settings key can lock `acceptEdits` and `auto` out of the TUI; if one can, the office's settings set it.
- **Known limit, a symlink swapped after an answer** (rev 3.3): the office checks a file write's real path when it answers the request, and Claude's unsandboxed CLI writes afterwards. A detached tool process can replace a workspace path with a symlink in that window, and the write then lands wherever the symlink points, unless one of Claude's own deny rules matches it. S1 checks whether Claude's `Edit` and `Write` re-check deny rules at write time or refuse to follow symlinks. If they refuse to follow symlinks, this closes. If they only re-check deny rules at write time (and those rules match real paths, the S1 run under "S0 checks and fallbacks"), the gap narrows to targets no deny rule names. Real isolation (an OS sandbox around the CLI) is v2.
- **Known limit, Codex take-over:** a person in Codex's TUI can switch its sandbox or approval policy from inside (`/approvals`), which also lifts the fixed denies for that take-over. A person is at the keyboard, so we accept it, the office pins both again when it resumes headless, and the app says so when a person takes over a Codex agent. S0 tries Codex's managed requirements (`allowed_sandbox_modes`, `allowed_approval_policies`) to lock `danger-full-access` out.
- **Antigravity** is gated: S4 doesn't ship until the spike proves that agy's runtime dir and `.agents/` (the agent file, `hooks.json` and rules, including those that arrive through a git checkout) can't be written by the agent, or aren't loaded. Otherwise Antigravity ships with that as a documented, accepted limit, and the app tells the owner when they enable an Antigravity agent. That limit covers only `.agents/` content that arrives in the workspace, never the fixed denies: Antigravity doesn't ship until its sandbox holds the runtime-dir denies and the read denies, like every runtime.
- **Secrets stay out of argv, settings files, logs and the event log.** Before it stores or streams any event, and before it writes stderr to the log, the office redacts:
  - the live session tokens and `app_token`s it knows (an `env` call would print them)
  - **`CLAUDE_CODE_MESSAGING_TOKEN`**, which Claude puts into every tool's environment. The office isn't given its value, so it redacts it by name: the value after `CLAUDE_CODE_MESSAGING_TOKEN=`. Each adapter lists its runtime's secret variable names for this. **Known limit:** a bare value (`printenv CLAUDE_CODE_MESSAGING_TOKEN`) isn't caught. Learning the value from a tool's environment, as the mark scan reads it, would close that later. Tools can't reach the socket it opens anyway (the `/tmp/cc-socks` deny above).
  - token patterns (GitHub, Anthropic, OpenAI, Google keys)

  Tool output in the runtime's own transcript (for example `agent-os gh-token`) is covered by the plaintext-transcript decision (U-S3).
- **Known limit, one store with the user:** an office-run Claude agent's conversations live in the user's own `~/.claude`. If the user runs `claude --continue` or `/resume` in an agent's home or worktree, they can pick up the office-run conversation, and there are two writers on one transcript. The owner is told to use take-over instead.
- **What the session token stops:** casual and accidental impersonation between agents, and, with the ancestry check, a token copied to another process. It doesn't stop a hostile process running as the same user, which can read another process's environment on macOS and could start a child of the agent's own process. Real isolation (a separate user or an OS sandbox) is v2, as in ADR-0003 and `projects-git-github.md`.

### Standalone-v1 managed state and grants (rev 3.6, temp rows rev 3.7)

The launch plan resolves the full configured paths from the trusted instance
registry and selected release. **Managed state** includes the complete instance
root, every registered peer instance root (including custom locations), the
registry itself, platform/file credential locations, release payloads and private
runtime directories. Decision 7's write and absolute-allow-rule checks cover that
whole set, not only `office-state/`. Resolve aliases before building grants; neither
an employee-supplied path nor a changed environment variable expands a grant.

The existing office-state table below is supplemented by these concrete grants.
"Denied" covers reads/search results and writes, for both shell and file tools.
Protected configuration paths are enumerated from setup's actual resolved files;
configuration, credentials or private data cannot be provisioned at an unclassified
managed path. Never deny the whole selected instance or its shared `agents/` parent:
those contain legitimate writable roots, and no nested allow may cancel a deny.

| Resolved path | Employee tools |
| --- | --- |
| Instance configuration files, root lock, setup journal/staging, credential files, private logs and backups, each at its actual configured path | Denied, including paths outside `office-state/`; none may overlap a granted workspace or temporary directory |
| Instance registry and its lock | Denied; `agent-os` receives its identity and endpoint from launch, not registry discovery |
| `<instance root>/agents/<other office>/<member>/` and `<instance root>/agents/<own office>/<other member>/`, enumerated per private home | Denied; no access to another employee's persona sources, specialists or memory |
| `<instance root>/agents/<own office>/<own member>/` | Home session: read/write except protected generated files. Task session: read-only. Internal jobs receive only their purpose-specific subset; checkpoint helpers receive the supplied snapshot only |
| Each other registered instance's root, private runtime directory, credential paths and temp folder (`<user temp>/ao/<its handle>/`, from the registry) | Denied, using canonical paths, never a wildcard over the parent containing this instance |
| Under the own instance's temp folder: every other office's folder; and every folder under the own office's except the session's own member folder and own jobs folder (so other employees', other employees' jobs and the runs folder are denied). An internal job keeps its own job folder and denies its employee's member folder and the sibling job folders that exist at its launch; a one-off keeps its own run folder and denies the office's member and jobs folders and the sibling run folders that exist at its launch. Every other folder directly under `<user temp>/ao/` (leftovers of unregistered instances) is denied too (rev 3.7) | Denied: other employees', internal jobs', one-offs', other offices' and other instances' `TMPDIR`s (with their `GH_CONFIG_DIR`). Built at launch from the folders that exist, which the re-creation rule above guarantees for every registered instance and the whole roster, so leftovers of crashed sessions and offboarded members are denied too. Canonical paths after resolving the platform's temp symlinks (`/var/folders` on macOS). Never a deny on `<user temp>/ao/`, the own instance's or the own office's folder, which are ancestors of this session's `TMPDIR`. The employee's own sessions share its member folder, like its home; the main session can read its own jobs folder, a job can't read the member folder, and a job or one-off started later than a sibling isn't denied to it (an accepted residual, stated under `TMPDIR`) |
| The web process's log and temp directories (X-02 §2; a draft, see the launch v3 note in office-host-boundary.md §3), under the instance's private state, never under `<user temp>/ao/` | Denied |
| The selected instance's private runtime directory | Denied; only connection to the exact `ORG_OFFICE_SOCKET` endpoint is granted separately. No host/sibling office socket access or directory mutation |
| Pinned release directory, helper executable and installed runtime executable paths | Read/execute as needed; fixed write deny. Runtime sign-in stores retain their separate fixed read denies |

The matching `agent-os` helper uses the launch-supplied endpoint and verifies the
server/session binding; knowing or modifying an endpoint string grants no rights.
Each harness must prove that its exact socket grant works while the runtime
directory's file/search denies hold. Otherwise it is unavailable for office-run
sessions until a reviewed equivalent is verified; do not drop a deny to make it run.

Before newly provisioned private homes (and their temp parents), newly created offices (their temp folders, before the office's worker starts) or peer instances become accessible, setup,
the host and the owning office must refresh affected active runtime policies or stop/relaunch those
runtimes through their normal lifecycle. If a harness cannot apply that safely,
defer the provisioning change with an explicit reason. For a hire, the owning office refreshes or relaunches its own affected sessions at a safe point. Changes made by setup or the host (a new office or instance) need a host → worker mechanism that's still open (X-02 Q9's `layout_changed`; a draft, see the launch v3 note in office-host-boundary.md §3); until it exists, those changes are deferred while affected sessions run, a product effect for the specs (follow-up with Eden). Recheck the registry and
resolved grants at every launch; a previous launch is not evidence for a changed
layout. Same-user OS isolation limits remain as documented in the server design.

Validation must exercise reads, searches, writes, symlink aliases and attempted
approval overrides against every path category above; it must also prove that the
employee's allowed home/task work, its exact office connection and pinned helper
still function. Include provisioning a second instance/private home while a runtime
is active, and reading, listing and writing another employee's and another instance's
session `TMPDIR` (refused), a one-off's run folder from an employee session and back (refused), an internal job's folder from its main session (allowed) and the member folder from the job (refused), all of these again after a reboot or a wiped temp tree with the other instance's host started second, a one-off in an organization created while a session is active (refused from that session), an earlier sibling's job folder from a job and an earlier one-off's run folder from a one-off (refused), and binding a Unix socket at `TMPDIR` plus a 32-byte suffix (including its separator) with `TMPDIR` at the budget; creating an entry directly in `<user temp>/ao/`, the own instance's or the own office's folder from an agent session (refused: these ancestors carry no deny, so no runtime may grant the platform temp directory beyond `TMPDIR`; Codex's workspace-write adds `$TMPDIR` and `/tmp` by default, an S0 item); a deny target deleted and re-created while a session runs (still denied; on Linux bwrap's mount-based denies, an S0 item) (rev 3.7). No sandbox grant may expose another employee's or instance's private data.

### What agents can reach under `<office home>`
Rev 3.3 replaces the blanket deny on `<office home>/**`, which covered every task worktree and `work.git` (they're under `<office dir>/workspace/`). It's a **deny list of named paths**, never a deny with an exception: a deny always wins, so a deny on a folder that holds a writable root would make the root unwritable, or, if a nested allow won, would stop being a fixed deny. **No entry is an ancestor of a writable root**, the spec's denies and the start denies alike. The office checks that when it builds the lists, and refuses the start otherwise (the agent goes to Needs you).

Paths are relative to `<office home>`. "Denied" means reads and writes, from Bash (the sandbox) and from the file tools (the permission policy), for every runtime.

| Path | Agents' tools | Why |
| --- | --- | --- |
| `host.lock` | denied | the host's state lock (`office-host-boundary.md`); host and office sockets are protected at their actual runtime paths by the standalone table above |
| legacy `host.sock`, `run/**`, if present | denied, and no connection | these are no longer launch endpoints; `agent-os` receives only its exact authenticated office endpoint through `ORG_OFFICE_SOCKET` |
| `logs/**` | denied | the host's logs |
| legacy `bin/**`, if present | read only | never selected for launch; the pinned release helper path is protected by the standalone table above |
| `offices/.refused/**` | denied | quarantined offices |
| `offices/<other office id>/**`, each one named at start | denied | every other office on this Mac, other organizations' included: their worktrees, mirrors and composed instructions. An office created during a session is covered from the session's next start; it holds nothing of an agent's until one runs there |
| `offices/*/office.lock` | denied | the server's lock |
| `offices/*/office.json` | denied | `agent-os` doesn't read it when `ORG_OFFICE` is set, as it always is in office-run sessions (ADR-0003 §2.1) |
| `offices/*/org.db`, `offices/*/org.db-wal`, `offices/*/org.db-shm`, `offices/*/org.db-journal`, `offices/*/office.db`, `offices/*/office.db-wal`, `offices/*/office.db-shm`, `offices/*/office.db-journal` | denied | the databases and their SQLite side files, each one named (rev 3.4: a path pattern accepts `*` only as a whole segment, so `org.db*` can't be written) |
| `offices/*/spool/**` | denied | office-run agents never spool (`agent-os` exits `5`) |
| `offices/*/logs/**` | denied | agents' stderr logs |
| `offices/*/agents/**` | denied, the agent's own runtime dir included | settings, MCP config, composed instructions. Only the runtime itself reads its files, outside its sandbox and tool permissions (decision 6) |
| `offices/*/workspace/projects/*/mirror.git/**` | denied | the office's mirror, the only repository that talks to GitHub |
| `offices/<this office>/workspace/projects/<project id>/work.git/` | read; writes only to the literal commit paths, minus the `denyWrite` list | "Git in worktrees" |
| `offices/<this office>/workspace/projects/<project id>/worktrees/<this task>/` | read and write: the session's cwd and writable root | "Git in worktrees" |
| other tasks' worktrees, each one named at start | read only (a write deny) | a write could tamper with another agent's uncommitted work. One made during the session isn't a writable root and no allow rule can name it (decision 7), so it's covered too |
| this office's other projects' `work.git/` and `worktrees/`, each one named at start | read only (a write deny) | another project's shared git and agents' work |
| anything else under `<office home>` | read only | it's never a writable root, no allow rule can name it, and the office denies any write request there, even one allowed once (decision 7). The office checks at answer time; see the known limit on a symlink swapped after an answer |

A home session has no writable root inside `<office home>` at all, since the home must be outside it. A one-off without a project works in the `ws/` folder of its run temp folder (rev 3.7).

## Contract (org-proto, additive)

### Legacy v1 contract and proposed standalone v2 mapping

These exact earlier wire sketches retain security/compatibility obligations.
They are not new public commands or proof that handlers are implemented. Existing
code and negotiated transport versions remain the current contract. Target
RunTurn/ResumeTurn/CancelTurn/StopAgent and connection operations are internal
execution-service contracts in document 11, not automatically aliases for
agent.wake/run.one_off or this proposed API_VERSION 2. Adapt them with actor-bound
operation receipts, expected config/placement/turn revisions, immutable input
references and explicit unknown-outcome recovery. Keep legacy session-token and
peer-ancestry enforcement until a reviewed versioned mapping replaces it.

The additive sketches below apply within their negotiated protocol version. ADR-0001 defines the first standalone API_VERSION 2 transition for retired fields/calls and changed authorization/control semantics; the inspected source still declares version 1. HTTP `/api/v1` and the private launch protocol have independent versions.
**Authorization (standalone v1):** the human controls below require an authenticated, authorized principal through HQ; the browser receives no `app_token`. HQ preserves the actor, scope and operation identity when routing to the owning office. Agent-bound connections are refused unless a specific reviewed contract grants a narrowly scoped operation; an office certificate grants no human powers. ADR-0003 §2.1 and its standalone amendment retain per-call role and owner checks. Tests must refuse unauthorized browser, agent and relay callers. **Org commands** (`member.*`, `team.*`, `project.*` and the rest) from agents remain refused unless a row in that authorization table allows one.
- **Sessions:** where a call takes `session`, it's `"home"` or `{ "task_id": "…" }`.
- **Agents:**
  - `agent.wake { member_id, session?, prompt? }` → `{ run_id }` (the live run's id when the wake is merged). `session` defaults to `home`.
  - `agent.stop { member_id }`: Pause via the durable HQ status command and separate office stop acknowledgement (see "Run modes"); the HTTP envelope supplies operation ID and expected member version. This is part of the API_VERSION 2 standalone transition, not an unversioned reinterpretation of a v1 live RPC.
  - `agent.interrupt { member_id }`
  - `agent.send { member_id, session?, text, urgent? }`: the owner talks to an agent directly. `session` defaults to `home`.
  - `permission_policy.get { member_id }`, `permission_policy.set { member_id, policy, expected_version }`. `set` refuses an allow rule for a tool that always escalates (`invalid_request`, reason `escalating_tool`) and one whose absolute path pattern reaches managed state (reason `managed_state`); see decision 7.
  - `member.set_run_mode { member_id, run_mode?, schedule?, expected_version }`: an org command at HQ, like the other `member.*` commands. It's on `v1` already, with `AgentProfile.run_mode` and `schedule`. It's listed here for its authorization row; HQ's handler is plan step S2c.
- **One-offs:**
  - `run.one_off { role, title?, runtime, model?, instruction, project_id?, work_item? }` → `{ run_id }`
  - `run.stop { run_id }`: stops a one-off (a checkpoint turn, then stop; the run ends `stopped`)
  - `run.interrupt { run_id }`
  - `run.send { run_id, text, urgent? }`
  - These take only a live one-off's `run_id`. An agent's run is `invalid_request` (use the `agent.*` calls), and an ended run is `not_found`.
- **Messaging:** `msg.bind` gains `session_token?`. It's required when binding as an office-run agent, and on its own (with no `agent`) it binds a one-off run. Peer ancestry is checked in both cases. A missing or invalid token, or a failed ancestry check, is refused with `not_authorized`. This is the one change open to agents.
- **Runtimes:**
  - `runtime.status { runtime? }` → per runtime: installed, path, version, in tested range, signed in, quota windows
- **Runs:**
  - `run.list { member_id?, since?, cursor?, limit? }` → `{ runs, next_cursor }` (one-off runs included)
  - `run.get { run_id }`
  - `run.events { run_id, from_seq }`, then live as `run.event` through `subscribe`
- **Approvals:**
  - `approval.list`
  - `approval.answer { approval_id, decision: allow_once | allow_always | deny, scope?, note? }`
    - `scope` is the rule's path pattern, command prefix or MCP server. `allow_always` adds that rule to the agent's permission policy, as a versioned change. The fixed denies still win.
    - On an approval whose `escalation` isn't `none`, `allow_always` and `scope` are refused with `invalid_request` (reason `escalated`). An absolute path `scope` that could reach managed state is refused (reason `managed_state`), using decision 7's full normalized overlap check; workspace-relative scopes remain subject to their real-path and fixed-deny checks.
    - An unanswered approval expires after **30 minutes** (configurable) and counts as a deny.
    - If its session died, or it expired or was cancelled, answering returns `not_found`.
- **Take-over:**
  - *Not v1 (ADR-0004).* The calls below belong to the earlier app-attached terminal and are not part of the v1 contract.
  - `session.take_over { member_id, session?, cols, rows }` → `{ lease_id }`. `session` defaults to the live session, or `home` when nothing is live. The connection that makes this call holds the lease (see "Taking over").
  - `session.input { lease_id, data }` and `session.resize { lease_id, cols, rows }`: only from the lease holder's connection (`held` from another app connection)
  - `session.heartbeat { lease_id }`, every 10 s
  - `session.hand_back { lease_id }`
  - The event `session.output { lease_id, seq, data }` (at most 64 KiB of terminal data, base64) goes only to the lease holder's connection.
- **Events:**
  - `agent.state_changed`
  - `run.started`, `run.finished`, `run.event`
  - `approval.requested` (with the structured input, the justification labeled untrusted, and `escalation`), `approval.answered`
  - `member.presence_changed` with the amended presence values
  - **`run.*`, `approval.*` and `agent.state_changed` go only to app-bound subscribers.** `subscribe` on an agent-bound connection never receives them, and a test proves it (plan step S2d).
- **Errors:**
  - `unavailable` with a reason: `not_installed`, `signed_out`, `version`, `quota` or `paused`
  - `invalid_request` with the reason `offboarded` (see "Offboard and remove"), `escalated`, `escalating_tool` or `managed_state` (see decision 7)
  - a **new code, `held`**: the session is held by someone else (a take-over lease). `conflict` keeps its meaning, a stale `expected_version`.
  - `not_found`, `not_authorized`, plus the existing codes
  - So `ApiError` gains an optional `reason`, with `#[serde(default, skip_serializing_if = "Option::is_none")]`, so older peers still read it. `ErrorCode` gains `Held`. It isn't in `error.rs` yet (S2 adds it), and it's additive: `ErrorCode` is `#[non_exhaustive]` and decodes unknown codes as `Unknown` (`#[serde(other)]`).
- New enums (session state, run outcome, presence, reasons, error codes) decode unknown values as `Unknown`.

## ADR-0003 amendments
Made in ADR-0003, each marked "(amended 2026-11-08: agent sessions, ada)", "(amended 2026-11-08: agent sessions rev 3, ada)", "(amended 2026-11-08: agent sessions rev 3.1, ada)", "(amended 2026-11-08: agent sessions rev 3.2, ada)" or "(amended 2026-11-08: agent sessions rev 3.3, ada)":
- **M-A:** the session token is issued per run (one process's life) for office-run agents and required on `msg.bind`, together with peer ancestry. A one-off binds with its run's token and the same ancestry check, and its run address (`run:<office id>/<run id>`) is accepted in `from`/`to`, the authorization rows and routing. HQ refuses a run address in `from` from any other office. A message to an ended run is dead-lettered with `run_ended`, which `msg.ask` reports as `unknown_recipient` (exit 3). Only the people of a one-off's job and the office's notices may message it (else `unknown_recipient`), and HQ refuses handles (people's and agents') that start with `run:` or are ULID-shaped. Spool ingest accepts only a `from` that's an active agent run by hand, so M-A is closed for office-run agents and spool forgery of people is closed for everyone. Binding fails closed: a token is required for any `run_mode`, and an offboarded or `Unknown`-status agent can't bind.
- **M-B:** the new calls are app-bound only, `run.stop`, `run.interrupt`, `run.send`, `session.input` and `session.resize` included. Org commands from agent-bound connections are refused unless a row allows them, and `member.set_run_mode` is app-only. `subscribe` on an agent-bound connection never gets `run.*`, `approval.*` or `agent.state_changed`. The original `msg.bind` row says that a missing session token is `not_authorized`, and binding as a person without `app_token` is `not_authorized` too (it was `invalid_request`), so the two match.
- **Presence** (§2.4, with the enums in §4.1 and §4.2): `busy`, `idle`, `needs_you`, `with_you`, `asleep` (derived) and `offline`; `online` stays for people; a held wait counts as idle; waits on token-bound connections are capped at 60 s by the server.
- **Handles** (§2.4, §2.6, §4.1, §4.3): a handle names at most one member, display names aren't accepted as addresses, `AmbiguousName` is gone, directory cards carry `handle`, and parameters are `id_or_handle`. `messaging_name` is on `v1` (proto-agents, 86a6a9f).
  - Rev 3.2, **`--to` resolution:** a well-formed run address first, then the reserved `owner`, then an exact member id, then a handle (ignoring case). An exact id match beats a handle. `owner` and `office` are reserved handles. A handle that starts with `run:` or is ULID-shaped is refused with `invalid_name`, for people and agents. **The owner's display name isn't a handle:** it's exempt from the namespace (a person may share it), and the owner is addressed only as `owner`. Display names never resolve. This follows `teams-and-org-chart.md` as amended on `ada/docs-followups-3`. Server-dev's `proto-wording` has the same rules, except that its ULID shape leaves out I, L, O and U; `proto-wording` is to be changed to count them too (a follow-up for server-dev).
- **Exit codes** (§2.6): rows for `not_authorized`, `unavailable`, `conflict` and `held`.
- **Contract** (§4.1): the agent-session calls are app-only, `msg.bind` gains `session_token`, the `SessionAuth` port (`requires_token` returns a `Result`, so it fails closed).
- **Asks to people** (§2.5): on a token-bound connection there's no 1-hour re-ask; `agent-os msg ask` exits `2` with `"pending": true` after one 60-second hold.
- **Owners** (§4.3): Remy (`ac-software-engineer-remy`) owns `org-mailbox`, `org-spool` and `agent-os`, in place of `agy-senior-dev`.
- Rev 3.3, **messaging goes only through the office** (§2.1.4, M-A): office-run Claude sessions don't get Claude's own cross-session tools (`SendMessage`, `ListAgents`, `PushNotification`) or its trigger and scheduling tools, so an office-run agent can't reach another Claude session around ADR-0003, its session token or a one-off's "who may message" rule. §2.1.3's note on `ada/docs-followups-2` now says that branch is merged into `v1`.

## Deferred to phase 2

Automatic scheduling/wakes, memory extraction and native delegate orchestration
remain unselected; a data field does not select an engine. Browser takeover,
production authenticated control composition, cross-office execution admission
and budget evaluators require their documented implementations. Accounting and
budget/recovery data are selected in documents 10/16/17 and must not be labeled
future optional persistence. Their automatic services are still unimplemented.

## Where the code lives

| Component | Existing boundary and selected integration work | Owner |
| --- | --- | --- |
| `crates/org-runtime` | Existing synchronous sans-I/O adapter contract, validated spec/paths, events and framing. Target routing/checkpoint adaptation requires conformance; shared RuntimeHost topology remains deferred. | ada (contract), River |
| `crates/org-supervise` | Implemented piped spawn, identity ordering, stop/reap, private file/probe preparation and native recovery API. No SQL/router/scheduler; PTYs and automatic recovery integration remain separate work. | River |
| `crates/org-runtime-claude` (planned; absent) | Proposed Claude protocol adapter; the crate does not exist in this checkout. Separate research/spike fixtures do not establish an implemented adapter. Real launch eligibility requires every isolation/auth/policy/usage proof above. | River |
| `crates/org-runtime-codex`, `crates/org-runtime-agy` (planned; absent) | Proposed native adapters; neither crate exists in this checkout. Separate runtime research is capability evidence only. Installed-version conformance gates support; no assumed Antigravity registration format. | codex-dev |
| `org-office` | Domain admission/turn/attempt/config and policy rules through focused ports; no dependency on runtime adapters or org-supervise. No selected automatic queues/memory/delegates. | server-dev |
| `org-office-sessions` (planned integration) | Map admitted immutable input/routes to SessionSpec/LaunchContext, pump pipes/ticks/events and invoke OS control. Preserve token ancestry/security. It runs inside the office worker, not another daemon. | server-dev, River (OS identity) |
| `agentic-org-office` | Thin composition of supervisor/router/session runner and the one writer/recovery owner. HQ additionally owns shared permits/cooldowns and placement authority. Full runner/coordinator remains implementation work. | server-dev |
| `org-host` | Host lifecycle, key custody and worker recovery ordering. Never race a live worker's agent ownership or treat in-memory launch inventory as durable placement authority. | River |
| `crates/org-office-launch` | Existing signed/private launch contract; new child/control/version fields require reviewed compatibility changes. | ada (contract), River |
| `org-git` | Hardened repository/cache/worktree operations. Target organization registrations and task paths require adapted per-runtime grants; preserve unpushed work through handoff. | River |
| `org-mailbox`, `agent-os` | Current messaging/directory transport and agent authentication; preserve exact peer/token/spool admission. One-off extensions stay proposed until wired. | Remy |
| `apps/agentic-org-ui` | Target explicit turn/readiness/attempt/approval and desired-versus-observed stop views with cost scope. Automatic mode/delegate/memory and browser takeover views remain deferred. | app-frontend-dev |
| `org-proto` | Existing negotiated wire contract; internal execution service shapes are not public aliases until versioned adaptation is implemented. | ada |

Dependency direction remains office composition → integration → domain/adapter/OS
ports, and adapters → org-runtime. Domain code must not depend back on its
integration or adapters. Recalculate distinct direct dependencies at the chosen
crate/module level when implementing; the earlier proposal's Ce≈8/Ce≈9 estimates
are not evidence of a completed extraction or a current dependency graph. Apply
architecture-rules.md's warning/review thresholds to the actual change.

## Plan

Retained step IDs are integration/security work, not proof of completion. S0–S7
must add document 11 AE-01..18, document 16 durability/fencing and document 17
independent-hold evidence. Scheduling/delegate/memory/takeover steps are deferred
individually by scope; do not enable their native tools as a prerequisite for
explicit turns. Exact legacy path tests must be rerun against the target layout.

| Step | What | Who | Depends on |
| --- | --- | --- | --- |
| S0 | Spikes, one per runtime, plus every check under "S0 checks and fallbacks" below:<br>• **Claude: DONE** (River, branch `river/spike-claude`, `spikes/claude/README.md`, Claude Code 2.1.311, 40 recorded fixtures). Key results: approvals round-trip with `--permission-prompt-tool stdio`, with no CLI timeout within 150 s; interrupt is in-band and the process lives on; resume works from any cwd, snapshots the system prompt and doesn't restore `--settings`; `--setting-sources ""` loads no settings or instruction file, and `autoMemoryEnabled: false` turns auto-memory off; a workspace skill's `allowed-tools` pre-approves tools only when the project source loads; `allowUnixSockets` admits exactly the listed socket; `git commit` works in the sandbox, with `denyWrite` entries for the parts of `work.git` it opens by itself; Bash tools run in their own session and process group; an unknown `--agent` falls back silently, so identity is checked through `initialize`; an interrupted turn's `result` reports zero usage; take-over keeps the conversation both ways; about 210-240 MiB per live process. Still open for Claude (S1 runs them; the load-bearing ones have fail-closed fallbacks under "S0 checks and fallbacks"): SIGINT to the CLI; the office's mark scan run on Claude's tools; a nested `CLAUDE.md` loaded lazily; background work after a `result`, with `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1`; isolation with the real `~/.claude` (synced plugins, which the spike saw kept out only by hand and in an isolated dir; `--strict-mcp-config`, which kept connectors out in an isolated dir, with controls; the real `~/.claude.json` is untested); the read deny on `~/.claude/**`, for Bash and for the file tools; the tool allowlist (a non-empty `--tools` list ran only with `--restricted`; `--tools ""` alone removed every built-in tool but not MCP tools; the inline agent's `tools` restricted the main thread on their own; `--disallowedTools` wasn't tested); a task commit at its real path under `<office home>` with the full deny table; the shell snapshot and the user's rc files; symlinked spellings of sandbox paths (`/tmp`, the `TMPDIR` under `/var/folders`); narrowing refs to the agent's own branch; an approval left unanswered for 31 minutes or more; whether a tool can read the Claude sign-in from the Keychain (`security find-generic-password`), and that the read deny on `~/.claude.json` holds; the trust dialog for a folder the user never opened; many concurrent processes (the memory figure counts the CLI only, not its tools).<br>• **Codex** (codex-dev, in progress on `codex/spike-codex`, checkpoint 6647d75, Codex 0.167.0: offline protocol and sandbox probes, no model turn yet). Found so far: the legacy writable roots are `turn/start.sandboxPolicy.workspaceWrite.writableRoots`, with no read deny; a named permissions profile (one `-c` value for the whole table, plus `features.network_proxy=true`) admits exactly the office socket, refuses other sockets and direct TCP, and can deny a file's reads and writes; the legacy policy and a profile aren't combined; git must be the binary `xcrun --find git` resolves. Still open: the app-server over stdio, one per session, in live turns; the profile's full git and office deny lists, and the profile in live turns; what leaks from the user's Codex home and how `-c` / `--disable` turn hooks and MCP off; `approvalsReviewer`; approvals; interrupt; `thread/unsubscribe`; `codex resume` in a PTY with the same pins; the office socket and `git commit` from a model's tool shell under the profile; that a write through a symlink planted in the workspace, pointing outside the writable roots or into a denied path, is refused by the sandbox at its real path; the commit from a worktree at its real path under `<office home>`; Codex's built-in tools, for its allowlist; whether managed requirements (`allowed_sandbox_modes`, `allowed_approval_policies`) lock `danger-full-access` out of the TUI; tool detaching; `shell_environment_policy` keeps `ORG_SESSION_TOKEN` in tool shells; optionally `--ws-auth capability-token` for live joining.<br>• **Antigravity:** ACP versus stream-json; interrupt; resume; `request_permission`; a custom agent from `--add-dir`; sign-in with `--gemini-dir`, `--include-default-customizations=false` and `--app-data-dir`; whether its runtime dir and `.agents/` can be written or are loaded; whether agy reads its agent file through `--add-dir <runtime dir>` while its tools are denied that folder (the `offices/*/agents/**` deny), and whether its own store, `--gemini-dir <runtime dir>/gemini`, still works under the deny if its `--sandbox` wraps the whole CLI (else both move: the agent file into the workspace as decision 6 says, and the store to a per-agent folder outside `<office home>`, as S0 decides; Antigravity doesn't ship until it's settled); take-over without `~/.gemini`; `--sandbox` scope, the office socket and `git commit`; whether it filters its tools' environment; tool detaching. | River (Claude, done), codex-dev (Codex, Antigravity) | — |
| S1 | `org-runtime` (first version), `org-supervise` and the Claude adapter, with fixture tests and a fake runtime. With the real Claude it runs the open Claude checks above, including: a user line whose body starts with `/` runs no slash command (rev 3.4); `init.tools` holds no disallowed tool under `--tools` and `--disallowedTools`, `init.plugins` only built-ins and `mcp_servers` only the office's, and with `ENABLE_TOOL_SEARCH=false` the office's MCP tools appear in `init.tools` and can be called | River | S0 Claude |
| S2 | Contract in org-proto, and the ADR-0003 amendments | ada | S0 Claude |
| S2b | The session token: `SessionAuth` in the messaging library, the token and ancestry on `msg.bind`, one-off binding and its allowed recipients, closing bound connections, capped waits, no spool for office-run agents, and spool ingest that accepts only active hand-run agents | Remy (the messaging owner) | S2 |
| S2c | Deferred scheduler handler for legacy `member.set_run_mode`; do not create a target schedule schema. Retain current handle/security rules: `run:` and ULID-shaped handles refused for people and agents (`invalid_name`), `owner` and `office` reserved, the owner's display name exempt | server-dev | S2 |
| S2d | The `subscribe` filter: agent-session events only to app-bound subscribers, with its test | server-dev | S2 |
| S2e | The price table for Codex and Antigravity (API prices in US dollars) | codex-dev, alongside phase-1 plan item W14 (costs, time and insights) | S2 |
| S3a | Office explicit turn/attempt/process runner, frozen input and receipts, binding/discovery readiness, safe checkpoint/recovery, independent holds, `org-office-sessions` and SessionAuth ancestry integration. Automatic trigger queues/switch scheduling remain deferred | server-dev, River (the process table and ancestry walk) | S1, S2, S2b, the HQ office server (server-dev's phase 1 storage + server) |
| S3b | Office approval lifecycle and HQ shared connection permits/cooldowns, coordinated with budget holds. Schedules and role-based one-off runner/control/messaging extensions remain deferred | server-dev | S3a |
| S3c | Adapt org-git workspaces to document 04 task/employee/repository paths; recompute grants/denies from retained work.git evidence, preserve handoff worktrees and verify any missing-workspace restoration | River (`org-git`), server-dev (the office side) | G2c, S3a |
| S3d | Single-owner host/worker recovery ordering, durable cleanup inventory and placement fencing before replacement; launch-pipe inventory alone is insufficient | River (ada reviews the `org-office-launch` change) | S1 |
| S3e | Deferred one-off extension: routing run addresses (`run:<office id>/<run id>`) through HQ: non-owning offices forward any well-formed one, HQ accepts them in synced messages, the owning office dead-letters late ones | River (`org-sync`), server-dev (HQ side) | S3b, `org-sync` |
| S3f | Not v1 (ADR-0004: no browser terminal; takeover is a local CLI operation). Earlier scope: the lease, the `session.*` handlers, the PTY spawn through `org-supervise`, and `session.output` / `session.input` / `session.resize` only on the lease holder's connection | server-dev, River (the PTY in `org-supervise`) | S1, S3a |
| S4 | The Codex and Antigravity adapters. Antigravity ships only with the S0 proof, or with the documented limit for workspace `.agents/` content and the owner told (see "Security"); the fixed denies are never a documented limit | codex-dev | S0, S1 |
| S5 | App explicit turn/attempt/readiness panel, approvals, independent holds and desired/observed Pause/Resume. Mode scheduling, one-offs and browser takeover remain deferred | app-frontend-dev | S2, S3a, S3b, S3f |
| S6 | **Security tests** (below) | River + server-dev, codex-dev for its runtimes | S2b, S2d, S3a, S3b, S3c, S3d, S3e, S3f, S4 |
| S7 | End to end: explicit turn, route-specific usage/receipts, office crash and safe explicit recovery in preserved workspace; no second effect/process. Verify each shipped driver; implicit message wakes remain deferred | River + server-dev | S6 |
| D1 | ADR-0002 note: runtime transcripts are plaintext outside `office.db` (U-S3) | ada | U-S3 |
| D2 | ADR-0004 rewritten to the headless-first decision | ada | U-S4 |
| D3 | AGENTS.md rows for `org-office-launch`'s children lines and the guidance files | ada | — |
| D4 | Deferred wake-guidance feature; any later instruction loading uses each project/repository association's required integration branch and verified cache | Eden (the text), server-dev (loading) | S3a |

**Security tests (S6):**
- A **hostile workspace**, like G2c's hostile git config: `.claude/settings.local.json` allowing everything, `.claude/skills` with `allowed-tools`, hooks, rules files, `.agents/hooks.json`, `.codex` config, `.git/hooks/*`. None of it may take effect, and writes to them are denied.
- A symlinked dotfile whose target lies inside a writable root (the workspace or `TMPDIR`) is still denied: a write to its real path, from Bash and from the file tools, is refused. A writable root whose last part is a symlink refuses the start.
- A **hostile `work.git`**: agent A can't write `work.git/config`, `config.worktree`, `hooks/` or any `worktrees/*/config.worktree`, in every runtime, and a hook or `config.worktree` that A plants never runs in agent B's commits. Writes to `work.git/HEAD`, `packed-refs`, `info/exclude`, `description`, `FETCH_HEAD`, `objects/info/` and the worktree's `gitdir` and `commondir` are denied, and the commit still works. `git config` and `git config --worktree` fail.
- **The office home** (rev 3.3): in a task session whose worktree is at its real path under `<office dir>/workspace/`, the agent edits files and commits in its worktree, from Bash and from the file tools, for every runtime. Every path the table under "Security" denies is refused as it says (reads and writes; `bin/` and other tasks' worktrees are read-only), from Bash and from the file tools, even with an `allow_always` rule: the databases and their `-wal` files, `office.json`, `office.lock`, `host.lock`, `host.sock`, `run/` (while `agent-os` still connects to its own socket), `logs/`, the spool, every agent's runtime dir (its own included), `mirror.git`, another office's folder, another task's worktree. `Grep` and `Glob` rooted at `<office home>` or `<office dir>` return nothing from a denied path. A write to another project's `work.git/config`, to a path given with `..` or `~`, or under an office created mid-session, is denied even when the owner allows it once, and a rule with `..` is refused. With `./**` allowed in the policy, `Edit` and `Write` through a symlink the agent planted in its worktree (pointing at another project's `work.git/refs/`, or at another task's worktree) are refused, and one pointing outside both the workspace and `<office home>` (`~/Documents/…`) goes to Needs you. A policy rule or `scope` whose absolute path pattern reaches into `<office home>`, directly or through a symlink, is refused with `managed_state`. A start whose spec denies or start denies hold an ancestor of a writable root is refused.
- **Claude's tools** (rev 3.3): `init.tools` holds only the allowlist, and none of `SendMessage`, `ListAgents`, `PushNotification`, `RemoteTrigger`, `CronCreate`, `CronDelete`, `CronList`, `ScheduleWakeup`, `Monitor`, `Task`, `TaskStop`, `Workflow`, `DesignSync`, `EnterWorktree`, `ExitWorktree`, `WebFetch` or `WebSearch`. The office's MCP tools are present, and `ToolSearch` is absent. A fake runtime whose `init` lists a disallowed tool fails the start, and on a later `init` the session is stopped at once: an `interrupt` goes out, every later `can_use_tool` is denied, the `Session` returns `Abort`, and the process is gone within the fast stop's 5 s. A `can_use_tool` for an unknown tool is `unknown` and goes to Needs you. No tool can connect to the user's own Claude session (a live interactive `claude` on the same Mac), and a Bash `run_in_background` call doesn't leave work running after the `result`.
- **Escalation** (rev 3.3): a Claude Bash request with `dangerouslyDisableSandbox: true` is denied as `sandbox_bypass`, under any policy; a Claude `ExitPlanMode` request is denied at once and recorded with escalation `permission_change` (it's disallowed), and a fake runtime's mode-switch request (ACP) goes to Needs you as `permission_change` even with an allow rule for it in the policy (which `permission_policy.set` refuses anyway); `allow_always` or `scope` on an escalated approval is `invalid_request`; `approval.requested` carries `escalation`; the runtime's own allow rules never name an escalating tool. The Claude adapter never sends `updatedInput` or `updatedPermissions`, whatever `permission_suggestions` a request carries.
- Writes to the user's config (`~/.codex/config.toml`, `~/.gemini`, `~/.claude`, `~/.claude.json`, `~/.agents`, `~/.gitconfig`, a shell rc file, `~/Library/LaunchAgents`) are denied, even with an `allow_always` rule. An agent whose home is `$HOME` isn't run.
- Writes to `~/.local/bin/`, to a user-writable `PATH` folder, to the folder a `PATH` symlink resolves into (Homebrew's `Cellar/` and `Caskroom/`, or the whole prefix when it's user-writable), to a runtime's real path and to `<release dir>/bin/` are denied. `command -v agent-os` in a tool resolves to `<release dir>/bin/agent-os`.
- `agent-os` binds from a tool shell in each runtime (Codex with the `shell_environment_policy` pins), so the token reaches it.
- A reply to an `ask` sent from a task session wakes that task session (or the home session once the task is closed). A switched-out task session runs again after the home session's run.
- The child's environment and open file descriptors hold no `app_token`, database key, GitHub token, `SSH_AUTH_SOCK` or user git and `gh` config. Argv holds no secrets.
- Every new call is refused on an agent-bound connection. `subscribe` on an agent-bound connection gets no `run.*`, `approval.*` or `agent.state_changed`.
- `msg.bind` as an office-run agent or a one-off is refused without a valid token, with another run's token, and from a process outside the session's tree. When a run's process exits, connections bound with its token are closed.
- **Spool ingest** dead-letters a spooled send whose `from` is `owner`, a person, `office`, a run address, an office-run agent (`Unknown` mode included), an offboarded, paused or `Unknown`-status agent, or an unknown id or handle, with a log line that holds no body. It ingests one from an active hand-run agent.
- `msg.bind` as an offboarded agent or an `Unknown`-status agent is refused, and when `requires_token` can't look the member up, the bind is refused.
- Orphans: stopping a session kills its tools, including tools in their own session (Claude's), and so does cleanup after the runtime was SIGKILLed; killing a session's group kills its grandchildren; a dead leader with a live grandchild is still cleaned up; a reused pid is never killed; a reused process group whose members don't carry the mark is never killed; a child that moved to its own session is found by its mark; cleanup never hits the session's next process.
- A one-off can't message anyone outside its job, and nobody outside its job can message it (`unknown_recipient`, from the same office and from another). Its approvals go to Needs you under an empty policy, its `allow_always` never changes the role's default, and a crashed one-off isn't re-run by itself. A person or agent with a `run:…` or ULID-shaped handle is refused, and so is the handle `office`. An exact id beats a handle, and the owner's display name never resolves.
- Offboarding an agent stops its session at once, closes its connections and voids its token, and `agent.wake` and `session.take_over` on it are refused.
- SIGKILL the office, restart it: no survivor, a single writer per conversation, and pending approvals expired.
- The lease expires after 30 s without a heartbeat. An approval that expires counts as a deny.
- Take-over frames: `session.output` reaches only the lease holder's connection. `session.input` from an agent-bound connection is `not_authorized`, and from another app connection `held`. Closing the lease holder's connection hands the agent back.
- A pause retry after a `conflict` never overrides a later Resume.
- A message body starting with `/` (for example `/config` or a skill's name) reaches Claude as text after the office header and runs no command; the encoder refuses a user line whose text starts with `/`.
- stdin lines with `\n`, U+2028 and U+2029 in user text stay one line. A stdout line over 4 MiB becomes a `truncated` event and the session goes on.
- Tool output with a token, a live session token, an `app_token` or `CLAUDE_CODE_MESSAGING_TOKEN=<value>` is redacted in the event log and the stderr log.
- A tool can't read `~/.claude/**`, `~/.claude.json` or `~/Library/Keychains/**`, directly or through a symlink planted in its workspace (`Read`, `Grep`, `Glob` and Bash), and `security find-generic-password -s "Claude Code-credentials" -w` from a tool fails without a prompt to the user. A tool can't connect to any `/tmp/cc-socks/` socket. After a SIGKILLed Claude, its `/tmp/cc-socks/<pid>.sock` is removed, and a live process's socket never is.
- A Claude start whose reserved `--agent` id isn't in `initialize`'s agent list, or is there with another description, fails. So does one whose `init.plugins` lists a plugin that isn't built in, or whose `init.mcp_servers` lists a server that isn't the office's.
- An approval nobody answers is expired by the office and answered as a deny. One the runtime withdraws (an interrupt) is `cancelled`, and so is one left pending when its session stops or crashes, or its turn ends. Answering either is `not_found`.

**S0 checks and fallbacks.** These weren't verified for rev 3. S0 checks each one, and the fallback applies if it fails. The Claude spike settled some Claude parts, as marked (rev 3.2). What it left open for Claude is checked in S1 with the real `~/.claude`, and every load-bearing point **fails closed**: Claude isn't office-run until it holds, and its agents stay run by hand (rev 3.3):
- **Tools see the session environment.** `agent-os` binds from a tool in each runtime: Claude's Bash, a Codex tool shell with the `shell_environment_policy` pins, and an Antigravity tool (does agy filter its tools' environment?). **Claude: verified** that the allowlisted variables reach the tool's environment; the bind itself is tested in S6. Fallback: a runtime whose tools can't see `ORG_SESSION_TOKEN` isn't office-run. The office refuses to run its agents (Needs you), and they stay run by hand. The token never goes another way (argv, a file).
- **Claude's `--setting-sources` value that loads no file. Resolved for an isolated config dir:** `""`, with `autoMemoryEnabled: false` in the office's settings (decision 6). S1 repeats it with the real `~/.claude`: no user `CLAUDE.md`, skill, agent or command loads, `init.plugins` holds only built-ins (no synced plugin), and `init.mcp_servers` only the office's (`--strict-mcp-config`). Fallback: Claude isn't office-run until it holds. A synced plugin's hooks would run unsandboxed in every agent, so this isn't a documented limit. A per-agent `CLAUDE_CONFIG_DIR` is later hardening, not a fallback: it needs the undocumented `CLAUDE_SECURESTORAGE_CONFIG_DIR` to keep the user's sign-in.
- **The read deny on `~/.claude/**` and `~/.claude.json`** with the real `~/.claude`: Bash (the sandbox) and the file tools (the policy's deny rule) can't read them, and Bash still works (the shell snapshot's fallback is under decision 6). **Searches too:** `Grep` and `Glob` rooted at an allowed parent (`~`, `<office home>`, `<office dir>`) return no names or content from any read-denied path (`~/.claude/projects/`, runtime dirs, the databases). They run in the CLI, so this depends on Claude applying deny rules to descendants. **And through symlinks:** through a symlink the agent planted in its workspace (with sandboxed Bash), `Read`, `Grep` and `Glob` into each read-denied path (`~/.claude/projects/`, `~/Library/Keychains/`, a runtime dir, a database, another office), and `Edit` and `Write` into each write-denied path, are refused, although Claude auto-allows reads inside the workspace. Fallback: Claude isn't office-run. A second run isolates Claude's own deny rules for symlinked writes (an approver that answers allow, or the TUI in `acceptEdits` or `auto`), for the Claude take-over limit under "Security".
- **The Claude sign-in in the Keychain:** a tool can't read it. `security find-generic-password -s "Claude Code-credentials" -w` from Bash fails, and the user sees no prompt; the file read deny on `~/Library/Keychains/**` is in place. Fallback: Claude isn't office-run until it holds.
- **The tool allowlist:** under `--tools` and `--disallowedTools`, `init.tools` holds only the allowlist, and a disallowed tool can't be called. In the spike, a non-empty `--tools` list ran only with `--restricted` (exactly the listed tools), `--tools ""` alone removed every built-in tool but not MCP tools, and the inline agent's `tools` restricted the main thread on their own; a non-empty list alone and `--disallowedTools` weren't tested. Fallback: the adapter's `init` check already stops any session with a tool outside the allowlist, so Claude isn't office-run until `--tools` holds.
- **The office's MCP tools under the allowlist:** with `ToolSearch` disallowed and `ENABLE_TOOL_SEARCH=false`, the office's `mcp__…` tools appear in `init.tools` (not deferred) and can be called. Fallback: a Claude agent that needs an office MCP tool isn't office-run until they can.
- **Background work after a `result`:** with the background tools off and `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1`, no tool keeps running and no turn starts after a `result` without office input. Fallback: Claude isn't office-run until it holds, because the idle timeout, the switch at a turn boundary and busy time all depend on it.
- **An approval left unanswered for 31 minutes or more** gets no CLI-side answer. Fallback: if the CLI times out first, the office's expiry is set below the CLI's limit, and a CLI-side timeout is handled as a withdrawal (`cancelled`). It fails closed either way: an unanswered approval is a deny.
- **A task commit at its real path under `<office home>`**, with the full deny table under "Security" and the rev 3.3 git environment, in each runtime. Fallback: a runtime isn't office-run for task sessions until it works. The denies are never widened to make it work.
- **Codex's permissions profile and a worktree's admin folder** (in `work.git/worktrees/<id>/`, outside the cwd): commits work with the listed roots, under the profile with the full deny lists, from a model's tool shell. S0 Codex's checkpoint committed with these roots under the legacy policy, from a standalone `command/exec`. Fallback for the profile: none; Codex isn't office-run until it holds (decision 6). Fallback for commits: the roots are never widened to all of `work.git`. Codex isn't office-run until commits work, and its agents stay run by hand.
- **Each sandbox can express "`objects/` except `objects/info/`".** Claude's can (a `denyWrite` on `objects/info/` holds). Codex's legacy policy can't (no subtractive deny); a profile may. Fallback: all of `objects/` is writable, and the agents' git environment adds `core.commitGraph=false`. At every session start and before every office git command, the office checks `objects/info/`: a `work.git` with `objects/info/alternates` is refused (Needs you), and commit-graph files are deleted.
- **Literal paths that don't exist yet** (a home's `.git/index.lock` and the like): Claude's sandbox allows them (verified). The fallback is under "Git in worktrees".
- **`KERN_PROCARGS2` on hardened children**: the mark scan can read their environment. Fallback: a process whose environment can't be read is never killed by the mark scan, and it's logged. Step 1 (the live leader's group) still applies. A tool that both leaves the group and can't be read is a documented limit. Claude's Bash tools always leave the group, so after a crash or a SIGKILL the mark scan is the only way to find them (a normal stop's SIGTERM to Claude kills them). The spike didn't run the office's scan on them, so S1 checks it.
- **User-level `~/.agents`**: whether Codex or Antigravity load rules, skills or hooks from it. Writes there are denied either way. Fallback: if they load, it's a documented limit like `~/.gemini`, and the owner is told when they enable that runtime.
- **`gh` with the session's own `GH_CONFIG_DIR`** (rev 3.3: an empty folder in the session's `TMPDIR`, removed when the session ends, since tools can't read the runtime dir): `gh` works through `agent-os gh-token`. It holds no login of the user's, and no other employee's session sees it, so no fallback is needed for safety.
- **Claude's sandbox deny lists accept wildcards** (rev 3.4): the `*` and `**` patterns this design writes (`offices/*/office.lock`, `offices/<other office id>/**` and the like) are honored by the sandbox for reads and writes, from Bash. The spike used only literal paths. Fallback: the office expands every pattern into literal paths at start (it knows every office and file), and the start fails if it can't.
- **Remote control off:** the setting or environment variable that keeps Claude's remote control off in an office-run session, and that `initialize` then shows it off. Fallback: Claude isn't office-run until it's found.

Tests at every step use a **fake runtime** (a test binary that speaks each recorded protocol), so CI never calls a model. The one smoke test per runtime is **opt-in**: it needs a secret and is off by default.

## Open questions for the user

These are retained earlier proposal questions, not a new approval request or
selected defaults. The 20% quota reserve, scheduling time limits, shared login and
memory behavior are superseded/deferred as described above; terms/privacy and
interactive capability evidence still require their own scoped decisions.

- **U-S1:** We'd run your own Claude, Codex and Antigravity logins for our own team. Before Agentic Org goes to anyone else, the vendors' terms need checking (the research found that Anthropic doesn't allow third-party products to offer claude.ai login without approval), and an API-key path per runtime is required. With an API key or a `claude setup-token` per organization, each Claude agent could also get its own `CLAUDE_CONFIG_DIR` (later hardening, decision 6). OK to proceed on that basis?
- **U-S2:** The defaults: up to **4** live agents per office at once; **30 minutes** of busy time per home-session run and **2 hours** per task-session run, with a 1-minute grace; a **10-minute** minimum slice before a normal trigger switches sessions; **20%** of each 5-hour quota window reserved for you; an approval expires after **30 minutes**; always-on agents restart after 10 s and stop after 3 restarts in 10 minutes. OK?
- **U-S3:** Each runtime keeps its transcripts as **plaintext** on disk in its own store (`office.db` stays encrypted). OK for phase 1?
- **U-S4:** The office drives every agent headless, and you work with one directly by taking it over (stop, then resume in a terminal the app shows). That replaces ADR-0004's plan (Claude's `--bg` with `claude attach`, from spike S2), and I'd rewrite ADR-0004 to match. OK? **Answered 2026-11-17:** headless, yes. No terminal in the app: chat is the interaction and the session view is read-only ([ADR-0004](../adr/0004-headless-agent-sessions.md)).
