# AGENTS.md: how agents work in this repository

## Prototype work

Before working under `apps/agentic-org-web-prototype/`, read and follow [the prototype instructions](apps/agentic-org-web-prototype/AGENTS.md).
They cover prototype verification, publishing, and owner-approved Svelte handoffs.
Compare approved snapshots for each release summary and checkpoint any release in
progress before context compaction.

## Knowledge & Backlog repository

Backlog tasks, designs, UX/AX specs, ADRs, and organization-data contracts live in the parent **`agentic-org-project`** repo under `backlog/` (`../../../../backlog/` from agent worktrees; `../../backlog/` from `repos/agentic-org`). Run all `backlog` CLI commands from that parent root.

## Current working mode

Two engineers build v1, each with their own agents. A lead agent takes an
increment from `backlog/docs/doc-5 - v1-Implementation-Plan.md` and owns it end
to end: UI, backend, database, shared contracts, docs and integration. No
path belongs to a named person; file ownership is stated in the increment's
short plan before code. One writer per file at a time; hand shared files over explicitly.

Use local subagents when useful under `.agent-kit/WORKFLOW.md`. The lead owns the
complete vertical feature and its integration; give implementation subagents bounded
scopes and use fresh local reviewers. Agree a shared interface before a second
increment depends on it, and update affected trait implementations together. No PR
or outside reviewer is a prerequisite.

Keep working toward the full v1 goal. If one feature is blocked, preserve its
checkpoint and continue an independent feature. A partial delivery is not v1
completion and is not a reason to stop.

The product is named **Agentic Org**. For data relationships, fields, identities,
canonical files and selected execution/control contracts, start with
`backlog/docs/organization-data/doc-72 - Organization-Data-—-README.md`
and `backlog/docs/design/doc-62 - Design-Documentation-Authority-Map.md`.
Designs (`backlog/docs/design/`), ADRs (`backlog/decisions/`) and UX/AX specs
(`backlog/docs/specs/`) explain those contracts in context; where one differs from
the data contract, the data contract wins. Use `principal-designer` for browser UX and `principal-ax-designer` for `agent-os` AX (`doc-96` outranks persona). A requirement doc says what is to be
built; what exists is in the checklist evidence and executable status doc below.

`backlog/docs/doc-3 - Feature-Checklist.md` is the single feature delivery checklist.
Update its relevant row when changing implementation or scope with evidence and remaining work.
Mark Done only for full stated scope, keeping unknown status explicit. Requirements stay in owning
specs; historical task logs do not establish completion. Assign each row to one primary category in
`backlog/docs/doc-4 - Feature-Areas.md`, keeping summaries consistent and linking cross-area dependencies.

`backlog/docs/design/doc-68 - Design-v1-Project-Structure.md` defines target names and domain-module/vertical-slice layout. Worker and UI paths remain `apps/agentic-org-office` and `apps/agentic-org-ui`. Executables are `agentic-org`, `agentic-org-office`, `agentic-org-web` and `agent-os`; host code lives in `crates/org-host` composed by `agentic-org serve`. Package names match executables; persisted identities stay stable; domain moves remain pending. See `backlog/docs/design/doc-63 - Design-Executables-and-Entry-Points.md` for entry-point status.

Build order is in `backlog/docs/doc-5 - v1-Implementation-Plan.md`; `backlog/docs/doc-2 - Start-Here.md` maps requirement locations. Engineers and agents make architectural decisions inside `backlog/docs/doc-7 - Architecture-Non-Negotiables-and-Who-Decides-What.md` and record them.

## Rules
1. **Read `backlog/docs/doc-8 - Architecture-Rules.md` first.** For domain/data work, read the authority map and owning organization-data contract before secondary designs or UX criteria. Follow `doc-68`: apps compose crates, policies own contracts, `org-proto` deps are intentional, no source cycles, logic in libraries, thin binaries. Planned variation (OS, vendor, storage, deployment) goes behind a consumer-owned adapter from day one; unimplemented OS/provider/engine fails closed (`doc-8` §Planned variation).
2. **Branches and integration:** follow `backlog/docs/processes/doc-20 - Process-Branch-Review-and-Integration.md`.
   - Start feature branches from `origin/v1`. Use separate worktrees for concurrent writers; otherwise work in the current checkout.
   - Keep one active implementation branch per lead; preserve blocked checkpoints and advance independent work.
   - Review locally, merge `origin/v1` into your branch, notify other lead sessions, `git merge --no-ff` into `v1` and push. `v1` integrates **all v1 features** (owner, 2026-11-18; `v1-integration` retired); no PR or outside reviewer required.
   - Never push to `main` without explicit release instruction; never delete unmerged work. Clean up merged worktrees and local/remote branches per `doc-20` §5.
3. **Checks and local review:** use kit task-routing, skill-routing, and review-cycle. Keep original author context, fix confirmed findings, validate original intent, and obtain a separate fresh final local review for the exact commit before merging.
   - **Read-only tiered review & cache hygiene (owner, 2026-11-23/24):** Pass all fast deterministic gates (`fmt`, Repowise `exit 0`, `clippy --workspace --all-targets -- -D warnings`, tests, `kit_hooks.py check`) **before** starting/resuming any review. Run read-only reviews via `python3 .agent-kit/runtime/kit_sol_review.py start --effort <xhigh|max> --scope <rust|ui|docs|full> -` and `resume <session-id> -` (never bare `codex exec` or `--ephemeral`) to lock read-only mode, `hooks=false`, `multi_agent_v2=false`, `HINDSIGHT_DISABLED=true`, `390000` compaction, scoped MCP/plugin disables, and effort/cache stability. Keep slices ~800–1,500 non-test LOC; cap `xhigh` at 2–3 passes before a **separate fresh** `max` gate (accepted by `kit_codex.py final`) until `VERDICT: APPROVE`. Prompts explain scope, forbid edits, and request structured findings (`Severity`, `file:line`, proof, direction, verdict); lead verifies cited code before fixing or rejecting false positives.
   - **Efficiency & quality watchdog (owner, 2026-11-24):** `kit_sol_review.py` runs `kit_efficiency_watchdog.py` inline after every pass and `outcome` receipt (alerts on stderr). Record each finished review session's quality receipt via `python3 .agent-kit/runtime/kit_sol_review.py outcome <session-id> --confirmed N --false-positives M [--regressions K] [--prior-session <id> --escaped E] [--commit <sha>]`, and run `python3 .agent-kit/runtime/kit_efficiency_watchdog.py --window-hours 24` at task closeout and every 3 tasks or 2h (with `/schedule` on long runs) to refresh `.agent-kit/logs/efficiency-watchdog/latest.md`. On any `HIGH` alert, shallow approval, quality escape, or recurring waste, diagnose, fix without weakening quality, verify, and commit on `v1`.
   - Rust changes: run the formatter, production and all-target Clippy gates, and workspace tests in [Development tooling](DEVELOPMENT.md#required-local-checks).
   - App changes: type check, tests, build, relevant browser checks, and UI evidence.
   - Docs/workflow-only changes: relevant kit/catalog checks and instruction review; no unrelated app test run required.
4. **Never write to a user's organization workspace from tests.** Use temporary directories.
5. **Pull request descriptions are short:** sections **Why**, **What**, **Notes**, **Breaking changes** ("None" when none), <= 2,000 chars total (owner, 2026-11-19).
6. **Required tools are green before any work starts.** See [Tools for agents](#tools-for-agents).

## Tools for agents

Start the agent (Claude Code or Codex) at the repository root; it is the entry point for
all work, including the UI. **Every tool below is required and must be green before
any work starts**, in every checkout and worktree (each is prepared on its own; a new
worktree runs the setup commands below first). Run `.agents/check-tools.sh` from the
repository root to check: it prints nothing when all are present. When it reports a
missing tool, do not start or continue the task: tell the user each missing tool, what
it is for and the command that fixes it, and wait until the check passes. Never work
around a missing tool, and never disable or bypass the check (for example by creating
its `.agents/.tools-ready` marker by hand).

- **Claude Code** runs the check automatically at session start and before every
  Edit/Write tool call, and blocks those calls while a tool is missing. Shell commands
  stay available only to install the tools; never write project files through the
  shell while the check is red.
- **Codex** runs `.agents/check-tools.sh --codex` itself before starting work; that
  mode skips the rows marked *Claude Code only*.
- **Required skills loaded:** `svelte-code-writer`, `svelte-core-bestpractices`,
  `shadcn-svelte` and `playwright-cli`. The check proves each is installed and valid;
  only the agent can see whether its session loaded them. At session start, confirm
  each appears in your list of available skills (Claude Code loads them from
  `.claude/skills/`, pre-approved in `.claude/settings.json`; Codex from
  `.agents/skills/`). If any is missing, stop, name it and tell
  the user; do not start work.
- **Worktrees:** a subagent in a separate worktree is gated against the session's
  checkout, so prepare and check the worktree (`cd` into it and run the check) before
  that subagent starts.

| Tool | What it gives agents | Set up with |
| --- | --- | --- |
| UI dependencies | Svelte MCP server, pinned shadcn-svelte CLI, type checks, lint, tests | `cd apps/agentic-org-ui && bun install --frozen-lockfile` |
| Playwright browsers | UI browser tests | `cd apps/agentic-org-ui && bunx --no-install playwright install chromium webkit firefox` |
| Node (`.node-version`) and Bun (`packageManager` in the UI `package.json`) | UI toolchain | install the pinned versions |
| rustup and the pinned toolchain | Rust toolchain from `rust-toolchain.toml` | <https://rustup.rs>, then `rustup toolchain install` at the repository root |
| rust-analyzer | Rust navigation (Claude Code LSP tool, Codex Rust LSP MCP) | `rustup component add rust-analyzer` at the repository root |
| svelte-language-server | Svelte navigation (Claude Code: the repository's `svelte-lsp` plugin; Codex: `svelte_lsp` in `.codex/config.toml`) | `npm i -g svelte-language-server@0.18.6` |
| mcp-language-server (*Codex only*) | Codex's bridge to the Rust, TypeScript and Svelte language servers (`.codex/config.toml`) | `go install github.com/isaacphi/mcp-language-server@v0.1.1` |
| typescript-language-server with TypeScript 6 | TypeScript navigation (Claude Code LSP tool; Codex: `typescript_lsp` in `.codex/config.toml`); TypeScript 7 has no `tsserver` | `npm i -g typescript-language-server@6.1.0 typescript@6.0.3` |
| python3 | agent kit checks (`.agent-kit/runtime/kit_hooks.py check`) | install Python 3 |
| Repowise code index | `starter_kit_context` `lookup`: search, symbols, relationships and history | `python3 .agent-kit/runtime/kit_context.py setup --query is_uuid_v7 --expect-path crates/org-proto/src/identity.rs` |
| Hindsight memory (local `hindsight-embed` daemon) | `starter_kit_context` `recall`: reviewed project memory | install as in "Local memory daemon" in `.agent-kit/ADOPTION.md`, then `python3 .agent-kit/runtime/kit_memory.py setup` (also restarts a stopped daemon) |
| cargo-deny 0.20.2, Gitleaks 8.30.1 | required local checks | Homebrew on macOS; see `DEVELOPMENT.md` |
| playwright-cli | browser skill | `bun add --global --exact @playwright/cli@0.1.24` |
| Required skills (links: *Claude Code only*) | the skills below | committed; `git checkout -- .claude/skills .agents/skills` restores them |

If tools that are installed are reported missing, the agent was probably started
without your shell's `PATH`; start it from a terminal with your normal profile.

Claude Code configuration in the repository supplies the rest; approve it when Claude
Code first asks (Codex uses `.codex/config.toml`, managed by the agent kit):

- `.mcp.json`: the Svelte MCP server, run from the lockfile-pinned
  `apps/agentic-org-ui/node_modules`, never downloaded at run time; and the agent
  kit's `starter_kit_context` server (the launcher from `.agent-kit/context-mcp.json`).
  Use its `lookup` for code discovery before broad searches and its `recall` for
  project memory, as `.agent-kit/reference/CONTEXT_TOOLS.md` describes.
- `.claude/skills/`: committed links to the required skills in `.agents/skills`
  (sources in `skills-lock.json`; do not edit those vendored copies). Follow
  `apps/agentic-org-ui/AGENTS.md` for Svelte documentation and autofixer use; the
  `svelte-file-editor` agent comes from the UI app.
- `.claude/marketplace/`: the repository's own `svelte-lsp` plugin, which only runs
  `svelteserver` for `.svelte` files. The upstream `svelte` plugin is not used because
  its MCP server downloads `@sveltejs/mcp` at run time.
- `.claude/settings.json`: the `typescript-lsp`, `rust-analyzer-lsp` and `svelte-lsp`
  plugins, the kit's hooks, the tool check at session start and before Edit/Write tool calls, pre-approval of the
  required skills, and `permissions.deny` rules for the download forms below.

Use only pinned tools; never download a package at run time. The vendored skills show
download forms (`npx`, `pnpm dlx`, `bunx --bun`, `@latest`, `npm init`, `npm install -g`).
Use these instead:

- shadcn-svelte: `cd apps/agentic-org-ui && bunx --no-install shadcn-svelte <command>`
  (the version in the UI's `package.json`).
- Playwright test runner: `cd apps/agentic-org-ui && bunx --no-install playwright <command>`.
- Browser automation: the pinned global `playwright-cli`.

`DEVELOPMENT.md` has the full list of checks and versions.

## The goal: work inside the product as soon as possible

Owner direction, 2026-11-17. Two engineers and their agents are building this
product, and they are to be onboarded onto it as early as it can carry them, so that
they talk to each other, track their tasks and run their agents through the product
they are building. The owner's order of work: milestone one is one office on a Mac
(getting in, the organization with its office, projects, teams and people, agents
hired through HR, messages, then tasks); milestone two is Linux, built as its own
track alongside milestone one; milestone three is
a second office. As each step lands, the people and agents building the product
start using it.

Nothing from before is carried over. The earlier command-line tool that the former
development team used to message each other, and its message queue, are dropped:
they are not preserved, imported or cut over, and no rule protects them. Standalone
person-started commands select an explicit instance and office or a saved context;
managed employees use their injected binding.

## Session start: check for a handover

First read `backlog/docs/workstreams/doc-14 - Workstreams-Overview.md` and your stream's file
(`doc-15` backend, `doc-16` linux or `doc-17` UI under parent repo `backlog/docs/workstreams/`): it is the shared record of where each stream stands and
what comes next; a local `HANDOVER.md` only adds checkout detail.

At the start of every lead session, check for `.agent-kit/tasks/HANDOVER.md` in this
checkout (each checkout has its own). It is local session state, ignored by Git and
never committed. If it exists, read it before starting new work, summarize its state and
next step to the user in a few lines, and ask whether to continue that work; do not
resume it unasked. When the handed-over work is finished or the user drops it, say so
and, after the user confirms, delete or replace the file.

## Local coordination

Use local subagents through the installed agentic kit when needed. Until the product
can carry the team's own coordination (see the goal above), coordinate through the kit
and this repository.

### Architecture ownership and efficient parallel work

- Each lead owns its increment's plan, acceptance contract, architecture, shared interfaces, dependencies, and integration; agree cross-increment contract/security/storage changes before dependent work.
- Assess independent slices at planning/checkpoints; record parallel splits or why serial is better in the plan.
- At most two concurrent source writers per increment (including the lead), using separate worktrees, nonoverlapping files, and explicit ownership handoffs for shared files.
- Before dispatch, specify worktree/base, owned files, interfaces, dependencies, acceptance checks, routed model/effort, escalation rules, and skill packet; preserve author sessions.
- Every implementation/review handoff references independent delivery and dependency boundaries (`backlog/docs/doc-8 - Architecture-Rules.md`, `backlog/docs/doc-6 - Independent-Delivery-and-Dependency-Boundaries.md` in parent `agentic-org-project`), stating usable outcome/foundation, prerequisites, and early production-path verification.
- Workers return concise receipts (changed paths, check results/logs, remaining issues, decisions needed) with full logs on disk. The lead inspects targeted code/contracts without repeating routine investigations; independent reviews, author validation, and full checks remain mandatory.

### Communication for the lead and delegates

- The main lead answers concisely in normal prose with evidence, risks, and decisions needed.
- Every delegated session (implementation, lookup, architecture, final review) reads and applies `.agents/skills/caveman/SKILL.md` before its first response (delegate-only, never the main lead's voice; never reduces scope, tests, or review depth).
- Include the skill reference (or text if file reads are unavailable) and delegate-only scope in every dispatch. Source changes invalidate old packets—resolve current guidance before subsequent assignments.

## Tailwind Plus (licensed UI)
The app's look uses Tailwind Plus components, which the licence allows in our open-source end product. **Allowed:** using or modifying them (including converting them to Svelte) **inside the product UI application (`apps/agentic-org-ui`)**. **Not allowed:** publishing them separately (a shared UI kit or component package, a template or starter, a Figma kit), or building a website-builder or admin-panel product from them. Never move them into `crates/` or a standalone package.

<!-- starter-kit:start -->
# Starter kit workflow

Read `.agent-kit/WORKFLOW.md` and the active profile in
`.agent-kit/policy/model-routing.json`. Preserve this project's engineering
conventions, architecture decisions and the integration rules above. The policy
names a model for each phase. A lead that runs on another model keeps every phase
and gate (plan, implement, fresh read-only review, repairs, original-author
validation, separate fresh final review), runs them with fresh-context subagents of
its own model, and records which model ran each phase. No gate is skipped because a
named model is unavailable. Skills live in
`.agents/skills/`; load only the current phase. Do not load whole task/review archives.
Use project-capabilities during adoption/maintenance and skill-routing before work.
Load only the selected project role brief and relevant skills; preserve full-stack
ownership when delegating independent pieces. Missing/stale guidance blocks readiness.

In `codex-only`, this Codex session is the lead. Use task-routing to register,
plan and assign scoped work with the installed Sol effort policy. Keep the original
author session for intent validation. Use bounded-lookup only for useful read-only
extraction; helpers never implement. Default to one worker; concurrent writers need
separate worktrees. Large/complex architecture needs a fresh independent review
before detailed planning. A failed premise pauses affected work for reassessment.

In `hybrid`, the implementation lead and final reviewer use Claude as selected by
the policy; Codex runs the Sol review/fix phase. Read relevant project conventions
in CLAUDE.md in that mode. Missing access never permits substituting models silently.

When invoked by kit_codex.py, follow its explicit phase: a fresh review is read-only;
fix only on the explicit resume turn. Never recursively invoke the runner, delegate,
commit, push, switch branches, alter review artifacts/check configuration or weaken
tests inside these turns. The wrapper owns repair commits. A separate ordinary lead
session may commit scoped accepted work when the user authorizes it.

Use the fixed Repowise MCP lookup for large-project discovery, bounded source reads
and project compiler/type checks for correctness. Use project-memory for scoped
source-backed knowledge. Treat memories/indexes as hints and revalidate against code.
A fresh reviewer inspects source before previous findings. Return unsupported design
assumptions, missing prerequisites and disputed fixes to the original author/lead.

Run `.agent-kit/runtime/kit_hooks.py check` explicitly before acceptance. Native
hooks provide feedback only when trusted/supported; they cannot replace current
verification receipts or the independent review cycle. Record unsuccessful outcomes
and unknown usage honestly through process-improvement. Do not edit secret files
(`.env*`, private keys or credentials); exact example/sample/template files are public.

Use `.agent-kit/reference/ADOPT.md` for setup. Distinguish configuration, verified
checks and a live model cycle. Do not claim model availability from a version probe.
<!-- starter-kit:end -->

<!-- BACKLOG.MD GUIDELINES START -->
<!-- backlog.md-instructions-version: 1.55.3 -->
<CRITICAL_INSTRUCTION>

## Backlog.md Workflow

This project uses Backlog.md in the parent **`agentic-org-project`** repository (`backlog/`). Run all `backlog` CLI commands from that parent repository root, not inside this code checkout.

**At the beginning of each conversation in this project, run `backlog instructions overview` (from the parent repo root) before answering or taking action. Re-read it only if you have not read it yet in the current conversation.**

Use the overview to decide whether to search, read, create, or update Backlog tasks.

Before task lifecycle actions, read the matching detailed guide:
- `backlog instructions task-creation` before creating or splitting tasks
- `backlog instructions task-execution` before planning, changing status or assignee, adding a plan or implementation notes, or implementing task work
- `backlog instructions task-finalization` before checking acceptance criteria, writing final summaries, or moving tasks to terminal statuses

Use `backlog <command> --help` before running unfamiliar commands. Help shows options, fields, and examples.

Do not edit Backlog task, draft, document, decision, or milestone markdown files directly. Use the `backlog` CLI so metadata, relationships, and history stay consistent.

</CRITICAL_INSTRUCTION>
<!-- BACKLOG.MD GUIDELINES END -->
