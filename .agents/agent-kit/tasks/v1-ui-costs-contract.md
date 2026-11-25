# Contract · v1 UI: Organization settings › Costs

Branch `v1-ui-costs` (worktree `agentic-org-ui-wt/costs`), from `v1-integration` at `38494638`.
Plan row: `docs/plans/v1-ui-screens.md` Step 3, "Costs: prices, organization cost", route `/settings/costs`.

## Outcome

The organization owner opens Organization settings › Costs and can:

1. see what the organization has cost for a period (This week · This month · Last month · Custom · All time),
   at API prices in US dollars, with every qualification the ledger carries (estimate, unpriced, unknown,
   usage incomplete, API-equivalent, coverage as of a checkpoint and stream gaps);
2. break that total down by project (with the organization's Not on a task line), by consumer (agents and
   one-off smart features) and by work type (Build, Run, not recorded), and open the usage records behind any row;
3. see each agent's daily and monthly budget bars in the budget's time zone, with its zone and provenance
   (read here; they are set on the agent's profile, step 7);
4. read the versioned model prices that value token usage and add a rate (a new version from today or a later
   date), with its effect stated before saving.

Production answers every call with `unavailableCall`; the page then shows the area's unavailable state, never
sample data. The mock lives in `e2e/fakes/modules/costs.ts` and `e2e/fakes/data/costs/`.

## Requirements and their source

| # | Requirement | Source |
| --- | --- | --- |
| C1 | Owner only: amounts and budget bars show to the organization owner; anyone else sees one sentence saying amounts are the owner's, no amounts. Prices (rates, not spend) are readable by anyone signed in; only the owner adds them (guess to confirm with the owner) | costs-and-insights-ux §1 "Who sees amounts", Organization › Cost row; app-shell §0.1 "Costs where authorized" |
| C2 | Every cost is US dollars at API prices; estimates show ≈; unpriced/unknown show "—" with an accessible reason; an all-unknown sum stays "—"; null is never shown as zero | §0 decisions 2, 3; COST-5, COST-6 |
| C3 | A total that is partly known says "Usage incomplete" with counts (unpriced, unknown cost, unknown tokens) and runtimes that don't report usage; "includes API-equivalent" when it does; every affected subtotal repeats it | §0 decision 2, 9; COST-4, COST-6, INS-4 |
| C4 | Coverage: totals say "as of" their checkpoint and name stream gaps; never claim real-time zero | §0 decision 2; §5; COST-4 |
| C5 | Period filter This week · This month · Last month · Custom · All time, initial This month; Custom takes inclusive whole dates in the viewer's account zone (From required, To optional = now; To before From refused); bounds and zone are shown | §1 Periods; §3 "Cost reads and periods"; app-shell "Account time display" |
| C6 | Breakdowns by project (with Not on a task), consumer, work type; unknown classification stays unknown; each row opens its effective usage records | §1 Organization › Cost; COST-4; INS-2 |
| C7 | A usage record row shows consumer, time, runtime/provider/model, the four disjoint token categories (reasoning as a subset), token and cost completeness, basis (Runtime estimate / Token estimate / —), API-equivalent, price version and historical attribution, and a correction if it replaced an earlier record | §1 Usage record row; COST-1, COST-3 |
| C8 | Agent budget bars (owner only): daily and monthly, spent of limit with the 80% mark, unit dollars or tokens, the budget zone with its provenance ("Today (America/New_York)"); 80% reached (neutral), Stopping · Budget reached, Paused · Budget reached (amber), the one-time extension used; an incomplete amount is a lower bound; a runtime without usage says the budget can't stop its unmeasured work; an unknown budget zone says budgets can't start. The neutral "Budget reset" note belongs to the agent profile (step 7) | §1 Agent profile › Cost, Organization › Cost; §4 Periods, Missing data; doc 17; AC-IA20-2 |
| C9 | Prices: owner-managed versioned provider/model rates with effective dates; referenced versions never change; a change is a new version; the empty state says "No prices set: unpriced records add no known dollars." | §5 Prices; doc 10 `model_prices` |
| C10 | Add a rate: from today or a later date; four per-million rates in dollars converted exactly to integer micro-USD (blank = unknown, 0 = free); keeps values while Saving…; refusals on their field with focus; verified failure keeps input and retries with the same operation ID; an unknown outcome says "Save not confirmed" with Check status and the same operation ID | §5 Prices; §4 Set/raise dialog wording for unknown outcomes; app-shell §6a forms |
| C11 | States: loading skeleton shaped like the page; "No spend in this period"; "Couldn't load costs. Try again." with Try again (one for the page when every part fails, else per part); unavailable (one page placeholder when nothing is served); records sheet loading, empty ("No usage records yet"), error | §1 States; app-shell §3, §6a Feedback |
| C12 | Title "Costs · Organization settings · Agentic Org"; h2 page heading; focus per §6a | app-shell §4, §6a |
| C13 | 320 px, 200% text, 44 px targets below 640 px, no sideways scroll; breakdown rows become stacked rows below 640 px | app-shell §2, §6a |

## Routes

- `/settings/costs` – the page (thin route; the module renders it).

## States

Page: loading · unavailable (area placeholder) · error with Try again (all parts failed) · not authorized (non-owner: prices only) · ready.
Organization cost: loading · error · no spend in this period · ready (complete, incomplete, gap) · custom range refused at its field.
Budgets: loading · error · no agents · budget zone unknown · ready (under, 80%, stopping, paused, extended, no budget, runtime without usage).
Prices: loading · error · empty · ready (current, scheduled, replaced versions).
Add a rate: editing · client refusal on a field · Saving… · server refusal on a field · verified failure (Try
again keeps input) · not confirmed (Check status: saved, not saved, still unknown).
Records: loading · error · empty · list with Load more.

## Not in this slice (and why)

- Project › Insights, organization Insights, the Projects list Investment column, Home's cost line, the member
  Cost tab, typed-task Cost and time and release headers: plan steps 4, 5, 6 and 7. The shared parts
  (formatters, cost figure text, usage record row, budget bar, period menu) are exported from
  `$lib/modules/costs` for them.
- Setting, raising, clearing or extending a budget: agent budgets are set on the agent's profile Cost (owner
  only), task budgets on the task, release caps on the release (§4 "Where set"). There are no organization,
  project, team or feature budgets.
- Attribution corrections (Move…): from the task page's By agent table (§2).
- CSV export and the status report: Insights › Export (§3; the status report is not in v1).
- Trend chart: the Organization › Cost row asks for totals and breakdowns; its bars are drawn inside the
  breakdown table, which is itself the Table view.
- Editing or deleting a price version, and scheduled-draft management: doc 10 leaves price administration of
  unused drafts to a separate contract; referenced versions are immutable.

## Ownership

This slice writes only: `src/lib/modules/costs/**`, `src/routes/(shell)/settings/costs/+page.svelte`,
`e2e/fakes/modules/costs.ts`, `e2e/fakes/data/costs/**`, `e2e/costs.spec.ts`, `e2e/gate-manifest.json`
(spec registration and its WebKit exemption) and this contract. It also removes the unused price and cost
leftovers from `src/lib/modules/settings/{api,types}.ts`, `e2e/fakes/modules/settings.ts` and
`e2e/fakes/data/settings/{prices,costs}.json`, so the costs module is the one source of truth. No shared
component, nav, shell or settings frame changes.

Dependencies: the costs module imports shared components and `$lib/office/api` only. The route composes it with
the viewer's zone (`preferences` account zone) and the member profile's Cost-tab URL (`members` public index), so
the costs module imports no other domain module and the member Cost tab can import costs in step 7 without a cycle.

## Acceptance checks

`bun run check`, `bun run test`, `bun run format:check`, `bun run build` pass; `bun run lint` shows only the
nine known Chat-route errors; `e2e/costs.spec.ts` passes on chromium; screenshots at 1440 and 390 px of every
state in `checkpoints/shots/costs/`.
