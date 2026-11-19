# v1 UI: Organization settings › Updates — delivery contract

Branch `v1-ui-updates` (worktree `agentic-org-ui-wt/updates`), from `v1-integration` at 38494638.
Plan row: `docs/plans/v1-ui-screens.md` Step 3 "Updates" (`/settings/updates`).
Boundary rules: `docs/architecture-rules.md`, `docs/independent-delivery.md`.

## Usable outcome

An owner opens Organization settings › Updates and sees, without leaving the browser:

- which release this instance (HQ) runs, and what HQ last learned about a newer release
  (up to date, a version available and whether it is already downloaded and verified,
  the check failed, the instance runs offline, or nothing reported);
- which release every office of the organization runs, compared with HQ, and which office
  holds new features back;
- what updating takes: the exact local command for the selected instance, where to run
  it, and what it does (from server-installation-and-access-ux §5), with a Copy command
  button.

There is **no restart-to-update or install button** (app-shell-ux §0.1: "Updates does not
offer restart-to-update before a supported distribution and update flow exists"). Upgrades
are explicit local CLI operations (server spec §1, §5, SRV-15).

Production: the web API does not serve this read yet, so the production adapter refuses
every call (`unavailableCall`) and the page shows the shared "Updates isn't built yet / It's
coming in this release." state (app-shell §0.1), never sample data (SRV-18). The mock lives
only in `e2e/fakes`.

## Sources (data contract wins)

- `docs/design/web-api-v1.md` §3 Instance view: `server_version` is the release pin's
  version; office view: identity, name, role, host, status.
- `docs/design/organization-data/11-model-profiles-and-agent-execution.md` "Release
  lifecycle": background checks stage updates; activation drains, switches the pin,
  restarts through the service manager, verifies readiness, keeps the prior release; an
  unavailable update source never prevents startup; offline performs no network request.
- `docs/design/standalone-server.md` "Service and updates" (release pins per instance).
- `docs/design/offices-hq-and-satellites.md` "Versions" (organization protocol level, the
  app shows which offices hold a new level back).
- `docs/specs/offices-and-teams-ux.md` §6.1 (office version; the note "<Office> runs an older
  version of Agentic Org, so new features wait until it updates.").
- `docs/specs/server-installation-and-access-ux.md` §4a.4 (a restart or update never ends a
  session), §5, SRV-15, SRV-16, SRV-34.

## Acceptance criteria

- UPD-1 · `/settings/updates` has the title "Updates · Organization settings · Agentic Org",
  an h2 "Updates" and the settings frame marks Updates current.
- UPD-2 · "This instance" shows the instance name and its version, and the update check
  state in words: Up to date (checked time), Update available (version, downloaded or not),
  Check failed (reason; startup is not affected), Offline (no checks), Not reported.
- UPD-3 · "Offices" lists every office, HQ first then A–Z, with where it runs and its version.
  Each non-HQ office states in words how its version compares with HQ: Same as HQ, Older than
  HQ, Newer than HQ, Version unknown. An office holding back new features is marked "Needs
  update" (amber: waiting on the owner) and the offices spec's quiet note names it.
- UPD-4 · "How to update" gives `agentic-org upgrade --name <instance>` with Copy command,
  says to run it on the HQ machine (named), and states its effects (spec §5) and that
  other instances and offices keep their version until updated there.
- UPD-5 · No control installs, restarts or checks: no restart-to-update, no "Check now".
- UPD-6 · States: loading skeleton with a Loading label (Still loading after 2 s); error with
  Retry (busy "Retrying…"), focus goes to the page heading after a successful retry;
  unavailable shows the not-built state; only HQ is a valid minimal state (no empty list:
  HQ always exists).
- UPD-7 · Layout at 1440, 1024, 640, 390, 320 px and 320 px with 200% text: no sideways
  scroll, pills keep whole words, targets ≥ 44 px below 640 px; look tokens only.

## Routes and files (one writer: this session)

- `src/routes/(shell)/settings/updates/+page.svelte` (replace the placeholder; thin).
- `src/lib/modules/updates/{types,api,index,present}.ts`, `present.test.ts`,
  `features/overview/*.svelte`.
- `e2e/fakes/modules/updates.ts`, `e2e/fakes/data/updates/overview.json`.
- `e2e/updates.spec.ts`; `e2e/gate-manifest.json` (register spec and sources).
- No shared files change. The settings navigation already lists Updates.

## Mock switches

`?mock=slow|fails|empty|unavailable` plus `?mock=current|failed|offline|unknown` for the
check state (comma-joined). `empty` means HQ alone with nothing reported.

## Non-goals

Restart/install from the browser, a "Check now" action, release notes, update policy
settings, rollback, uninstall, per-satellite remote upgrade, the API/page version mismatch
banner (shell/session concern), About in the account menu.

## Contract gaps for the lead (guesses)

- The web API has no route for update-check state or per-office versions; the UI type
  `UpdatesOverview` proposes them. Office view needs a version field (offices spec §6.1
  already shows "Agentic Org version").
- Not found is not applicable (no ID in the route); refusals and confirmations are not
  applicable (no mutations).
