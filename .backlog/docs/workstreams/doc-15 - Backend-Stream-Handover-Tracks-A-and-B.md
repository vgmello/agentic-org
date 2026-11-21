---
id: doc-15
title: Backend Stream Handover (Tracks A and B)
type: guide
created_date: '2026-11-21T00:16:00Z'
updated_date: '2026-11-21T00:16:00Z'
tags:
  - workstreams
  - backend
  - handover
---
# Backend stream (Track A and B)

> [!NOTE] Review note (2026-11-21)
> All remaining Backend increments and follow-ups from this handover have been migrated into Backlog.md (`TASK-1`–`TASK-19`, `TASK-33`–`TASK-61`). Preserved open owner decisions: B2 Q8 (browser-to-office ownership — tracked in `TASK-5`), B2 Q9 (placed-agent checks to C3), macOS fd-holder check scheduling, and reconciling `.agent-kit/policy/model-routing.json` (`codex-only-2026-11-18`) with the owner's 2026-11-19 Claude Opus review rule.

Last updated: 2026-11-20 (PR 17 merged into v1 at 03df9805; C1 done)

Scope: Rust crates, the office worker (`apps/agentic-org-office`), host and CLI apps, server-to-server
wire, the office store and engines, data-contract and backend design docs, the agent kit (`.agent-kit/`),
backend rows of `docs/FEATURES.md`, and the documented web API (`docs/design/web-api-v1.md`).

**Paused by the owner on 2026-11-19: no new features; only review, verify and merge what is done.**
Ask the owner before starting anything new.

## How this stream works (owner rules)

- This file is the current state and wins over the delivery log, which is history
  (`.agent-kit/tasks/v1-milestone-1.md`).
- Integrate into `v1` (`git merge --no-ff`, push `v1`); never push `main` (AGENTS.md rule 2).
- Writers: Claude Opus, effort high. Every review: a fresh Claude `code-reviewer`, Opus, effort medium.
  No Codex or Sol runs (owner, 2026-11-19, usage). One fresh review per change and a whole-change review
  before every merge into `v1`; verify findings before acting.
- A PR review the owner asks for is posted as a comment on the PR.
- Backend and documented web API only; the UI stream owns `apps/agentic-org-ui`.
- When host or worker launch changes, the gate includes `cargo test -p agentic-org` and the org-host
  ignored real-process set (see the delivery log, "How to continue", step 3).

## Status

| Increment | Status | Evidence | Next / remaining |
| --- | --- | --- | --- |
| A1 to A4 (setup, registration, sign-in, create organization) | In progress | v1 (merged before 74865317) | Per their FEATURES rows. The browser-to-office path in A3's Build is not built: no browser request reaches an office yet |
| B1a · office store core | Done | v1 (merged before 74865317) | — |
| B1b · the office runs on the new store | Done | v1 5f9941f1 | Narrowings recorded in FC-146: real-host delete-runtime with office calls, and journal corruption or rollback, move to A5. Rewritten LNX-2 including step 10 passed in CI's container job on GitHub's Ubuntu runner (run 44552811792); rewritten LNX-1 not yet run on Linux |
| B2.1 · one home team on the store (domain) | Done | v1 7f3000bc | FEATURES FC-025..029 and "code today" text not yet updated; old test vectors `crates/org-office/test-vectors/{org-commands,deferred-org-commands,organization}.json` describe the deleted model (only UI tests read them; freeze or replace with B2.2) |
| B2.2 to B2.4 · wire, browser path and routes, CLI | Blocked: owner paused new features | plan `.agent-kit/slices/b2-organization-teams-people-plan.md` | Owner questions Q8 (who builds browser-to-office), Q9 (agent-placed checks to C3); confirm Q3 refusal code `invalid_request` vs Q2 `not_empty`, and "a manager must be active" (a paused agent can't manage) |
| B3, A5, O1, A6, B4, B6 | Not started | — | In `docs/implementation-plan.md` order, after B2 |
| C1 · the sign-in spike | Done | v1 03df9805 (PR 17) | Result: `docs/research/c1-sign-in-spike.md`; first adapter `crates/org-agent-runtime-claude` (internal foundation, no production caller). Its remaining checks are in FEATURES FC-161, FC-098; Codex sub-agent isolation with FC-172/FC-101 |
| L15 · `os-sandbox` isolation mode (v1 scope, owner 2026-11-20) | Not started | — | Planned: FEATURES FC-190, ADR-0015 decision 12 (design proposed). First step: the macOS spike (an iOS app built with `xcodebuild`, its simulator tests run under a Seatbelt profile) |
| L16 · dedicated accounts and one-command secure setup (late v1, owner 2026-11-20) | Not started | — | Design proposed: `docs/design/accounts-sandbox-and-extensions.md`, FEATURES FC-191. First step: the accounts, launcher, security and secrets spikes (note section 11); ADRs follow them. Open owner questions in the note's section 12 |
| L17 · extensions: Containers (Colima) and Xcode (late v1, not milestone one, owner 2026-11-20) | Not started | — | Design proposed: same note, section 8; FEATURES FC-192 to FC-194. The web app's extension slot waits for ui-dev's agreement (request in `docs/workstreams/ui.md`) |
| C2 to C5 (agents) | Not started | — | C3's next adapter work (the office session runner on `org-agent-runtime-claude`) has not started. Owner decides |

## Next up

When the owner lifts the pause: B2.2 (wire, dry run, `operation.get`; ui-dev updates its mirrors in the same
merge). The owner's answers to Q1 to Q5 are at the end of the B2 plan.

Accounts, sandbox and extensions (late v1, after milestone one's increments; the owner decides when): start
with the spikes in [the note's section 11](../design/doc-61%20-%20Design-Dedicated-Accounts-Agent-Sandbox-and-Extensions.md#11-spikes) (accounts and launcher, security, isolation setting,
secrets, administrator rights; the macOS iOS-build spike of FC-190), record
the results in `docs/research/`, write the ADRs of [section 10](../design/doc-61%20-%20Design-Dedicated-Accounts-Agent-Sandbox-and-Extensions.md#10-adrs-and-designs-to-write-or-update-after-the-spikes),
then build L15 (`os-sandbox`, FC-190) and L16 (accounts and setup, FC-191, with L6 / FC-161), then L17
(extensions, FC-192 to FC-194). Each spike section names the requirements (`ASX-`) it proves.

## Owner decisions 2026-11-20

The full list (D1 to D28) is in [the note's owner decisions](../design/doc-61%20-%20Design-Dedicated-Accounts-Agent-Sandbox-and-Extensions.md#owner-decisions-and-direction-2026-11-20);
its requirements are `ASX-1` to `ASX-20` ([requirements](../design/doc-61%20-%20Design-Dedicated-Accounts-Agent-Sandbox-and-Extensions.md#requirements)); where each is tracked is
[its tracking table](../design/doc-61%20-%20Design-Dedicated-Accounts-Agent-Sandbox-and-Extensions.md#where-this-is-tracked).

- Isolation by accounts is the default for an install; development uses `--isolation off` (D8; FC-191, L16).
- One setup command does every core administrator step; a closed-list helper is preferred for later steps (D6, D7).
- `os-sandbox` is v1 (late) (D4; FC-190, L15); Claude Code's sandbox switch is on by default (D2; FC-189).
- Extensions after milestone one: Containers (Colima) and Xcode, with a UI slot to agree with ui-dev (D9 to D12; FC-192 to FC-194, L17).
- Take-over paused to late v1 (D3; FC-151). Background tasks and sub-agents stay on (D1; ADR-0025).
- Answers of 2026-11-20 (D14 to D23): Linux offices in the template unit `agentic-org-office@.service` with a polkit rule; host secrets in owner-only files for now; extension `platform` modules (LNX-5 exception); ui-dev leads the UI slot at L17; agent accounts per office, separate from the service account; helper confirmed pending the spike; disk encryption check-and-warn; Git rules carry over; `uninstall` and changing the isolation setting ask once; the host-with-the-person option not adopted.
- Answers of 2026-11-20 (D24 to D28): the recovery code, CA escrow and protected recovery delivery are v2 (FC-016 Deferred; code stays until the owner confirms); a lost HQ in v1 is replaced by importing from Git (FC-167, R1) and other offices re-enroll with one command; one CA per organization, installed as trusted, with a "Trust this organization" download; watching an agent's screen through the agent account's own graphical session, "visible on my desktop" as the fallback; iOS signing is the developer's own setup and the non-negotiable-10 exception is accepted for Xcode-extension agents.
- No question of section 12 is open now; question 3 is deferred to v2.

## In flight

- Nothing. B1b and B2.1 are merged into v1; their branches are deleted after merging.
- Outside the repo: test logs and review reports lived in the session scratchpad and are not kept;
  every decision and result they produced is in the delivery log below. Plans are committed under
  `.agent-kit/slices/`.

## Open decisions (owner)

- B2 Q8: who builds the browser-to-office path (a new feature) — this stream or the other Track A lead.
- B2 Q9: move B2's checks that need a placed agent to C3.
- macOS admission relies on the audit token; the fd-holder check is unscheduled (security design).
- The kit's model policy on v1 now reads `codex-only-2026-11-18` (came with the UI merge), but the owner's
  rule above (no Codex/Sol, Claude Opus reviews) still applies. Reconcile `.agent-kit/policy/model-routing.json`
  with the owner.

## Small follow-ups (not new features)

- To do once secrets are settled: a secrets map (document and visual page: each secret, who holds it, where it
  lives, who can read it); per-program custody (each program with its own Keychain items or files) is a later
  release option ([note, To do and 6.4](../design/doc-61%20-%20Design-Dedicated-Accounts-Agent-Sandbox-and-Extensions.md#to-do)).

- CI on `v1` is red in jobs that were already red before B1b (checked on c60bcada, 58a44b2b, 701d8460): `rust (macos-15)` (`release::store` tests, `unsupported_bootstrap_never_creates_partial_offices_or_service_state`), Secret scanning (30 findings across the whole history), Dependency security (frontend audit). Browser CSP jobs are the UI stream's.

- `docs/design/office-host-boundary.md` still says HTTPS is unavailable (X7 added it).
- Kit: stale manifest hash for `skill-catalog.json` (needs the owner's overlay override), old review-run label, catalog rationale cites "HEAD:".
- Platform allow-list: two office test files keep 11 predicates.
- Flaky only under heavy load: three tests, listed in the delivery log.
- Look at before calling it a flake: `office::tests::a_creation_retried_after_a_crash_point_keeps_its_ca_and_its_one_entry`
  once saw 2 receipts instead of 1 (SQLite, AfterCommit) under load; it passes alone. Receipts are what make a
  retried creation safe.
- `.agent-kit/policy/review-sandbox.json` still lists `refs/heads/v1-integration` in `protected_refs` (harmless).
- Not tested: a team move racing a rename of one of its descendants (B2 plan risk C1).

Full history, decisions and gates: `.agent-kit/tasks/v1-milestone-1.md`, section "WHERE WE ARE / HOW TO
CONTINUE".

## For the other streams

- UI: office calls only the old tables served (messaging, subscribe, work writes, `status.set`,
  `operation.get`) answer `unsupported` until B2, B4 and B6. Organizations made before B1b must be
  recreated. B2.2 will change the org wire (membership model removed); ui-dev must update its mirrors in
  the same merge. Processes write contract noted: a taken key is refused with `already_exists`.
  Since B2.1, `team.create` is owner-only; an archived parent answers `invalid_request` (was `conflict`) and an
  unknown parent a recorded `not_found`.
- Linux: `deploy/container/lnx2.sh` creates the organization through the web API (B1b). The container
  HTTPS switch is not done. The two remaining allow-listed office test files are this stream's.
- Every web API route this stream adds is announced to ui-dev, one line per route.
