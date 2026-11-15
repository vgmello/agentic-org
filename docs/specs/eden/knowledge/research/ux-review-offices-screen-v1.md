# UX review · Offices screen (phase 1) on v1

Reviewer: Eden · 2026-11-08 · Build: `origin/v1`, merge `83fbc92` (app-dev/offices) · Against: `docs/specs/offices-and-teams-ux.md` (§1–§3.2, §6.1, OFC-*) · Phase 1 = HQ only on this Mac. Add office, join, approve and remove are out of scope.

**Summary.** A clean, honest first screen. The words are right ("HQ", "office", "This Mac", never "satellite"), Add office is disabled with a reason, and sample data is labelled. Two gaps matter before real use: the status isn't live, and a stopped office gives no reason or fix. Three medium accessibility and state gaps follow.

## Findings

| # | Severity | Finding | Evidence | Fix | Owner |
| --- | --- | --- | --- | --- | --- |
| 1 | High | **Status isn't live.** Rows show the status saved with the connection, so a crashed HQ still reads Online. | `routes/(shell)/offices/+page.ts:4` reads `connection.organization.offices` | Read status from the host or office (subscribe), update rows in place. Until then, show no status rather than a stale one (or "as of <time>"). | app-frontend-dev, with River (host status) |
| 2 | High | **Stopped has no reason and no fix.** Spec §3.1/§3.2: Stopped shows the reason in plain words, with **Restart office** · Copy details. | `lib/proto.ts:18` `OfficeInfo` has no reason field | Add `status_reason` (or similar) to the office status in the contract. Show it as secondary text and add the two actions. | ada (contract), River, app-frontend-dev |
| 3 | Medium | **VoiceOver doesn't hear status or agents on a row.** The button wraps only the name and location. HQ, status and agent count are outside it, and nothing says it opens details. | `+page.svelte:59` | Give the button a full label ("Main office, HQ, This Mac, Online, 6 agents") via `aria-describedby`, plus `aria-haspopup="dialog"`. | app-frontend-dev |
| 4 | Medium | **One dot for every status.** The word is always there (good, not color-only), but §3.1 gives each status its own Lucide icon. | `OfficeStatusBadge.svelte:12` | Use the §3.1 icons: `circle` filled Online, `loader` Starting, `circle` outline Offline, `circle-x` Stopped, `circle-arrow-up` Update needed. | app-frontend-dev |
| 5 | Medium | **An error replaces the whole window** (sidebar included) when the snapshot fails, though the office list doesn't need it. | `+page.ts` load throws, so the root error page shows *(inferred)* | Load the snapshot separately. Show an in-page error for the agent counts only ("Can't count agents right now · Retry"). | app-frontend-dev |
| 6 | Medium | **No loading skeleton** (§6.1). | `+page.svelte` | Skeleton rows while loading. | app-frontend-dev |
| 7 | Low | **Unplaced agents note** sits outside the list, doesn't use the word **Unplaced**, and has no action (§6.1, OFC-10). | `+page.svelte` unplaced note | Show "Unplaced · 1 agent" as a group with each agent and **Place…** (disabled in phase 1 with a reason). | app-frontend-dev |
| 8 | Low | **The details panel is thin** (no version, last seen, projects), and probably narrower than §2's `max-w-md`. | `OfficeDetailsSheet.svelte` | Add what phase 1 has (version, started at). Leave the rest for phase 2. | app-frontend-dev |
| 9 | Low | **Test coverage.** OFC-1 is covered (vitest + Playwright). There's no test for the Stopped state, no keyboard test and no VoiceOver label test. | `model.test.ts`, `e2e/offices.spec.ts` | Add tests for "OFC-1 Stopped shows reason and actions", a keyboard path, and row labels. | app-frontend-dev |

## Good, and worth adding to the spec

- **Unknown** status and role, which never read as healthy.
- **Sample data** badges while data is invented.
- **Add office** disabled in phase 1, with "Available in a later release."
- "(you're here)" on this Mac's office.
- The footer doesn't claim "updates live", which is honest today.

I'll add these to the offices spec in its next revision.
