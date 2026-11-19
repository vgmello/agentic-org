# v1 UI: Agent setup — delivery contract

Branch `v1-ui-agent-setup`, from `v1-ui` (step 3 row "Agent setup: list and one agent's setup, health" in
`docs/plans/v1-ui-screens.md`). Builder: principal designer (implement). Owner direction 2026-11-19: production
ready except the mock APIs, including structure and architecture.

## Outcome (usable)

An owner or administrator opens Organization settings › Agent setup and sees every current agent with its
configured runtime, model and model profile, its office and one Health word. They open one agent's setup to see
why it can or can't run, change its execution profile and model (execution revision) or its role (employee
revision), pause, resume or offboard it, and go to its profile. Hire an agent opens the existing hire flow. The
member profile's "Open Agent setup" lands on that agent's setup.

## Requirements and where they come from

| ID | Requirement | Source |
| --- | --- | --- |
| AS-1 | `/settings/agents` lists current agents (active and paused; former members are not listed) with configured runtime, model, model profile, office and Health. ≥640 px: aligned columns; below 640 px: divided rows. | our-org §5 `/settings/agents`, AC-IA07-1 |
| AS-2 | Health words: **Ready**, **Needs setup: {reason}**, **Paused**, and runtime readiness unavailable shown explicitly as **Runtime readiness unknown**. Needs setup takes precedence over Paused. Derived only from readiness/configuration facts; never from presence. Health grants nothing. | our-org §5 |
| AS-3 | Needs setup reasons: Execution not configured; Unplaced; office readiness blockers (missing binding, signed out, driver unavailable/unverified, coordinator outage, pool exhausted, …) with the fix when known; missing model only under the strict predicate (an exact selected model known absent from the runtime's reported list; unset model, unreadable list, missing runtime or unreachable office do not establish it). | our-org §5 "Missing data", §6 Problems, agent-run-modes §1, RUN-9, doc 11 §8 |
| AS-4 | `/settings/agents/:memberId` shows the supported configuration: execution (profile, model, ordered connections and their runtime and readiness on the agent's office, tools, prompt present), where it runs (office, placement generation, freshness, observed runtime, managed home read-only with no picker), the employee record (role, home team, reports to) and Health with reasons and fixes. | our-org §5 "Agent setup fields", supervision §1, run-modes §1 |
| AS-5 | Edit (execution) saves profile + model against the expected execution revision; identity is never written with it; no model is chosen silently (changing profile clears the model until chosen); a conflict shows the newer version and keeps the person's choices; refusals inline; busy label. Takes effect from the next turn. | our-org §5, run-modes §2, doc 11 §4/§5 |
| AS-6 | Change role saves `role_id` (or none) against the expected employee revision; "No role on record" when missing; a role grants nothing. | our-org §5, PROF-1, roles-ux |
| AS-7 | Pause / Resume / Offboard with the spec's preview words (§9); Pause shows stop pending until confirmed; Resume names remaining holds; Offboard previews retained identity and changes. Uses the existing organization commands, not a parallel path. | our-org §9, DEAL-1..4, RUN-10 |
| AS-8 | View profile returns to `/members/:memberId`; the profile's Open Agent setup links to `/settings/agents/:memberId`. | AC-IA07-1..3, AC-IA06 |
| AS-9 | Hire an agent opens the existing reviewed Home-team-required hire flow (`/teams/hire`). | AC-IA07-3 |
| AS-10 | Missing home team is an invalid record, shown as a problem, never "No team". The built-in HR lead is listed as Built-in and cannot be offboarded (the service enforces it; the UI does not offer it). | our-org §2a, §5 |
| AS-11 | States: loading skeleton shaped like the page, empty (no agents: Hire an agent), error with Retry, unavailable (production: NotBuilt, never sample data), not found (agent missing or not an agent), read-only viewer (no actions), refusals inline, busy labels, confirmations naming the effect. | app-shell §6a |
| AS-12 | Titles (app-shell §4): "Agent setup · Organization settings · Agentic Org"; "{Name} · Agent setup · Organization settings · Agentic Org"; "Agent not found · …". h2 page headings. Focus moves to the agent's heading after client navigation (after the shell's h1 tick). | app-shell §4, §6a |
| AS-13 | 1440, 1024, 640, 390, 320 px and 320 px at 200% text: no sideways scroll, 44 px targets below 640 px, status pills keep whole words, paths wrap. | app-shell §2, §6a |

## Routes

- `/settings/agents` → `AgentSetupList` (agent-setup module).
- `/settings/agents/[memberId]` → `AgentSetupPage`; `+page.ts` loads the organization snapshot (`loadOrgView`) for
  the shared Pause/Offboard commands and the Offboard preview, as the member profile does.

## Architecture and ownership

- New module `src/lib/modules/agent-setup/`: `types.ts`, `api.ts` (production: every call `unavailableCall`),
  `health.ts` (+ test), `present.ts` (+ test), `urls.ts` (+ test), `index.ts` (public API), `features/list/*`,
  `features/agent-page/*`, `features/edit-execution/*`, `features/change-role/*`, `components/HealthBadge.svelte`.
- Mock: `e2e/fakes/modules/agent-setup.ts`, data `e2e/fakes/data/agent-setup/*.json`; members, teams, offices and
  status from the org mock and the members fake's shared readers so IDs match. Switches `?mock=slow|fails|empty|
  unavailable|viewer|conflict|save-fails|not-found`.
- The parked part-way start in `modules/settings` (`agentSetups`, `saveExecution`, `AgentSetup*`, `ExecutionEdit`,
  `Readiness`, `ModelProfileChoice`, `e2e/fakes/data/settings/execution.json`) is removed: one owner for Agent setup.
- Couplings (declared in `e2e/gate-manifest.json` where the gate asks): agent-setup → `modules/employees` public
  index (hire URL); members profile → agent-setup public index (per-member URL). Lifecycle uses the shared
  `$lib/org/{actions,agents,session,api,load}`.
- Files edited outside the module: the four member-profile links to Agent setup (OverviewTab, RoleTab, RunNowDialog,
  IdentityDialog), `modules/settings/{api,types}.ts` and `e2e/fakes/modules/settings.ts` (removal only),
  `e2e/gate-manifest.json`, the two route folders, `e2e/agent-setup.spec.ts`. Shared components untouched.

## Non-goals (not in this increment)

- Editing tools, the system prompt, provider connections, model profiles or office credential bindings (office-local
  and private; no UI contract yet). Shown read-only where the record has them.
- An "execution enabled" switch: stopping an agent is Pause (run-modes decision 6); a disabled `execution.md` is
  shown as Needs setup.
- Placing an Unplaced agent (the placement flow is specified with the profile and not built); the page links to
  Offices › Place or offboard.
- Run modes, schedules, how-it-runs panel (RUN-1, RUN-7, RUN-13: not in v1). Per-agent process editing (Processes
  settings, step 6).
- Identity edits (name, title, focus, handle) stay in the profile's Edit.

## Verification

- Unit: `health.test.ts` (precedence, strict missing-model predicate, unknown), `present.test.ts`, `urls.test.ts`.
- E2E `e2e/agent-setup.spec.ts` (chromium): list columns and Health words, narrow rows, detail parts, Edit execution
  (save, conflict, refusal), Change role, Pause/Resume, Offboard, not found, error + Retry, empty, unavailable,
  read-only viewer, profile link round trip, 320 px reflow and targets.
- `bun run check`, `bun run test`, `bun run format:check`, `bun run build`, `bun run lint` (only the nine known Chat
  errors). Screenshots at 1440 and 390 px under `checkpoints/shots/agent-setup/`.
