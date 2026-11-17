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

[`docs/FEATURES.md`](docs/FEATURES.md) is the single feature delivery checklist.
Update its relevant row when changing implementation or selecting/deferring scope;
include evidence and remaining work. Mark Done only for the row's full stated scope,
and keep unknown implementation status explicit. Requirements stay in their owning
specifications; historical agent task logs do not establish feature completion.
Assign each checklist row to one primary category defined in
[`docs/FEATURE-AREAS.md`](docs/FEATURE-AREAS.md). Keep its scope/boundaries and area
summary consistent with the checklist when changing statuses or categorization;
cross-area dependencies use links rather than duplicate feature rows.

The owner-requested [v1 project structure](docs/design/project-structure.md) defines the target names and domain-module/vertical-slice layout. The worker and UI source paths remain apps/agentic-org-office and apps/agentic-org-ui. Production executable names are agentic-org, agentic-org-office, agentic-org-web and agent-os; host code lives in crates/org-host and is composed by agentic-org serve. Application source/package names now match their executable names; persisted identities remain stable; domain moves remain pending. See docs/design/executable-migration.md for implemented versus unavailable entry points.

## Rules
1. **Read [`docs/architecture-rules.md`](docs/architecture-rules.md) first.** For domain/data work, also read the authority map and owning organization-data contract before implementing a secondary design or UX criterion. Follow the [v1 project structure](docs/design/project-structure.md): apps compose crates, policies own their required contracts, and dependencies on `org-proto` are intentional rather than mandatory for every crate. No source cycles. Logic lives in libraries, and binaries stay thin. Planned variation (another OS, vendor, storage engine or deployment form) goes behind a consumer-owned adapter contract from the first implementation; every operating system without an implementation gets an adapter that fails closed, an unbuilt provider or engine is refused at composition, and later additions change only the adapter and composition ([rule](docs/architecture-rules.md#planned-variation-goes-behind-an-adapter-from-day-one)).
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

## The goal: work inside the product as soon as possible

Owner direction, 2026-11-17. Two engineers and their agents are building this
product, and they are to be onboarded onto it as early as it can carry them, so that
they talk to each other, track their tasks and run their agents through the product
they are building. The lead's reading of what that means for the order of work:
creating an organization, signing in, people and teams, tasks and chat come first,
because they are what onboarding needs. As each milestone lands, the people and agents building
the product start using that feature.

Nothing from before is carried over. The earlier command-line tool that the former
development team used to message each other, and its message queue, are dropped:
they are not preserved, imported or cut over, and no rule protects them. Standalone
person-started commands select an explicit instance and office or a saved context;
managed employees use their injected binding.

## Local coordination

Use local subagents through the installed agentic kit when needed. Until the product
can carry the team's own coordination (see the goal above), coordinate through the kit
and this repository.

### Architecture ownership and efficient parallel work

- The lead owns one overall plan, acceptance contract, architecture, shared
  interfaces, dependency order and integration. Keep one fullstack integration
  owner; return material contract/security/storage changes to the lead before
  dependent implementation.
- At planning and meaningful checkpoints, assess ready independent slices. Use
  parallel work when its expected benefit justifies the handoff; record the split
  and dependencies, or why serial work is preferable, in the current plan.
- Allow at most two concurrent source writers across the task, including the
  lead, with separate worktrees, nonoverlapping files and agreed interfaces.
  While two workers write, the lead coordinates and verifies without becoming a
  third writer. Agree an explicit ownership handoff before changing shared files.
- Before dispatch, specify the worktree/base, exact owned files, interfaces,
  dependencies, acceptance checks, routed model/effort and escalation conditions.
  Supply the selected skill packet and bounded context, preserving author sessions.
- Every implementation and review handoff must reference the stable delivery
  contract and [independent delivery and dependency boundaries](docs/architecture-rules.md#independent-delivery-and-dependency-boundaries).
  State the usable outcome (or internal foundation), declared prerequisites and
  early production-path verification. Include its focused policy, `docs/independent-delivery.md`,
  directly in the resolved skill packet. Review actual boundaries and behavior;
  existing checks, original-author validation and fresh review gates still apply.
- Workers return concise receipts: changed paths/source references, check results
  and log locations, remaining issues and decisions needed. Keep detailed logs on
  disk. The lead uses targeted follow-up reads to assess architecture, integration
  and acceptance; avoid repeating routine worker investigations without a concrete
  gap, failure or contradiction. Required source inspection, independent reviews,
  author validation and complete feature checks remain mandatory.

### Communication for the lead and delegates

- The main lead answers concisely in normal prose. Include the evidence, risks
  and decisions the user needs; expand when clarity or complexity requires it.
- Every delegated session, including implementation, lookup, architecture and
  final review, must read and apply `.agents/skills/caveman/SKILL.md` before its
  first response. The skill applies only to delegate communication, never to the
  main lead's voice. It does not change task scope, model routing or review gates.
- The lead includes the skill reference and the delegate-only scope in every
  dispatch, including fresh-context and nested delegates. The baseline catalog
  includes it in implementation and review packets. If a delegate cannot read
  files, supply the skill text with its bounded assignment instead.
- Keep required review formats, findings, uncertainty and verification evidence
  complete. Source changes invalidate old packets: resolve and read current
  guidance before subsequent assignments. Concise reporting never permits
  shallower implementation, testing or review.

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
