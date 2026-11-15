# AGENTS.md: how agents work in this repository

## Current working mode

Codex works alone as the lead and owns implementation across this repository,
including UI, backend, database, shared contracts, documentation and integration.
The former named developers and Ada are offline. Do not message them, check or
wait on their inboxes, request their agreement, or wait for their review or merge
approval. Former path ownership does not restrict the current lead's work.

Use local subagents when useful under `.agent-kit/WORKFLOW.md` and the active
`codex-only` model-routing policy. The lead owns the complete vertical feature and
integration; give implementation subagents bounded scopes and use fresh local
reviewers. Preserve interface compatibility and update affected trait
implementations together. No external developer or PR response is a prerequisite.

Keep working toward the full v1 goal. If one feature is blocked, preserve its
checkpoint and continue an independent feature. A partial delivery is not v1
completion and is not a reason to stop.

The product is named **Agentic Org**. For data relationships, fields, identities,
canonical files and selected execution/control contracts, start with
[`docs/design/organization-data/README.md`](docs/design/organization-data/README.md)
and the [documentation authority map](docs/design/documentation-authority.md).
The other designs in `docs/design/`, ADRs in `docs/adr/` and UX specs in
`docs/specs/` explain those contracts in their own contexts. Their older code/API
observations do not override the selected model. Conversely, draft SQL and target
acceptance criteria do not establish that a migration, command or service ships.
Read the owning primary document before changing a dependent example or rule;
keep implemented wire/security evidence explicitly scoped to its version.

The owner-requested [v1 project structure](docs/design/project-structure.md) defines the target names and domain-module/vertical-slice layout. The worker and UI source paths remain apps/agentic-org-office and apps/agentic-org-ui. Production executable names are agentic-org, agentic-org-office, agentic-org-web and agent-os; host code lives in crates/org-host and is composed by agentic-org serve. Application source/package names now match their executable names; persisted identities remain stable; domain moves remain pending. See docs/design/executable-migration.md for implemented versus unavailable entry points.

## Rules
1. **Read [`docs/architecture-rules.md`](docs/architecture-rules.md) first.** For domain/data work, also read the authority map and owning organization-data contract before implementing a secondary design or UX criterion. Follow the [v1 project structure](docs/design/project-structure.md): apps compose crates, policies own their required contracts, and dependencies on `org-proto` are intentional rather than mandatory for every crate. No source cycles. Logic lives in libraries, and binaries stay thin.
2. **Branches and integration:** follow [`docs/processes/branch-and-merge.md`](docs/processes/branch-and-merge.md) with the current solo workflow below taking precedence over older team instructions.
   - Start feature branches from `origin/v1-integration`. Use separate worktrees for concurrent writers; otherwise work in the dedicated current checkout.
   - Keep one active implementation branch at a time. Preserve blocked branches/checkpoints and move to independent work rather than stopping all delivery.
   - Review completed work locally, then `git merge --no-ff` into `v1-integration` and push that branch. It is the integration branch for **all v1 features**. Do not wait for a PR, external reviewer or Ada's go.
   - Never push to `main` or merge into release `v1` without an explicit release instruction. Never delete unmerged work; verify integration before cleaning up merged branches/worktrees.
3. **Checks and local review:** use the installed kit's task-routing, skill-routing and review-cycle. Keep the original author context, fix confirmed findings, validate original intent and obtain the separate fresh final local review for the exact commit. The lead merges after current checks and local acceptance pass.
   - For Rust changes, `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass.
   - For app changes, the type check, tests, build and relevant browser checks pass; capture UI evidence where applicable.
   - For documentation/workflow-only changes, run relevant kit/catalog checks and review the changed instructions; no unrelated application test run is required.
4. **Never write to a user's organization workspace from tests.** Use temporary directories.

## Live messaging during the standalone cutover

The installed `~/.local/bin/org-agent` CLI currently uses live Phase A spool state; this branch builds its replacement as agent-os. Do not overwrite the installed CLI or move/reset its spool as part of building/testing the new executable. The
standalone proposal does not redirect it now. Before switching the team, preserve
pending asks, replies and read/triage state through the conditional cutover in
[ADR-0003](docs/adr/0003-agent-messaging-office-first.md#live-phase-a-coordination-cutover-and-person-started-context).
Standalone person-started commands must select an explicit instance/office or saved
context; managed employees use their injected binding. Old coordination data must
not be discarded under the desktop-app no-migration decision.

## Local coordination

Use local subagents through the installed agentic kit when needed. Do not use the
installed messaging CLI for routine development coordination, status, review or
approval. Preserve its executable and live spool data for the product cutover;
this preservation requirement does not require contacting offline developers.

## Tailwind Plus (licensed UI)
The app's look uses Tailwind Plus components, which the licence allows in our open-source end product. **Allowed:** using or modifying them (including converting them to Svelte) **inside the product UI application (`apps/agentic-org-ui`)**. **Not allowed:** publishing them separately (a shared UI kit or component package, a template or starter, a Figma kit), or building a website-builder or admin-panel product from them. Never move them into `crates/` or a standalone package.

<!-- starter-kit:start -->
# Starter kit workflow

Read `.agent-kit/WORKFLOW.md` and the active profile in
`.agent-kit/policy/model-routing.json`. Preserve this project's ownership,
engineering conventions, architecture decisions and the solo integration rules
above. Older named-owner, messaging and approval instructions are superseded. Skills live in
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
