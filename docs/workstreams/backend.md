# Backend stream (Track A and B)

Last updated: 2026-11-19 (backend-dev)

Scope: Rust crates, the office worker (`apps/agentic-org-office`), host and CLI apps, server-to-server
wire, the office store and engines, data-contract and backend design docs, the agent kit (`.agent-kit/`),
backend rows of `docs/FEATURES.md`, and the documented web API (`docs/design/web-api-v1.md`).

**Paused by the owner on 2026-11-19: no new features; only review, verify and merge what is done.**
Ask the owner before starting anything new.

## Status

| Increment | Status | Evidence | Next / remaining |
| --- | --- | --- | --- |
| A1 to A4 (setup, registration, sign-in, create organization) | In progress | v1 (merged before 74865317) | Per their FEATURES rows. The browser-to-office path in A3's Build is not built: no browser request reaches an office yet |
| B1a · office store core | Done | v1 (merged before 74865317) | — |
| B1b · the office runs on the new store | Done, with narrowings recorded in FC-146 | v1 5f9941f1 | Real-host delete-runtime with office calls, and journal corruption or rollback, move to A5. Rewritten LNX-1 and LNX-2 step 10 not yet run on Linux |
| B2 · organization, teams and people | Blocked: owner paused new features | branch `wip/b2-1-checkpoint` (does not build); plan `.agent-kit/slices/b2-organization-teams-people-plan.md` | Owner questions Q8 (who builds browser-to-office) and Q9 (move agent-placed checks to C3) |
| B3, A5, O1, A6, B4, B6 | Not started | — | In `docs/implementation-plan.md` order, after B2 |
| C1 to C5 (agents) | Not started here | PR 17 (another agent builds the runtime) | Owner decides; reviewed on PR 17 |

## Next up

When the owner lifts the pause: finish B2.1 from `wip/b2-1-checkpoint`. The owner's answers to Q1 to Q5
are at the end of the B2 plan.

## In flight

- Branch `wip/b2-1-checkpoint` (origin) and local `worktree-b2-1` in worktree slot-1: partial B2.1, not merged.
- No other open branches. Every B1b branch is merged.

## Open decisions (owner)

- B2 Q8: who builds the browser-to-office path (a new feature) — this stream or the other Track A lead.
- B2 Q9: move B2's checks that need a placed agent to C3.
- macOS admission relies on the audit token; the fd-holder check is unscheduled (security design).
- PR 17: the rename can merge; the ADR-0032 docs have four blocking issues (review on the PR).

## Small follow-ups (not new features)

- `docs/design/office-host-boundary.md` still says HTTPS is unavailable (X7 added it).
- Kit: stale manifest hash for `skill-catalog.json` (needs the owner's overlay override), old review-run label, catalog rationale cites "HEAD:".
- Platform allow-list: two office test files keep 11 predicates.
- Flaky only under heavy load: three tests, listed in the delivery log.
- AGENTS.md and `docs/processes/branch-and-merge.md` still name `v1-integration`.

Full history, decisions and gates: `.agent-kit/tasks/v1-milestone-1.md`, section "WHERE WE ARE / HOW TO
CONTINUE".

## For the other streams

- UI: office calls only the old tables served (messaging, subscribe, work writes, `status.set`,
  `operation.get`) answer `unsupported` until B2, B4 and B6. Organizations made before B1b must be
  recreated. B2.2 will change the org wire (membership model removed); ui-dev must update its mirrors in
  the same merge. Processes write contract noted: a taken key is refused with `already_exists`.
- Linux: `deploy/container/lnx2.sh` creates the organization through the web API (B1b). The container
  HTTPS switch is not done. The two remaining allow-listed office test files are this stream's.
- Every web API route this stream adds is announced to ui-dev, one line per route.
