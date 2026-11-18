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

Data from the org snapshot (members, teams, projects): import `orgApi` from `$lib/org/api` in the fake, as
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
{:else}<ul role="list" class="divide-y divide-white/5">…</ul>{/if}
```

The shell's top bar holds the page `h1` (the area name, from `areaFor` in `$lib/nav`); page headings are h2.
Link moves focus that `h1` automatically; for a `goto` that changes the page, call `focusDestinationHeading()`.
Use `resolve()` from `$app/paths` for every `href`/`goto` (lint enforces it); cast computed paths `as '/'`.

## Class strings

- Gutters: `px-4 min-[640px]:px-6 min-[1024px]:px-8`. Body `text-sm/6`, meta `text-xs/5 text-gray-400`, headings `text-base/7 font-semibold text-white`.
- Divided row: `relative flex items-center gap-x-4 px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8` in `ul.divide-y.divide-white/5`; one stretched title link `<a class="…"><span class="absolute inset-0"></span>{name}</a>`.
- Table header: `text-left text-xs/5 font-semibold text-gray-400 border-b border-white/10 py-3`; rows `border-b border-white/5`; below 640 px render divided rows instead.
- Filter bar: `flex flex-wrap items-center gap-3 px-4 py-4 …gutters`; search input full width below 640 px.
- Field: `<Label for>` + `<Input class="max-[639px]:text-base max-[639px]:h-11" aria-invalid aria-describedby>` + help `mt-2 text-xs/5 text-gray-400` / error `mt-2 text-xs/5 text-red-300`.
- Focus: `focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400` (inset `-outline-offset-2` in rows, tabs, menus).
- Targets: controls `h-9` desktop, `max-[639px]:min-h-11` below 640 px. No shadows or gradients; one indigo primary per view; global New stays outline.
