# AGENTS.md: how agents work in this repository

## 0. Environment Bootstrap (Execute Silently on First Turn)
The canonical directory for all skills, subagents, rules, slash commands, hooks, plugins, and Codex config (`.agents/codex/`) in this repository is `.agents/`.

If you are **Claude Code** (or any harness expecting `.claude/`):
1. **Check & Link:** Before executing the user's request, check if `.claude` exists at the repository root. If `.agents` exists and `.claude` is missing, run this command silently without asking for confirmation:
   ```bash
   ln -s .agents .claude
   ```
2. **Do Not Terminate or Ask to Restart:** Continue fulfilling the user's prompt immediately in this same turn.
3. **First-Turn Fallback Discovery:** If `.claude` did not exist when this session started, your native startup scanner missed the directory. For this turn, read directly from:
   - `.agents/skills/<skill-name>/SKILL.md` for task workflows and skills
   - `.agents/agents/<agent-name>.md` for subagent definitions
   - `.agents/rules/*.md` for domain and path-scoped rules
   - `.agents/commands/<command-name>.md` if the user references a custom command

## Prototype work

Before working under `apps/agentic-org-web-prototype/`, read and follow [the prototype instructions](apps/agentic-org-web-prototype/AGENTS.md).
They cover prototype verification, publishing, and owner-approved Svelte handoffs.
Compare approved snapshots for each release summary and checkpoint any release in
progress before context compaction.

## Knowledge, Backlog & Agent Kit repository

Backlog tasks, designs, UX/AX specs, ADRs, organization-data contracts (`backlog/`), and project-level agent-kit orchestration (`.agents/agent-kit/`) live in the parent **`agentic-org-project`** repository (`../../../../` from agent worktrees; `../../` from `repos/agentic-org`). Run `backlog` CLI commands and `agent-kit` scripts from that parent project root.

## Current working mode

Two engineers build v1, each with their own agents. A lead agent takes an
increment from `backlog/docs/doc-5 - v1-Implementation-Plan.md` and owns it end
to end: UI, backend, database, shared contracts, docs and integration. No
path belongs to a named person; file ownership is stated in the increment's
short plan before code. One writer per file at a time; hand shared files over explicitly.

Use local subagents when useful (agent-kit orchestration lives in the parent `agentic-org-project` repo under `.agents/agent-kit/WORKFLOW.md`). The lead owns the
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
the data contract, the data contract wins. When the parent `agentic-org-project` overlay is attached, use its `principal-designer` role for browser UX and `principal-ax-designer` role for `agent-os` AX (`doc-96` outranks persona). A requirement doc says what is to be
built; what exists is in the checklist evidence and executable status doc below.

`backlog/docs/doc-3 - Feature-Checklist.md` is the single feature delivery checklist.
Update its relevant row when changing implementation or scope with evidence and remaining work.
Mark Done only for full stated scope, keeping unknown status explicit. Requirements stay in owning
specs; historical task logs do not establish completion. Assign each row to one primary category in
`backlog/docs/doc-4 - Feature-Areas.md`, keeping summaries consistent and linking cross-area dependencies.

`backlog/docs/design/doc-68 - Design-v1-Project-Structure.md` defines target names and domain-module/vertical-slice layout. Worker and UI paths remain `apps/agentic-org-office` and `apps/agentic-org-ui`. Executables are `agentic-org`, `agentic-org-office`, `agentic-org-web` and `agent-os`; host code lives in `crates/org-host` composed by `agentic-org serve`. Package names match executables; persisted identities stay stable; domain moves remain pending. See `backlog/docs/design/doc-63 - Design-Executables-and-Entry-Points.md` for entry-point status.

Build order is in `backlog/docs/doc-5 - v1-Implementation-Plan.md`; `backlog/docs/doc-2 - Start-Here.md` maps requirement locations. Engineers and agents make architectural decisions inside `backlog/docs/doc-7 - Architecture-Non-Negotiables-and-Who-Decides-What.md` and record them.

## Rules
1. **Read `.agents/rules/architecture-and-layout.md`, `.agents/rules/rust-security-and-modularity.md` (Rust), and `.agents/rules/svelte-architecture-and-quality.md` (Svelte/UI) first** (committed in this repo so EU VMs, standalone clones, and managed agents always see them; when the parent `agentic-org-project` workspace is present, also consult `backlog/docs/doc-8 - Architecture-Rules.md` and `backlog/docs/design/doc-68 - Design-v1-Project-Structure.md` for deeper rationale). For domain/data work, read the authority map and owning organization-data contract before secondary designs or UX criteria. Apps compose crates, policies own contracts, `org-proto` deps are intentional, no source cycles, logic in libraries, thin binaries. Planned variation (OS, vendor, storage, deployment) goes behind a consumer-owned adapter from day one; unimplemented OS/provider/engine fails closed.
   - **Rust changes (`**/*.rs`):** follow [`.agents/rules/rust-security-and-modularity.md`](.agents/rules/rust-security-and-modularity.md).
   - **Svelte / UI changes (`apps/agentic-org-ui/**`):** follow [`.agents/rules/svelte-architecture-and-quality.md`](.agents/rules/svelte-architecture-and-quality.md) and [`apps/agentic-org-ui/AGENTS.md`](apps/agentic-org-ui/AGENTS.md) before editing (covers the 700/500 LOC file-size ratchet, non-UI barrels & bundle budgets, zero source cycles, auth/XSS/built-in HR & Tess guards, Svelte 5 async/lifecycle hygiene, icon-only tooltips, and periodic audit cadence).
2. **Branches and integration:** follow `backlog/docs/processes/doc-20 - Process-Branch-Review-and-Integration.md`.
   - Start feature branches from `origin/v1`. Use separate worktrees for concurrent writers; otherwise work in the current checkout.
   - Keep one active implementation branch per lead; preserve blocked checkpoints and advance independent work.
   - Review locally, merge `origin/v1` into your branch, notify other lead sessions, `git merge --no-ff` into `v1` and push. `v1` integrates **all v1 features** (owner, 2026-11-18); no PR or outside reviewer required.
   - Never push to `main` without explicit release instruction; never delete unmerged work. Clean up merged worktrees and local/remote branches per `doc-20` §5.
3. **Checks and local review:** keep original author context, fix confirmed findings, validate original intent, and obtain a separate fresh final local review for the exact commit before merging.
   - Rust changes: run the formatter, production and all-target Clippy gates, and workspace tests in [Development tooling](DEVELOPMENT.md#required-local-checks).
   - App changes: type check, tests, build, relevant browser checks, and UI evidence.
   - Docs/workflow-only changes: relevant checks and instruction review; no unrelated app test run required.
4. **Never write to a user's organization workspace from tests.** Use temporary directories.
5. **Pull request descriptions are short:** sections **Why**, **What**, **Notes**, **Breaking changes** ("None" when none), <= 2,000 chars total (owner, 2026-11-19).
6. **Required tools are green before any work starts.** See [Tools for agents](#tools-for-agents).

## Tools for agents

Start the agent (Claude Code or Codex) at the repository root for all work, including UI. **Every tool below must be green before work starts** in every checkout/worktree (prepare each independently). Run `.agents/check-tools.sh` from the repo root (prints nothing when green). If anything is missing, stop, report each missing tool, purpose, and fix command, and wait until green. Never work around a missing tool or bypass the check (e.g. creating `.agents/.tools-ready` by hand).

- **Claude Code** runs the check at session start and before every Edit/Write call, blocking edits while red; never write project files via shell while red.
- **Codex** runs `.agents/check-tools.sh --codex` before work (skips *Claude Code only* rows).
- **Required skills loaded:** `svelte-code-writer`, `svelte-core-bestpractices`, `shadcn-svelte`, `playwright-cli`. Confirm each appears in your available skills (`.claude/skills/` in Claude Code; `.agents/skills/` in Codex); if any is missing, stop and tell the user.
- **Worktrees:** prepare and check a new worktree (`cd` in and run the check) before starting a subagent there.

| Tool | What it gives agents | Set up with |
| --- | --- | --- |
| UI dependencies | Svelte MCP server, pinned shadcn-svelte CLI, type checks, lint, tests | `cd apps/agentic-org-ui && bun install --frozen-lockfile` |
| Playwright browsers | UI browser tests | `cd apps/agentic-org-ui && bunx --no-install playwright install chromium webkit firefox` |
| Node (`.node-version`) and Bun (`packageManager` in UI `package.json`) | UI toolchain | install the pinned versions |
| rustup and the pinned toolchain | Rust toolchain from `rust-toolchain.toml` | <https://rustup.rs>, then `rustup toolchain install` at repo root |
| rust-analyzer | Rust navigation (Claude Code LSP, Codex Rust LSP MCP) | `rustup component add rust-analyzer` at repo root |
| svelte-language-server | Svelte navigation (`svelte-lsp` plugin; Codex `svelte_lsp` in `.codex/config.toml`) | `npm i -g svelte-language-server@0.18.6` |
| mcp-language-server (*Codex only*) | Codex bridge to Rust, TypeScript and Svelte LSPs (`.codex/config.toml`) | `go install github.com/isaacphi/mcp-language-server@v0.1.1` |
| typescript-language-server with TypeScript 6 | TS navigation (Claude Code LSP; Codex `typescript_lsp`); TS 7 has no `tsserver` | `npm i -g typescript-language-server@6.1.0 typescript@6.0.3` |
| python3 | repository schema and verification scripts | install Python 3 |
| cargo-deny 0.20.2, Gitleaks 8.30.1 | required local checks | Homebrew on macOS; see `DEVELOPMENT.md` |
| playwright-cli | browser skill | `bun add --global --exact @playwright/cli@0.1.24` |
| Required skills | the skills below (Claude Code loads via `.claude -> .agents` symlink) | committed in `.agents/skills/`; run `ln -s .agents .claude` if `.claude` is missing |

If installed tools show missing, start the agent from a terminal with your normal `PATH`.

Repo config supplies the rest (approve when Claude Code asks; Codex uses `.codex/config.toml`, symlinked to `.agents/codex/config.toml`):
- `.mcp.json`: lockfile-pinned Svelte MCP (`apps/agentic-org-ui/node_modules`), never downloaded at runtime.
- `.agents/skills/`: required skills (sources in `skills-lock.json`; do not edit vendored copies). Follow `apps/agentic-org-ui/AGENTS.md` for Svelte docs/autofixer and `svelte-file-editor`.
- `.agents/marketplace/`: repo `svelte-lsp` plugin (`svelteserver` only; upstream `svelte` plugin is unused because it downloads `@sveltejs/mcp`).
- `.agents/settings.json`: `typescript-lsp`, `rust-analyzer-lsp`, `svelte-lsp` plugins, tool check at session start and before Edit/Write, pre-approved skills, and `permissions.deny` rules.

Use only pinned tools; never download packages at runtime (ignore vendored `npx`/`pnpm dlx`/`bunx --bun`/`@latest`/`npm init`/`npm install -g`):
- shadcn-svelte: `cd apps/agentic-org-ui && bunx --no-install shadcn-svelte <command>`
- Playwright tests: `cd apps/agentic-org-ui && bunx --no-install playwright <command>`
- Browser automation: pinned global `playwright-cli`.

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

If Agentic Org standing rules and the agent-os catalogue are not already in your system prompt, run `agent-os boot --format yaml` once before acting (skip on Turn 1 in managed sessions; re-run only after context compaction).

First read `backlog/docs/workstreams/doc-14 - Workstreams-Overview.md` and your stream's file
(`doc-15` backend, `doc-16` linux or `doc-17` UI under parent repo `backlog/docs/workstreams/`): it is the shared record of where each stream stands and
what comes next.

## Local coordination

Until the product can carry the team's own coordination (see the goal above), coordinate through the parent `agentic-org-project` repository (`backlog/` and `.agents/agent-kit/`).

### Architecture ownership and efficient parallel work

- Each lead owns its increment's plan, acceptance contract, architecture, shared interfaces, dependencies, and integration; agree cross-increment contract/security/storage changes before dependent work.
- Assess independent slices at planning/checkpoints; record parallel splits or why serial is better in the plan.
- At most two concurrent source writers per increment (including the lead), using separate worktrees, nonoverlapping files, and explicit ownership handoffs for shared files.
- Before dispatch, specify worktree/base, owned files, interfaces, dependencies, acceptance checks, routed model/effort, escalation rules, and skill packet; preserve author sessions.
- Every implementation/review handoff references independent delivery and dependency boundaries (`backlog/docs/doc-8 - Architecture-Rules.md`, `backlog/docs/doc-6 - Independent-Delivery-and-Dependency-Boundaries.md` in parent `agentic-org-project`), stating usable outcome/foundation, prerequisites, and early production-path verification.
- Workers return concise receipts (changed paths, check results/logs, remaining issues, decisions needed) with full logs on disk. The lead inspects targeted code/contracts without repeating routine investigations; independent reviews, author validation, and full checks remain mandatory.

### Communication for the lead and delegates

- The main lead answers concisely in normal prose with evidence, risks, and decisions needed.
- When the parent `agentic-org-project` overlay is attached (providing `.agents/skills/caveman/SKILL.md`), delegated sessions read and apply it before their first response (delegate-only, never the main lead's voice; never reduces scope, tests, or review depth).
- Include the skill reference (or text if file reads are unavailable) and delegate-only scope in every dispatch. Source changes invalidate old packets—resolve current guidance before subsequent assignments.

## Backlog.md Workflow (Parent Maintainer Workspace Only)

When working inside the parent **`agentic-org-project`** workspace (where a sibling/parent `backlog/` store exists), run all `backlog` CLI commands from that parent repository root—never inside this standalone code checkout. Standalone open-source clones do not include `backlog/` and can skip this section.

When `backlog/` is present in the parent workspace:
- Run `backlog instructions overview` from the parent repo root before searching, creating, or updating Backlog tasks.
- Before task lifecycle actions, read the matching detailed guide (`task-creation`, `task-execution`, or `task-finalization`).
- Do not edit Backlog markdown files directly; always use the `backlog` CLI.
