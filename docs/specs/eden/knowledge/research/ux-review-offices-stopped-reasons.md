# UX review · Offices: why an office stopped (pre-merge)

Reviewer: Eden · 2026-11-08 · Build: branch `app-frontend-dev/real-office-status` @ `182772a`, from my own e2e run (offices.spec.ts, 16 passed) · Against: `docs/specs/offices-and-teams-ux.md` §3.3 (rev 9), OFC-11, OFC-12.

**Verdict (UX):** good to merge for its scope. No blockers.

## Good, keep it
- The stopped row's secondary text is the reason in plain words ("It stopped unexpectedly"), and the status is an icon plus the word "Stopped", never color alone.
- A pending restart counts down ("Restarting in N s", `in_use`: "Trying again in N s"), updates about every 10 s, disappears once due, and the screen-reader label doesn't tick.
- An office on this Mac reads Stopped, never Offline. An unknown reason gets "It stopped for a reason this version doesn't know", and a missing one gets "It isn't running" (model.ts).
- Copy details gives the office, id, status, reason, time and app version, with a "Copied the details." toast and no tokens.

## Findings
| # | Severity | Finding | Fix |
| --- | --- | --- | --- |
| 1 | Low | No screenshot of a **stopped** office's details (offices-details shows an Online office), so the Why row and Copy details can't be checked visually. | Add an `office-stopped-details` shot in the OFC-11 test. |

## Next branch (agreed scope, not flagged)
Restart office / Try again / Start office (`office.restart`, gated on the `hello` capability), "Update Agentic Org to restart offices from here", and Check for updates for `unsupported_version`. Until they land, a stopped office has no action in the row; that's the planned order.
