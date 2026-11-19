# Adapt this starter kit to an existing project

This is the entry point for an agent asked to set up a project. Use
`https://github.com/vgmello/agents` on branch `codex/claude-code-starter-kit` or a
pinned release commit. The repository's
`main` branch contains the previous agents repository, not this kit. Do not replace
the target project's source with the starter kit.

The outcome is a project configured for the workflow, with meaningful checks and
honest readiness evidence. Copying templates alone is not completion. Use the
existing session to do the setup; do not spawn a planning committee for installation.
Use this document as-is. The adopting agent discovers project paths, derives real
project commands and gathers the mode choice; the user does not edit this file or
fill in placeholders.

## Detect the project and ask for the mode

1. Honor an explicitly identified target project. Otherwise infer the application
   from the original workspace/current directory and Git metadata, before changing
   directories or cloning the kit. From inside that application checkout, use
   `git rev-parse --show-toplevel` to find its root, including when starting in a
   subdirectory or linked worktree. Record the resolved absolute path as
   `kit_project` and its branch/revision. Read-only discovery does not require a
   separate path confirmation.
2. Keep the starter-kit source checkout separate from the application. If already
   inside the kit, use the original workspace context to locate the application;
   do not select the kit merely because it is the current Git root. If multiple
   applications are plausible or none can be identified, ask which project to
   configure, showing discovered names/paths when available. Wait for that answer
   before writing to a target. Treat a monorepo as one target unless the request
   or its existing project configuration establishes a narrower boundary.
3. Unless the user has explicitly chosen a mode for this adoption, ask:
   **"Which setup mode would you like for this project?"** Present these two
   choices using the host's selection UI when available (for example Claude's
   AskUserQuestion or Codex's question tool); otherwise show the same choices in
   chat. Ask about the mode, not individual model/tool choices.

   | Choice | Mode ID | What it uses |
   | --- | --- | --- |
   | Codex only | `codex-only` | Sol for planning, implementation and reviews; requires Codex access, no Claude account |
   | Claude + Codex | `hybrid` | Opus/Sonnet/Haiku for their assigned work, Sol for review/fixes, Opus for final review; requires both clients/accounts |

   For an upgrade, include the currently installed mode in the question. Wait for
   an explicit answer before mode-dependent installation, configuration or model
   calls; continue independent read-only inspection while waiting. A preselected
   option, installed credentials or the installer's default is not a user choice.
   Honor a mode already explicitly selected for this adoption without asking again.
4. Record the chosen ID as `kit_mode` and the separate kit checkout as `kit_source`.
   Use the selected branch/revision of the kit. The commands below use these
   agent-resolved values; preserve or rebind them across shell calls and always pass
   the chosen mode explicitly. Record target-detection evidence and the user's
   choice in the adoption report. An upgrade that changes mode must finish active
   sessions and follow the migration guidance before switching.

Use the fixed stack in `docs/CONTEXT_TOOLS.md` (installed as
`.agent-kit/reference/CONTEXT_TOOLS.md`): Repowise 0.61.0, Hindsight 1.2.1 over
reviewed Git memory, direct verification with retained logs, and the existing
model/review policy. Hindsight uses local CPU models and embedded pg0; no model API
key is needed. Do not ask the user to select tools or add overlapping indexes,
alternate memory services, navigation servers or command rewriters. RTK is excluded.

## Inspect and preserve the project

1. Use the detected/selected target. Resolve Codex instruction precedence at each
   applicable directory: nonempty AGENTS.override.md, then AGENTS.md, then configured
   fallback filenames. Read the effective instructions, CLAUDE.md, contribution
   guide, package manifests, CI and relevant architecture. Inspect ancestor/scoped
   instructions and configured fallback files explicitly; the inventory only
   identifies the standard root entry point.
   Check Git branch/status; preserve existing work and instructions.
2. From the starter-kit checkout, run the read-only inventory:

   ```bash
   python3 "$kit_source/scripts/adopt.py" --project "$kit_project"
   ```

   Inspect the returned candidates and CI. This inventory does not execute scripts,
   recursively understand every monorepo, select a language server or prove a check
   is nonmutating. Use the actual package manager and installed project tools.
3. Check Python/Bash/Git and Codex for the user's selected `kit_mode`;
   Claude is required only in hybrid mode. Inspect login
   status without reading credential files. A saved login does not prove a usable
   token. Record the kit revision and target starting revision.
   Install missing supported tools using their current official distribution and
   the project's established environment/dependency workflow when setup authorizes
   it. Keep existing profiles and credentials intact. Login, account entitlement
   and organization-managed access may still require the user; record those exact
   blockers while completing all independent project adaptation.

## Install into this project

Prefer a self-contained project installation. After the mode choice is resolved:

```bash
bash "$kit_source/install.sh" --project "$kit_project" --mode "$kit_mode" --dry-run
bash "$kit_source/install.sh" --project "$kit_project" --mode "$kit_mode"
```

The installer preserves custom instructions, checks, Codex config and existing
scalar settings. Reconcile reported runtime conflicts and back up deliberate
changes; do not use broad overwrite/reset to avoid understanding a conflict.
Both modes install the same `.agent-kit` runtime/knowledge and `.agents/skills`.
Codex-only installs five
native `.codex/agents` roles and no Claude files on a fresh project. Hybrid also
installs `.claude` adapters. Global installation is optional for Claude only.
Read `docs/CODEX_ONLY.md` in the kit (installed as
`.agent-kit/reference/CODEX_ONLY.md`) for migration: preserve historical evidence,
rerun setup/checks, and finish active sessions before switching modes; never reuse
an old acceptance.

## Adapt rather than overwrite

1. Inspect the effective root Codex entry point and `.agent-kit/WORKFLOW.md`.
   The installer merges into nonempty AGENTS.override.md when present; otherwise it
   uses AGENTS.md. A blank override falls through. Preserve and reconcile any
   previously effective fallback instructions before accepting this new entry point.
   In hybrid mode, merge pointers to task-routing, cost-aware-execution and
   review-cycle into existing CLAUDE.md. Preserve project
   conventions. Ensure the effective Codex file tells it to read the same project
   standards and decisions. Custom instructions get a bounded managed entry point;
   existing text is retained. Diagnostics must find `.agent-kit/WORKFLOW.md` in the
   effective file; a pointer only in shadowed AGENTS.md does not complete setup.
   Resolve contradictory old instructions explicitly. Custom
   native TOML is preserved; complex TOML is not guessed/reformatted. Reconcile
   required defaults from the adapter and verify effective model/effort and the
   two-child concurrency limit in Codex. Inspect `/hooks` and trust the exact
   installed definitions. Native child roles inherit parent permissions; use the
   review wrapper's read-only exec for independent review, not a prose sandbox claim.
2. Fill `.agent-kit/verification.json` with meaningful non-watching commands from the
   actual project/CI. Check each command's behavior; some lint/check scripts mutate
   files. Keep the fast gate within its 300-second maximum and document longer
   integration/UI checks separately. Do not put a no-op command in an empty config.
   Do not invent a framework test suite when the project has none: record the gap
   and define task-specific acceptance checks.
   Set `base` to the integration branch. A check is an argument array (always runs,
   blocks) or an object: `argv`; `paths`, workspace-relative patterns (`*` also
   matches `/`), so it runs only when a changed file matches. List every input the
   check's tests read, including files outside its project (fixtures, contract
   vectors, policy files). `blocking: false` marks a slow, low-priority check: it runs
   last, within 120 seconds and the time left before the evidence reserve, otherwise
   it is skipped with a warning; its failure is only a warning, but a command that
   cannot start is a configuration error. A review package refuses scoped evidence
   that did not see every file changed since the merge base with the review base.
3. Populate the project map with real entry points and verified references. Record
   accepted constraints in the decision ledger; do not import all historical prose.
   Read `project-memory`: seed relevant subsystem capsules, capture useful proposals
   with source evidence, and promote only after lead review. Record intent/next steps
   in task state before handoff or compaction; hooks only save metadata/pointers.
   Run `kit_memory.py audit`. Keep the root map/active ledger small, link area-specific
   detail, and preserve active constraints when curating. Recall requires an affected
   path and returns at most 4,000 characters by default. Do not bulk-load historical
   records. On upgrades, reconcile this guidance into preserved project docs explicitly.
   Preserve the existing ADR directory, otherwise use `.agent-kit/decisions/` for
   canonical architecture records and link them from DECISIONS.md. Seed verified
   requirements/premises and rationale from actual evidence; label unverified
   assumptions. Use architecture-decisions for review before detailed planning of
   large/complex tasks and for implementation-driven reassessment.
4. Inspect model/effort overrides, including forced subagent families. Preserve
   intentional user choices, but clearly identify conflicts with requested routing.
   Confirm actual model/effort through the harness after restart.
5. **Deploy Repowise 0.61.0; the tool choice is fixed.** Read the installed
   `.agent-kit/reference/CONTEXT_TOOLS.md` and `indexed-context` skill. Ensure Python
   3.12 with venv/pip is installed using the host's package workflow. Read a real
   project symbol and its source path; assign them to `kit_symbol` and
   `kit_symbol_file`, then run from `kit_project`:

   ```bash
   python3 .agent-kit/runtime/kit_context.py setup --query "$kit_symbol" --expect-path "$kit_symbol_file"
   ```

   The command provisions isolated hash-pinned dependencies, merges both MCP
   configuration(s) for the selected mode, prepares this checkout and validates a real MCP lookup. Record
   its receipt. Do not stop at writing a recommendation or set search-only mode to
   manufacture readiness. Report genuine platform/language/access blockers and
   complete independent setup work.
   Validate a definition/caller query and dirty/new/renamed/deleted files in a
   disposable worktree. Confirm the actual root and lookup access in Codex, and
   also Claude when hybrid, after restart/trust setup. Prepare each child repo/worktree separately.
   Use Repowise symbol/context queries and bounded `rg`/source reads. Run the
   project's established compiler/type checker for semantic correctness. Record
   unsupported languages/dynamic edges; no extra language-server bridge is required
   by this stack, and shared structural retrieval does not establish typed LSP parity.
6. **Deploy Hindsight; the memory choice is fixed.** Read
   `.agent-kit/reference/HINDSIGHT.md`. Install/start Docker with Linux containers
   and BuildKit through the host's supported workflow, preserving other containers
   and data. Run `python3 .agent-kit/runtime/kit_memory.py setup` from the project.
   It builds the pinned CPU image, starts embedded pg0 with a persistent volume,
   discovers the loopback port, configures the project bank, tests retain/recall/
   deletion in an isolated probe bank and syncs committed approved knowledge.
   Repeat sync if `remaining_records` is nonzero. No model API keys or user-selected memory backend is required. The existing
   context setup wires both `lookup` and bounded `recall`; restart/trust that MCP
   connection in each selected harness. It serves recall outside the shell sandbox
   without weakening review permissions. Preserve Codex and
   Claude subscription logins. Record actual billing mode; inspect credential
   override names without exposing values if the CLI uses API billing unexpectedly.
   Resolve host/login blockers explicitly. Rerun setup per checkout/upgrade;
   never copy receipts, clear volumes or disable required memory to claim readiness.
7. Leave provider caching enabled. Check for disable overrides. Read the installed
   cost-aware-execution skill before changing TTLs: subscription main sessions may
   already get one hour; API/usage-credit defaults may be five minutes. Record
   billing mode and observed TTL when available, otherwise unknown. Never promise
   a hit rate or create periodic model calls merely to keep a cache warm.

## Prepare project-specific agents and skills

Use the installed **project-capabilities** skill in the adopting agent's existing
session. This is a required setup step alongside tools and checks. Inspect the
actual stack/versions, workspace members, architecture, conventions, ownership and
critical areas; no framework/language is assumed from examples. Reuse existing
maintained skills, then create concise missing procedures grounded in project
source and official version-relevant documentation. Prepare local role briefs for
the generic lead, full-stack engineer, reviewer and scout. They define the project's
implementation and maintenance practices while retaining native role names/gates.

Fill `.agent-kit/skill-catalog.json` with source evidence, role brief paths, scoped
implementation/review references and critical-area/risk rules. Use the shipped
templates and `.agent-kit/reference/PROJECT_CAPABILITIES.md`. Do not load all skill
bodies into the lead context or install a speculative skill pack. An empty catalog
cannot be called configured. Missing essential skills/tools remain visible blockers.
Select a real reviewed evidence file as `kit_basis` and a representative project
area as `kit_scope`; add more evidence paths/risk tags where the assessment requires
them. Run from `kit_project`:

```bash
python3 .agent-kit/runtime/kit_skills.py evidence --include-guidance --path "$kit_basis"
python3 .agent-kit/runtime/kit_skills.py validate
python3 .agent-kit/runtime/kit_skills.py resolve --path "$kit_scope" --role fullstack-engineer --phase implementation
python3 .agent-kit/runtime/kit_skills.py resolve --path "$kit_scope" --role code-reviewer --phase review
```

Derive these values from real project evidence. Record the observed adopting session
and rationale. Test representative assignments and a critical area if present,
then use skill-routing's recipient preflight. Confirm actual harness access to
essential skills and tools; successful reference validation alone is not a live
capability test. Retain one end-to-end owner and specialize workers using scoped
skill packets. Critical procedures enrich existing reviews; extra reviewer agents
require a specific risk-based purpose. Material stack/architecture/ownership changes
or recurring defects trigger focused reassessment and updated evidence.

On an upgrade from 0.8, complete this new assessment before claiming CONFIGURED.
Catalog/briefs are project data preserved on future upgrades; keep native adapter
files managed and put local specialization in the referenced briefs.

## Verify and report readiness

Use the installed **setup-health** skill after installation/upgrade, and whenever
tools stop behaving correctly. From the target project:

```bash
python3 .agent-kit/runtime/kit_doctor.py --project . --live --checks --json
python3 .agent-kit/runtime/kit_policy.py --assessment .agent-kit/tasks/ASSESSMENT.json
python3 .agent-kit/runtime/kit_skills.py validate
```

The live doctor repeats actual MCP code lookup and memory retain/recall/deletion,
checks saved CLI logins and retains `.agent-kit/logs/health/latest.json`. `--checks`
also runs the configured source-bound project commands; inspect their effects
first. No model calls are made. Resolve failures and rerun before declaring setup
complete. Saved login is not proof of subscription allowance, model entitlement
or client trust. These probes are independent of the later application pilot.

Create ASSESSMENT.json from the installed template for a real representative task,
with evidence for known facts. An all-unknown template intentionally selects the
strong route (Sol Xhigh or Opus High) and is not a completed project assessment. Run a controlled failure and recovery
in a disposable fixture/worktree; do not break the project's current working tree.
For the live pilot, run one small real Sol review/fix cycle and original-context
validation plus the profile's new final review: Sol High/Xhigh in Codex-only, Opus
Medium/High in hybrid. Capture actual session IDs, models, check receipts,
repair commits, cache reports and relevant outputs. A CLI version probe or a mocked
test is not a live model test. If login is unavailable, record exactly that blocker
and finish all independent setup work; do not fabricate a clean review.

Before the first substantive task's planning, register its stable request/acceptance
contract with the installed process-improvement skill and kit_metrics.py. Close every
outcome, collect all review runs, and preserve missing measurements as unknown.
Use the same stable contract for Sol review. Confirm scorecard/export commands in a
disposable fixture; a simulated record does not establish real savings. Preserve
existing application architecture, ownership and merge rules during adoption.
When no model allowance is available, finish setup/checks and defer the live pilot;
do not launch an expensive full-project review merely to validate installation.

Write `.agent-kit/ADOPTION.md` from the installed template, including changes, retained
overrides, real check results, navigation mode and unresolved requirements. Separate:

- CONFIGURED: files, project role briefs, skill catalog, instructions and task policy adapted; Repowise
  says CONTEXT_READY and memory says HINDSIGHT_READY. Missing/failed services remain blockers.
- CHECKS_VERIFIED: real project commands and relevant index freshness/worktree
  smoke checks passed on the recorded source.
- LIVE_VALIDATED: the actual selected-profile cycle and failure behavior were observed.

Commit the scoped setup when the task authorizes commits. Report what the user can
run, what remains blocked and the evidence for each readiness state. Do not call the
project fully ready while only the first state is true.
