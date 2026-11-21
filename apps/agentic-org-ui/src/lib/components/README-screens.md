# Adding a screen

Specs win: `docs/specs/app-shell-ux.md` §4 (titles), §6a (look), then your area's spec. Every route in
`docs/plans/v1-ui-screens.md` already exists as a placeholder: replace only that folder's `+page.svelte`
(and add a `+page.ts` if needed). Nav, top bar, Teams view switch and the settings frame are done; do not edit them.

## Module layout

`src/lib/modules/<module>/types.ts` (shapes), `api.ts` (the calls), `features/<feature>/*.svelte` (UI that
does one job), plus pure helpers with `*.test.ts`. Routes stay thin: load, compose, render.

## Mock data

`api.ts` declares an interface and exports a production object whose every call is `unavailableCall`.
Under `bun run dev:mock`, `e2e/fakes/modules/<module>.ts` replaces it by name; its data is JSON in
`e2e/fakes/data/<module>/`. Never import a fake or JSON from `src/`.

```ts
// src/lib/modules/inbox/api.ts
import { unavailableCall } from '$lib/platform/unavailable-api';
import type { InboxItem } from './types';
export interface InboxApi {
	list(organizationId: string): Promise<InboxItem[]>;
}
export const inboxApi: InboxApi = { list: unavailableCall };

// e2e/fakes/modules/inbox.ts
import type { InboxApi } from '../../../src/lib/modules/inbox/api';
import type { InboxItem } from '../../../src/lib/modules/inbox/types';
import items from '../data/inbox/items.json' with { type: 'json' };
export const inboxApi: InboxApi = { list: async () => structuredClone(items) as InboxItem[] };
```

Data from the org snapshot (members, teams, projects): import `orgApi` from `$lib/modules/organization/api` in the fake, as
`e2e/fakes/modules/shell.ts` does, so IDs match the existing pages. Navigation count and lists come from
`shellApi.navigation()` (`$lib/modules/shell/api`); Home's "Open Inbox (N)" reads `inboxOpen` from it too.

## Shared parts (`$lib/components/...`)

- `PageHeader` – h2 title, real count, description, actions (one primary), one named `back` link on detail pages.
- `TabsNav` – link tabs from the URL (`aria-current`); `match: 'prefix'` for nested pages; `focusHeading` for view switches.
- `Section` – a titled block (settings group, profile part). `DescriptionList` – label/value rows (`mono` for IDs, paths).
- `EmptyState` – what the place is for and one real next action. `ErrorState` – what failed, why, Retry.
- `SkeletonRows` – first-load rows with a Loading label and "Still loading" after 2 s.
- `Avatar` – round initials for agents and people (kind is a word, never a shape). `StatTile` – one figure inside a `dl`.
- `NotBuilt` – "{Title} isn't built yet" / "Not in this release". `MiniIcon` – Heroicons solid 20 inside controls.
- `Resource` (`resource.svelte.ts`) + `Loaded` – one read and its states: `new Resource(() => api.x(orgId))` loads at once;
  `<Loaded {resource} label error isEmpty emptyTitle …>{#snippet children(data)}…{/snippet}</Loaded>` renders skeleton,
  unavailable (with `place` + `area`, the route's NotBuilt), error with Retry, empty, then the content. `reload()` after a change.
- `ConfirmDialog` – destructive or consequential confirmation with its -ing word. `CopyButton`. `focus.ts`
  `neighbourFocus` – after removing a row, focus the next one. `classes.ts` – form and button class strings;
  `pills.ts` – status pill tones. Use these rather than a module's private copy; a module never imports another
  module's internals.
- Existing: `DividedRow`, `StatusPill`, `Callout`; primitives in `ui/` (button, dialog, sheet, dropdown-menu, input, label, checkbox, badge, alert).

## Page skeleton

```svelte
<svelte:head><title>{pageTitle('Members', 'Teams')}</title></svelte:head>
<!-- $lib/nav -->
<PageHeader title="Members" count={rows?.length} countLabel="members" />
{#if state === 'loading'}<SkeletonRows label="members" avatar />
{:else if state === 'unavailable'}<NotBuilt … /> or the spec's unavailable copy — never sample data
{:else if state === 'error'}<ErrorState
		title="Couldn't load the members"
		message={error}
		onretry={load}
	/>
{:else if rows.length === 0}<EmptyState title="No members yet" description="…">…</EmptyState>
{:else}<ul role="list" class="divide-y divide-line-subtle">…</ul>{/if}
```

The shell's top bar holds the page `h1` (the area name, from `areaFor` in `$lib/nav`); page headings are h2.
Link moves focus that `h1` automatically; for a `goto` that changes the page, call `focusDestinationHeading()`.
Use `resolve()` from `$app/paths` for every `href`/`goto` (lint enforces it); cast computed paths `as '/'`.

## Class strings

- Gutters: `px-4 min-[640px]:px-6 min-[1024px]:px-8`. Body `text-sm/6`, meta `text-xs/5 text-fg-muted`, headings `text-base/7 font-semibold text-fg`.
- Divided row: `relative flex items-center gap-x-4 px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8` in `ul.divide-y.divide-line-subtle`; one stretched title link `<a class="…"><span class="absolute inset-0"></span>{name}</a>`.
- Table header: `text-left text-xs/5 font-semibold text-fg-muted border-b border-line py-3`; rows `border-b border-line-subtle`; below 640 px render divided rows instead.
- Filter bar: `flex flex-wrap items-center gap-3 px-4 py-4 …gutters`; search input full width below 640 px.
- Field: `<Label for>` + `<Input class="max-[639px]:text-base max-[639px]:h-11" aria-invalid aria-describedby>` + help `mt-2 text-xs/5 text-fg-muted` / error `mt-2 text-xs/5 text-danger-text`.
- Focus: `focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus` (inset `-outline-offset-2` in rows, tabs, menus).
- Targets: controls `h-9` desktop, `max-[639px]:min-h-11` below 640 px. No shadows or gradients; one primary (`bg-primary`) per view; global New stays outline.

## Look tokens

Every colour comes from a look token in `src/app.css`, named for its role. **Never use raw palette classes**
(`text-gray-400`, `bg-white/5`, `outline-indigo-400`, `text-red-300` …) or colour literals: the unit test
`src/lib/platform/tooling/look-tokens.test.ts` fails on them and names the token to use, and
`bun scripts/look-tokens.ts` rewrites them (rerunnable; it prints anything it cannot map). A restyle or a light
mode changes the token values in `app.css` and `docs/specs/app-shell-ux.md` §6a together, not your markup.
Use a token with any colour utility (`bg-`, `text-`, `border-`, `divide-`, `outline-`, `ring-`, `fill-`) and
with an opacity where the role calls for a tint (`bg-danger-emphasis/10`, `ring-brand/30`).

| Token                                | For                                                                                 | Value today                                                                                          |
| ------------------------------------ | ----------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `surface`                            | page, top bar, dialogs, popovers (`bg-surface`, `bg-surface/50` for chart cards)    | gray-800                                                                                             |
| `surface-sunken`                     | floating menus and toolbars, code panels                                            | gray-900                                                                                             |
| `surface-raised`                     | fields, quiet panels, row hover                                                     | white 5%                                                                                             |
| `surface-overlay`                    | chips, secondary buttons, stronger hover                                            | white 10%                                                                                            |
| `surface-chip`                       | small labels on org chart tiles                                                     | gray-700                                                                                             |
| `tint` / `shade`                     | other light / dark layers, always with an opacity (`bg-tint/[0.03]`, `bg-shade/10`) | white / black                                                                                        |
| `tooltip`; `scrim`                   | tooltip fill; behind a modal (`backdrop:bg-scrim/80`)                               | gray-950                                                                                             |
| `line-subtle`                        | row and header dividers (`divide-line-subtle`, `border-line-subtle`)                | white 5%                                                                                             |
| `line`                               | section and card outlines (`outline-line`, `border-line`, `ring-line`)              | white 10%                                                                                            |
| `line-strong`                        | outlines that must show on a tint                                                   | white 20%                                                                                            |
| `field-border`                       | form field boundary (`outline-field-border`)                                        | gray-500                                                                                             |
| `fg`                                 | headings and primary text                                                           | white                                                                                                |
| `fg-bright`                          | code and chip text on a tint                                                        | gray-100                                                                                             |
| `fg-body`                            | reading text: messages, descriptions                                                | gray-200                                                                                             |
| `fg-secondary`                       | secondary labels, pill text                                                         | gray-300                                                                                             |
| `fg-muted`                           | meta, help, placeholders                                                            | gray-400                                                                                             |
| `fg-subtle`                          | disabled or decorative text, list markers; not ordinary text                        | gray-500                                                                                             |
| `on-emphasis`                        | text and marks on a solid `primary` or `danger-emphasis` fill                       | white                                                                                                |
| `on-warning`, `on-warning-strong`    | text on a solid warning badge                                                       | gray-900, gray-950                                                                                   |
| `primary`; `primary-hover`           | primary button and solid accent fills, checkboxes; hover of the onboarding buttons  | indigo-500; indigo-600                                                                               |
| `focus`                              | keyboard focus outline (`focus-visible:outline-focus`)                              | indigo-400                                                                                           |
| `ring`                               | focus outline of the onboarding and auth controls (shadcn's ring)                   | indigo-500                                                                                           |
| `link`                               | links                                                                               | indigo-400                                                                                           |
| `brand`                              | selection underline, working tints, highlight rings                                 | indigo-400                                                                                           |
| `brand-text`, `-strong`, `-stronger` | text on indigo tints; initials and hover; callout body                              | indigo-300, 200, 100                                                                                 |
| `quiet`, `quiet-emphasis`            | neutral status dots and pill tints                                                  | gray-400, gray-500                                                                                   |
| `danger` ramp                        | blocked and error                                                                   | red: base 400, `-emphasis` 500, `-text` 300, `-text-strong` 200, `-text-stronger` 100                |
| `warning` ramp                       | waiting on the owner (amber means only that)                                        | amber: base 400, `-emphasis` 500, `-deep` 600, `-text` 300, `-text-strong` 200, `-text-stronger` 100 |
| `success` ramp                       | completion, online                                                                  | green: base 400, `-emphasis` 500, `-text` 300                                                        |
| `positive` ramp                      | presence dots, person tiles, accepted/applies                                       | emerald: base 400, `-emphasis` 500, `-text` 300, `-text-strong` 200                                  |
| `info` ramp                          | starting and checking                                                               | sky: base 400, `-emphasis` 500, `-text` 300, `-text-strong` 200, `-text-stronger` 100                |

Every tone ramp has the same steps: the base colour for icons, dots, borders and tints (with an opacity);
`-emphasis` the solid fill; `-text` the readable text step (also small marks that need that contrast, such as
rings and spinners); `-text-strong`/`-stronger` the higher-contrast text inside a tinted callout. Status text is
`-text` (error text is `text-danger-text`, as §6a says); new code does not add another step. shadcn-svelte's own
variables (`background`, `foreground`, `primary`, `muted`, `accent`, `border`, `input`, `ring`, `destructive`, …)
are mapped onto these tokens, so `ui/` primitives follow a restyle too; shadcn's `border` is the default
border colour of the base layer, not a role, so markup uses `line`. Buttons: `ui/button`'s variants and the
`classes.ts` helpers use tokens only; colour, radius (`--radius`) and weight live there.
