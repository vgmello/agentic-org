---
process: branch-and-merge
name: Branch, review and integration
kind: process
level: organization
version: 2
owner: Codex lead
status: active
summary: Implement, verify and review locally, then integrate all v1 features into v1-integration.
applies_to: every agent that changes this repository
overridable: true
---

# Branch, review and integration

The owner selected solo Codex development with local subagents. Codex is the lead
and owns implementation and integration across all repository areas. Named team
ownership, external messaging, external reviewer waits and Ada approval are
superseded for current work. Follow [AGENTS.md](../../AGENTS.md) and the installed
[kit workflow](../../.agent-kit/WORKFLOW.md); no PR is required.

These are software-repository contributor instructions; their legacy seed front
matter is not an accepted organization-data payload. Materialized process records
use the selected stable-ID path, key, schema_version, revision, kind, title, scope,
lifecycle and attribution fields and preserve the explicit overridable value.
Selected organization process records remain governed by [organization structure](../design/organization-data/01-organization-structure.md)
and [Processes UX](../specs/processes-ux.md); changing this workflow does not
activate a product process engine or migrate organization data.

## 1. Branch and scope

Start a small feature branch from `origin/v1-integration`. Keep one active
implementation branch; preserve blocked branches and their checkpoints, then move
to independent work. Use separate worktrees for concurrent writers. Never delete
unmerged work or overwrite another local worker's changes.

## 2. Implement and verify

The lead owns the complete vertical feature, including applicable UI, backend,
database, contracts and integration. Use kit task-routing and skill-routing;
delegate bounded implementation work when useful within the kit's writer limit.
Keep shared interfaces explicit and update affected implementations together.

Run current source-bound kit checks and the affected repository checks from
AGENTS.md. Rust changes require formatting, workspace Clippy and workspace tests;
app changes require type checks, tests, build and relevant browser verification.
Documentation-only changes require relevant instruction/catalog checks and review.
Tests use disposable roots, never user organization workspaces.

## 3. Review locally

Use the kit review-cycle: fresh independent local review, confirmed fixes and
rechecks, original-author intent validation, then the separate fresh final local
review. Keep the author context and provide the exact branch, base, commit,
contract and relevant UI evidence. A later source change needs current review and
check evidence. Diagnose failed tools or repeated findings locally; do not wait
for offline developers or send them coordination messages.

## 4. Integrate and continue

After local acceptance and required checks, the Codex lead merges with
`git merge --no-ff` into `v1-integration` and pushes that branch. No external
reviewer READY or Ada go is required. Verify the remote commit and integrated
source. `v1-integration` is the shared branch for all v1 features; release `v1`
and `main` are not integration targets without an explicit release instruction.

Verify a completed branch is fully integrated before cleanup; preserve blocked
and unmerged work. Continue the next feature from the updated integration branch.
A partial feature or successful merge does not establish that v1 is complete.
