# Design system · Agency v1

Owner: Eden · Status: v1.0 · 2026-11-11 (BUG-70). Replaces `design-system.md` v0.1/v0.2 in full. The lavender "Look and feel v0.2", its department tints and its token tables are **withdrawn**.

**Source of truth for the look:** the owner's dark Tailwind Plus prototype, `prototype-agency/` (owner decision, 2026-11-10). Every value below is read from its code, not from screenshots. Where the prototype fails our accessibility bar, the rule says so and gives the corrected value (marked **Fix**). Prototype code may be copied into `apps/org-ui` (owner decision); it must not become a separate package.

Prototype files cited as:

| Ref | File (under `prototype-agency/src/`) |
| --- | --- |
| P-layout | `routes/+layout.svelte` (frame, top bar, mobile drawer, toast) |
| P-sidebar | `lib/components/Sidebar.svelte` |
| P-home | `routes/+page.svelte` (page header, sort menu, agent rows, Activity feed) |
| P-agent | `routes/agents/[id]/+page.svelte` (secondary nav, profile header, stats, table, journal) |
| P-badge / P-dot | `lib/components/StatusBadge.svelte` / `StatusDot.svelte` |
| P-status | `lib/status.ts` (badge and task-chip colour classes) |
| P-avatar | `lib/components/Avatar.svelte` + `lib/format.ts` (`avatarColor`, `initials`, `ago`) |
| P-soon | `lib/components/ComingSoon.svelte` |
| P-error | `routes/+error.svelte` |
| P-icons | `lib/components/icons.ts` (Heroicons outline 24) |
| P-inline | `lib/components/Inline.svelte` (inline code and bold) |
| P-css | `app.css` (Inter, `soft-pulse`, reduced motion) |

App files: `apps/org-ui/src/app.css` (tokens), `src/lib/components/` (StatusPill, DividedRow, Callout, Sidebar, Logo, icons.ts), `src/lib/components/ui/*` (shadcn-svelte).

---

## 1. Principles

1. **Calm.** A dark gray canvas, one indigo accent, thin white/5 dividers, muted meta text. Colour is reserved for state that matters. No gradients, no glows, no shadows on dark surfaces (`dark:shadow-none` everywhere in the prototype).
2. **Roomy.** Rows are 16 px top and bottom with 12 px between title and meta (88 px for a two-line row). Headers are 24 px top and bottom from 640 px. Don't compress to fit more columns; move detail into the meta line or the detail page.
3. **Flat.** Lists are full-bleed divided rows, not cards. No rounded tinted boxes around lists, tables or sections. A surface change (`bg-black/10`, `bg-gray-700/10`) marks a region (sidebar, feed column, profile header), never a single item.
4. **One status signal per row.** A row shows its status once: one pill at the trailing edge, optionally with the matching dot before the title. Never a pill and a coloured word and an icon for the same fact. Other facts (severity, kind, version, team) go in the meta line as plain text.
5. **Words carry meaning.** Every status, count and state is written in words. Colour and icons only reinforce them (WCAG 1.4.1).
6. **Truth over decoration.** Show only state the system really has. The prototype's "Working" and its pulse are allowed only when the office reports a live run; until then the v1 words (Asleep, Paused, Status unknown…) apply. No invented metrics, no fake actions: a control that does nothing yet is shown disabled with its reason, or not at all.
7. **No emoji as UI.** Icons in chrome, status, navigation and buttons come from one set: **Heroicons** (outline 24 px, stroke 1.5, for navigation and large icons; solid 20 px "mini" for icons inside controls; solid 16 px "micro" inside pills). Emoji are allowed only as user content (message text, reactions a person picked).
8. **shadcn-svelte first, prototype classes on top.** Use the shadcn-svelte component when one exists (Button, Input, Dialog, Sheet, DropdownMenu, Tabs, Tooltip, Sonner, Badge, Alert, Checkbox, Label) and give it the prototype's classes. Don't hand-roll a menu, dialog or tab list.
9. **Plain language, accessible by default.** WCAG 2.2 AA: text 4.5:1, UI parts and focus 3:1, full keyboard use, visible focus, reduced motion respected (see §17–§19).

---

## 2. App frame and page layout

Source: P-layout. App: `routes/(shell)/+layout.svelte`.

| Part | Rule | Classes |
| --- | --- | --- |
| Window | The app fills the window edge to edge; nothing scrolls but the content column. | `fixed inset-0 overflow-hidden bg-gray-800` |
| Sidebar column | 288 px, fixed, full height (§3). | `fixed inset-y-0 z-50 w-72 flex flex-col` from 960 px |
| Content column | Scrolls; starts after the sidebar. | `h-full overflow-y-auto min-[960px]:pl-72` (`pl-16` in the rail) |
| Top bar | Sticky, 64 px (§4). | `sticky top-0 z-40 h-16` |
| Page header | Full-bleed strip under the top bar (§5). | `border-b border-white/5 px-4 py-4 sm:px-6 sm:py-6 lg:px-8` |
| Page gutters | 16 / 24 / 32 px. Every full-bleed row, header and section uses the same gutters so text lines up down the page. | `px-4 min-[640px]:px-6 min-[1024px]:px-8` |
| Max width | Lists, tables and the feed are full-bleed (no max width). Prose is capped: descriptions `max-w-3xl`, journal/notes `max-w-4xl`, empty-state copy `max-w-sm`. Forms inside a page `max-w-2xl`. Search field `max-w-xl`. | |
| Sections | Separated by a top border and space, not by cards. | `border-t border-white/10 pt-11`, section title `text-base/7 font-semibold text-white` with gutters |

**List + feed (Agents home).** Source: P-home.

| Width | Layout |
| --- | --- |
| ≥ 1280 px | Two columns. The list `<main>` gets `pr-96`; the Activity feed is an `<aside>` fixed to the right: `fixed top-16 right-0 bottom-0 w-96 overflow-y-auto border-l border-white/5 bg-black/10`. (The prototype switches at `lg`, 1024 px, but it has no persistent sidebar there; with our 288 px sidebar the list would be 352 px wide at 1024, so we switch at 1280.) |
| < 1280 px | One column: the feed `<aside>` follows the list in the page flow, same header and rows, `bg-black/10`, full-bleed. |

The feed column has its own header (§5) with the title **Activity** and a trailing **View all →** link to `/activity` (`text-sm/6 font-semibold text-indigo-400`, arrow `aria-hidden`).

**Until an activity feed exists (Eden, 2026-11-11):** don't render the column at all, not even as an empty or unavailable state, because an empty column on the landing page reads as broken. The list takes the full width; the column and its "View all" link appear when the feed ships. Activity stays reachable from the sidebar.

---

## 3. Sidebar

Source: P-sidebar. App: `lib/components/Sidebar.svelte`, `lib/nav.ts`.

**Items, in this order (owner decision, 2026-11-11):**

| # | Label | Route | Icon (P-icons / app `icons.ts`) | Current when |
| --- | --- | --- | --- | --- |
| 1 | Agents | `/agents` (Home) | `agents` | `/agents…` or `/org-chart…` (Org chart is a tab of Agents) |
| 2 | Chat | `/chat` | `chat` | `/chat…` |
| 3 | Projects | `/projects` | `projects` | `/projects…` |
| 4 | Work | `/work` | `tasks` | `/work…` |
| 5 | Sessions | `/sessions` | `sessions` | |
| 6 | Activity | `/activity` | `activity` | |
| 7 | Settings | `/settings` | `settings` | `/settings…` and `/offices…` (Offices lives under Settings) |

Not in the nav: **Org chart** (tab on Agents, §12), **Offices** (under Settings and the footer menu), **Processes** (hidden in v1.0; the route may stay reachable by URL only if engineering keeps it, but nothing links to it).

| Element | Rule | Classes |
| --- | --- | --- |
| Panel | 288 px wide, scrolls on its own, slightly darker than the page. | `flex grow flex-col gap-y-5 overflow-y-auto bg-black/10 px-6 ring-1 ring-white/5` |
| Logo block | 64 px tall (96 px with `pt-8` in the Tauri window so it clears the traffic lights, and it is a drag region). Logo mark 32 px + name. The name is the **organization name** (v1 decision), truncated with a `title`. | `flex h-16 shrink-0 items-center gap-x-3` · Logo `h-8 w-auto` · name `text-base/7 font-semibold text-white truncate` |
| Section label | "Navigation", "Projects". | `text-xs/6 font-semibold text-gray-400` |
| Section list | Groups 28 px apart; items 4 px apart; items bleed 8 px into the gutter. | `<ul class="flex flex-1 flex-col gap-y-7">`, inner `<ul class="-mx-2 mt-2 space-y-1">` |
| Item | 40 px tall (24 px line + 8 px padding), radius 6 px, 12 px icon gap. | `group flex gap-x-3 rounded-md p-2 text-sm/6 font-semibold` |
| Item default | Gray text and icon. | `text-gray-400 hover:bg-white/5 hover:text-white`; icon `size-6 shrink-0 text-gray-400 group-hover:text-white` |
| Item current | Filled with white/5, white text and icon, `aria-current="page"`. No indigo fill, no pill, no glow. | `bg-white/5 text-white`; icon `text-white` |
| Item icon | Heroicons outline 24, `stroke-width="1.5"`, `aria-hidden`. | `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">` |
| Count badge (e.g. Chat "Needs you") | Only for things waiting on the owner. Solid amber, dark number, with the spoken meaning in `aria-describedby`. Max one per item. | `ml-auto rounded-full bg-amber-300 px-1.5 text-xs/4 font-semibold text-gray-950` (13.9:1) |
| Focus | Inset ring so it isn't clipped. | `focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-indigo-400` (**Fix**: prototype indigo-500 is 2.9:1 on the current item) |

**Projects section.** Under the main items, label "Projects". One item per project the organization has, A–Z, linking to `/projects/<id>`; current when on that project. Item = item classes above, with a 24 px initial chip instead of an icon:

`flex size-6 shrink-0 items-center justify-center rounded-lg border border-white/10 bg-white/5 text-[0.625rem] font-medium text-gray-400 group-hover:border-white/20 group-hover:text-white` (`aria-hidden`), then `<span class="truncate">{name}</span>`.

The initial is decorative; the name is the accessible text. More than 8 projects: show the first 8 and a "View all projects" item (`text-gray-400`, no chip) linking to `/projects`. None: `<li class="px-2 text-sm/6 text-gray-400">No projects yet</li>`.

**Footer: owner and office.** Bottom of the panel (`-mx-6 mt-auto`), in the prototype's "You · Owner" style. Two sibling links, never nested (decided with Rowan, 2026-11-11):

| Part | Rule and classes |
| --- | --- |
| Owner link (the switcher) | Goes to `/welcome` (switch organization). Keeps today's accessible name and behaviour. `flex w-full items-center gap-x-4 px-6 py-3 text-sm/6 font-semibold text-white hover:bg-white/5` + inset focus. |
| Avatar | Owner initials, §8 `size-8`: `flex size-8 items-center justify-center rounded-full bg-gray-800 text-xs font-semibold text-indigo-300 outline -outline-offset-1 outline-white/10` |
| Owner text | `You <span class="font-normal text-gray-400">· Owner</span>`, as in the prototype; the link keeps today's accessible name ("Switch organization", Rowan's freeze 6c7dc9f). |
| Office link | Under it: `{This Mac} · {HQ} office` in `text-xs/5 font-normal text-gray-400 hover:text-white`, going to `/offices` (Settings > Offices). |
| Rail (640-959 px) | Only the logo switch; no footer text. |

---

## 4. Top bar

Source: P-layout. App: `lib/platform/shell/ShellTopbar.svelte`.

| Element | Rule | Classes |
| --- | --- | --- |
| Bar | 64 px (56 px below 640 px), sticky, bottom divider, same colour as the page. | `sticky top-0 z-40 flex h-16 shrink-0 items-center gap-x-6 border-b border-white/5 bg-gray-800 px-4 sm:px-6 lg:px-8` (`max-[639px]:h-14`) |
| Menu button | Below 640 px only (§20). 44 px target. | Heroicons mini `bars-3` `size-5`; `-m-2.5 rounded-md p-2.5 text-white` |
| Search field | Left, grows, max 576 px, 36 px tall. `/` focuses it (not while typing, not with the drawer open). Placeholder names the scope: "Search agents…". Use shadcn `Input` inside a `<form role="search">`. | Form: `grid h-9 max-w-xl flex-1 grid-cols-1 rounded-md bg-white/5 outline-1 -outline-offset-1 outline-gray-500 has-[input:focus-visible]:outline-2 has-[input:focus-visible]:outline-indigo-400`. Input: `col-start-1 row-start-1 size-full bg-transparent pr-10 pl-9 text-base sm:text-sm/6 text-white placeholder:text-gray-400 outline-hidden`. Icon: mini `magnifying-glass` `ml-2.5 size-5 self-center text-gray-400`. Key hint: `<kbd class="rounded-sm border border-white/10 px-1 font-sans text-xs text-gray-400">/</kbd>` (`aria-hidden`; the input has `aria-keyshortcuts="/"`). **Fix**: prototype `bg-white/8` + `outline-white/10` + gray-500 placeholder fail (§18). |
| Links | Only links that open something real. Spec / Design / Help from the prototype are **not** shipped until they have a destination. | `text-sm/6 font-semibold text-white hover:text-gray-300` |
| Divider | Between links and the primary action, from 1024 px. | `hidden lg:block h-6 w-px bg-white/10` |
| Primary action | One per app: **Add agent** (opens the Add agent sheet). Primary button §10 with mini `plus` icon. | `Button` default + `gap-x-1.5`, icon `-ml-0.5 size-5` |
| Page title | Not in the top bar. The page `<h1>` is in the page header (§5). (v1 currently puts it here; see the gap list.) | |

---

## 5. Page headers

Source: P-home header, P-soon header, P-agent section headings.

```
<header class="flex items-center justify-between border-b border-white/5 px-4 py-4 sm:px-6 sm:py-6 lg:px-8">
  <div class="flex items-baseline gap-x-3">
    <h1 class="text-base/7 font-semibold text-white">All agents</h1>
    <p class="text-xs/6 text-gray-400">8 agents · 1 paused</p>
  </div>
  <!-- trailing: sort/filter controls or a secondary action -->
</header>
```

| Part | Rule |
| --- | --- |
| Title | `text-base/7 font-semibold text-white` (16/28). The one `<h1>` of the page, focus target after navigation (`tabindex="-1"`). No 24–32 px page titles inside the shell. |
| Count line | Optional, baseline-aligned, `text-xs/6 text-gray-400`. Real counts only, joined with ` · `. Singular/plural correct ("1 agent"). |
| Description | If the page needs a sentence of context, put it **under** the header strip as the first content line, `px… pt-4 text-sm/6 text-gray-400 max-w-3xl`, or drop it. Not a paragraph in the header. |
| Sort control | Text button + menu (shadcn `DropdownMenu` with `RadioGroup`): `flex items-center gap-x-1 rounded-sm text-sm/6 font-medium text-white` → "Sort by <span class="text-gray-400">status</span>" + mini `chevron-up-down` `size-5 text-gray-400`. Menu width `w-40`, checked item shows mini `check` `size-4 text-indigo-400`. |
| Filter control | Same pattern as sort: "Show <span class="text-gray-400">all</span>" + DropdownMenu radio items (All / Active / Paused / Former, with counts). Not a segmented pill group, not code chips. |
| Secondary action | One outline button at most (§10). The app-wide primary lives in the top bar. A page-specific primary ("New task") may sit here when the top bar's isn't relevant. |
| Sample data | When the organization is sample data, a gray pill **Sample data** (§7 gray) after the title, once per page. Not repeated in sub-sections. |
| Breadcrumb / back | Detail pages use the prototype's secondary nav (§12): first item "← All agents" / "← Projects" (`inline-flex items-center gap-x-1`, mini `chevron-left` `size-4`). Not a "Projects › Atlas" text breadcrumb above a second title. |

---

## 6. List rows (DividedRow)

Source: P-home agent list and feed. App: `lib/components/DividedRow.svelte` (owns `<li>`, padding, divider).

| Part | Rule | Classes |
| --- | --- | --- |
| List | Native `<ul role="list">`, full-bleed, dividers between rows only. | `divide-y divide-white/5` (or DividedRow's `border-b … last:border-b-0`) |
| Row | 16 px vertical padding, page gutters, trailing items centred. | `relative flex items-center gap-x-4 px-4 py-4 sm:px-6 lg:px-8` |
| Title line | Optional status dot (§7), then the title. 12 px gap. | `flex items-center gap-x-3`; title `<h2|h3 class="min-w-0 text-sm/6 font-semibold text-white">` |
| Title parts | Primary name, separator, qualifier, then a gray secondary phrase hidden below 640 px. | separator `text-gray-400` (`·` or `/`); secondary `hidden truncate font-normal text-gray-400 sm:inline` + `sr-only sm:hidden` copy |
| Whole-row link | The title link stretches over the row with an empty `absolute inset-0` span; focus ring drawn on that span. One link per row; other interactive parts sit above it with `relative z-10`. | `[&:focus-visible>.absolute]:outline-2 [&:focus-visible>.absolute]:-outline-offset-2 [&:focus-visible>.absolute]:outline-indigo-400` |
| Meta line | 12 px under the title, small gray facts separated by 2 px dots. Lower-priority facts hide on narrow widths. | `mt-3 flex items-center gap-x-2.5 text-xs/5 text-gray-400`; each fact `<p class="truncate">` or `whitespace-nowrap`; dot `<svg viewBox="0 0 2 2" class="size-0.5 flex-none fill-gray-400" aria-hidden><circle r="1" cx="1" cy="1"/></svg>`; `hidden md:block` for the 4th+ fact |
| Trailing pill | The row's one status (§7). | `StatusPill` |
| Chevron | When the whole row opens something. | mini `chevron-right` `size-5 flex-none text-gray-400` `aria-hidden` |
| Hover | Linked rows only, subtle. (Addition: the prototype has none; Tailwind Plus uses this.) | `hover:bg-white/[0.025]` — meta stays ≥ 4.86:1 |
| Row actions | Not inline buttons in every row. Use a trailing `DropdownMenu` kebab (mini `ellipsis-vertical`, `size-5 text-gray-400`, 36 px target) or the detail page. | |
| Empty list | One `<li>` centred: `px-4 py-12 text-center text-sm text-gray-400 sm:px-6 lg:px-8`, e.g. "No agents match “sam”." | |

**Feed row** (Activity, P-home aside): `px-4 py-4 sm:px-6 lg:px-8`; line 1 `flex items-center gap-x-3`: avatar `size-6`, actor `flex-auto truncate text-sm/6 font-semibold text-white` (link `hover:underline` to the agent), time `<time class="flex-none text-xs text-gray-400">5m</time>` (**Fix**: prototype gray-600 is 1.9:1); line 2 `mt-3 truncate text-sm text-gray-400` with a `title` holding the full text.

**Agent row content (Agents home).** This is the row spec for the Agents list, and it amends AGENT-3: divided rows replace the 8-column table at every width.
- Title: `{name}`, then the secondary `{title}`.
- Meta line, in this order: `{teams}` · `reports to {manager}` · `{office}` · `{runs on} · {how it runs}`. Below 640 px, only teams and the manager show.
- Trailing: the status pill, then a chevron. The row opens the agent's details sheet.
- The "Now" column is dropped from the row; what the agent is doing now shows in the details sheet.

**Tables.** Use a real `<table>` only for data people compare across columns (task backlog, bug list). Prototype P-agent table: `w-full text-left`; `thead` `border-b border-white/10 text-sm/6 text-white`, `th` `py-2 font-semibold`; `tbody divide-y divide-white/5`; cells `py-4 text-sm/6`, first cell gutters `pl-4 sm:pl-6 lg:pl-8`; secondary columns `hidden md:table-cell` / `lg:table-cell`; long text `line-clamp-2|3` with a `title`. No rounded enclosure, no zebra stripes. Below 640 px a table becomes divided rows.

---

## 7. Status pills and dots

Source: P-badge, P-dot, P-status. App: `lib/components/StatusPill.svelte` (composes shadcn `Badge`), `AgentStatusIcon.svelte`, `OfficeStatusBadge.svelte`.

**Pill shape.** `inline-flex flex-none items-center gap-x-1.5 rounded-full px-2 py-1 text-xs/4 font-medium ring-1 ring-inset` + a tone. 24 px tall. The text never wraps letter by letter: `whitespace-nowrap`; when space runs out the **row** wraps the pill under the title, the pill never shrinks below its text (fix for the v1 390 px roster). Optional leading icon: Heroicons micro 16 at `size-3.5`, `aria-hidden`. Task and work-item chips use `rounded-md` instead of `rounded-full` (P-status `TASK_CHIP`), same tones.

**Tones** (dark values; contrast on `gray-800` / on a hovered row, §18):

| Tone | Classes | Text contrast | Note |
| --- | --- | --- | --- |
| indigo (active, in progress) | `bg-indigo-400/10 text-indigo-300 ring-indigo-400/30` | 6.32 / 5.47 | **Fix**: prototype `text-indigo-400` is 4.07 |
| gray (neutral, idle, waiting) | `bg-gray-400/10 text-gray-300 ring-gray-400/20` | 8.43 / 7.28 | StatusPill default (prototype gray-400 measured 4.11 on tinted rows) |
| outline (off, ended) | `bg-transparent text-gray-400 ring-gray-400/20` | 5.64 / 4.86 | |
| green (done, online) | `bg-green-500/10 text-green-400 ring-green-500/20` | 6.98 / 6.05 | |
| amber (needs you, paused, update needed) | `bg-amber-400/10 text-amber-200 ring-amber-400/30` | 9.62 / 8.30 | Only tone allowed to ask for attention |
| red (stopped, unreachable, blocked, critical) | `bg-red-400/10 text-red-300 ring-red-400/20` | 6.75 / 5.84 | **Fix**: prototype `text-red-400` is 4.48 |
| sky (starting, checking) | `bg-sky-400/10 text-sky-300 ring-sky-400/20` | 7.37 / 6.39 | Transitional states only |

**Every status the app uses:**

| Domain | Status (exact words) | Tone | Icon (Heroicons micro) | Dot |
| --- | --- | --- | --- | --- |
| Agent | Working *(when the office reports a live run; not in v1.0 data)* | indigo | — | green-400 + pulse |
| Agent | Needs you *(future)* | amber | `bell` | amber-400 |
| Agent | Asleep · wakes when called | gray | `moon` | gray, filled |
| Agent | Started by a person | gray | `user` | gray, filled |
| Agent | Paused | amber | `pause` | amber-400 |
| Agent | Former | outline | `arrow-right-start-on-rectangle` | outline ring |
| Agent | Status unknown | outline | `question-mark-circle` | outline ring (prototype's amber "Status unavailable" is **not** used: unknown isn't a call to act) |
| Chat presence | Working · Online · Asleep · Offline | text only in lists; 10 px dot on the avatar (green-400 / green-400 / gray-400 / outline) | — | |
| Office | Online | green | `check-circle` | |
| Office | Starting · Checking… | sky | `arrow-path` (spins, static under reduced motion) | |
| Office | Invited | gray | `envelope` | |
| Office | Offline | outline | — | |
| Office | Update needed | amber | `arrow-up-circle` | |
| Office | Stopped · Unreachable | red | `x-circle` / `signal-slash` | |
| Office | Status unknown | outline | `question-mark-circle` | |
| Task (chip) | To do | gray | — | |
| Task | In progress | indigo | — | |
| Task | Blocked | red | — | |
| Task | Done | green | — | |
| Bug (chip) | New | gray | — | |
| Bug | Triaged · In progress | indigo | — | |
| Bug | Fixed | green | — | |
| Bug | Closed | outline | — | |
| Bug severity | Critical · Major · Minor · Trivial · Severity not set | **meta text**, not a pill (one status per row). Critical/Major get mini `exclamation-triangle` `text-red-300`/`text-amber-200` before the word. | | |
| Request | Not routed · Sent to X · not accepted yet · X is scoping | gray | — | |
| Request | Scoped by X *(waiting for your Review)* | amber, word **Needs you** + meta "Scoped by X" | — | |
| Request | Approved | green | — | |
| Request | Declined · Merged · Changed | outline | — | |
| Goal | Active / Paused | indigo / amber | — | |
| Announcement reason (chat) | Delivered · Ready for review · Blocked · Report · Decision needed | meta text after the kind word, not a pill | | |
| Process *(hidden in v1.0)* | Sealed | outline + micro `lock-closed` | | |

**Dot** (P-dot): wrapper `flex-none rounded-full p-1` + tone, inner `size-2 rounded-full bg-current` (`size-1.5` small). Tones: working `bg-green-400/10 text-green-400`; idle `bg-gray-100/10 text-gray-400` (**Fix**: prototype gray-500 dot is 3.04, borderline; use gray-400); attention `bg-amber-400/10 text-amber-400`; off `text-gray-400` with the inner dot `ring-1 ring-current ring-inset` (hollow). The dot is `aria-hidden`; the row's link carries `<span class="sr-only">, {status}</span>`. Use the dot only on the leading edge of rows whose trailing pill states the same status, or alone where the status word is right beside it.

**Working pulse** (P-css): `@keyframes soft-pulse { 0%,100% { box-shadow: 0 0 0 0 rgb(74 222 128 / .45) } 50% { box-shadow: 0 0 0 5px rgb(74 222 128 / 0) } }`, `.animate-soft-pulse { animation: soft-pulse 2s ease-in-out infinite }`, removed under `prefers-reduced-motion: reduce`. Only on the green Working dot. Nothing else pulses.

---

## 8. Avatars and initials

Source: P-avatar.

| Rule | Classes |
| --- | --- |
| Round for everyone (agents and people alike); the kind is said in words ("Agent"), not shape. | `inline-flex flex-none items-center justify-center rounded-full bg-gray-800 font-semibold outline -outline-offset-1 outline-white/10` |
| Sizes | 24 px `size-6 text-[0.625rem]` (feed, chat compact, project chips) · 32 px `size-8 text-xs` (footer, chat messages) · 40 px `size-10 text-sm` (profile header). |
| Initials | First letters of the first two words, upper case; one word → first letter (`initials()`). |
| Colour | Only the letter is tinted, picked by a stable hash of the id from 7 tints: `text-indigo-300 emerald-300 sky-300 amber-300 rose-300 violet-300 teal-300` (`avatarColor()`); unknown → `text-gray-300`. All ≥ 7:1 on gray-800. |
| Decorative | `aria-hidden="true"`; the name is always written next to it. The 10 px initials are the only text under 12 px allowed, because they are decorative. |
| No glyphs | No robot/bot icon overlays, no crown, no square tinted indigo tiles. A badge on an avatar is only the presence dot (§7) or the paused mark (micro `pause` in a 14 px `rounded-full bg-gray-800 text-amber-200` disc). |

---

## 9. Callouts

App: `lib/components/Callout.svelte` (composes shadcn `Alert`).

| Kind | When | Classes |
| --- | --- | --- |
| **Quiet (neutral)** — default | Context that doesn't need action: "Features for this project come in a later release", "Sample data", "1 project has no team yet", "Shown from HQ". | `Callout`: `rounded-lg border border-white/10 bg-white/[0.03] px-4 py-3 text-sm/6 text-gray-300`, optional mini icon `size-5 text-gray-400` (`information-circle`) |
| **Attention** | Only when the owner must act or something is broken right now: live updates paused, an office unreachable, an action failed. Max one per screen, at the top of the content, under the header. | Same shape, plus a leading micro/mini icon in the tone colour (`exclamation-triangle text-amber-300` or `x-circle text-red-300`). Text stays `text-gray-300`/`text-white`; the **border and fill stay neutral**. Action as a ghost/outline button at the end. `role="status"` (or `alert` for a failure the user just caused); never both a live role and a toast for the same event. |
| **Banner (shell-wide)** | The live-feed "paused" banner only. Full-bleed strip under the top bar. | `flex items-center gap-x-3 border-b border-white/5 bg-black/10 px-4 py-2 text-sm/6 text-gray-300 sm:px-6 lg:px-8` + amber icon + "Retry" ghost button |

Never: yellow/brown/amber-filled boxes, amber text paragraphs as notes, tinted borders, two callouts stacked.

---

## 10. Buttons

shadcn-svelte `Button` (`ui/button`). Size: default `h-9` (36 px), `px-3`, `text-sm font-semibold`, `rounded-md`, `gap-x-1.5`, icons Heroicons mini `size-5 -ml-0.5`. Prototype primary: `rounded-md bg-indigo-500 px-3 py-2 text-sm font-semibold text-white`.

| Variant | Use | Classes (dark) | Contrast |
| --- | --- | --- | --- |
| `default` (primary) | The one main action of a view or dialog (Add agent, Create task, Pause Noor Haddad). | `bg-indigo-500 text-white hover:bg-indigo-500/80` + focus §17 | 4.58; hover 5.1+ (**Fix**: prototype `hover:bg-indigo-400` drops white text to 3.12) |
| `outline` (secondary) | Other actions next to a primary, toolbar actions (Add person, Expand all). | `bg-white/5 text-white ring-1 ring-inset ring-white/10 hover:bg-white/10` (no shadow) | 12.65 |
| `ghost` | Tertiary, inline, icon-only (close, kebab, Retry in a banner). | `text-gray-300 hover:bg-white/5 hover:text-white` | 9.98 |
| `destructive` | Offboard, Remove, Delete — in the confirming dialog. | `bg-red-400/10 text-red-300 ring-1 ring-inset ring-red-400/20 hover:bg-red-400/20` | 6.75 (**Fix**: current `text-destructive` red-400 on red/20 is 3.86) |
| `link` | Inline text action ("View all →", "Show all 24 entries"). | `rounded-sm text-sm/6 font-semibold text-indigo-400 hover:text-indigo-300` | 4.71 |

Rules: one primary per view; verbs that name the object ("Offboard ada", not "OK"); a disabled button keeps its label and gets a visible reason (`aria-describedby`, `text-xs/5 text-gray-400` under it) — or isn't shown; touch targets ≥ 44 px below 640 px (`max-[639px]:h-11`); no "…" chips as buttons (`Pause…` is a real outline button, and the ellipsis means "opens a dialog").

---

## 11. Inputs and form controls

shadcn-svelte `Input`, `Label`, `Checkbox`; native `<select>` styled as below (Tailwind Plus pattern) until a rich select is needed (then shadcn `Select`).

| Control | Classes |
| --- | --- |
| Text input / textarea | `block w-full rounded-md bg-white/5 px-3 py-1.5 text-base sm:text-sm/6 text-white outline-1 -outline-offset-1 outline-gray-500 placeholder:text-gray-400 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-indigo-400` · height 36 px |
| Select | same box + `appearance-none pr-8`, mini `chevron-down` `size-5 text-gray-400` in the same grid cell, right 8 px |
| Checkbox | shadcn `Checkbox`: `size-4 rounded-sm border border-gray-500 bg-white/5 checked:bg-indigo-500 checked:border-indigo-500` |
| Label | `Label`: `block text-sm/6 font-medium text-white`; "(optional)" `font-normal text-gray-400` |
| Help text | `mt-2 text-xs/5 text-gray-400` (`aria-describedby`) |
| Error | `aria-invalid="true"`, outline `outline-red-300`, message `mt-2 text-sm/6 text-red-300` with mini `exclamation-circle`, linked by `aria-describedby`. |
| Layout | Label → 8 px → control; fields 24 px apart (`space-y-6`); two short fields side by side from 640 px (`sm:grid-cols-2 gap-x-6`). |

The control border is gray-500 (3.04:1 against the page) so the field is findable (WCAG 1.4.11). The prototype's `outline-white/10` (1.36:1) is not used for form fields.

---

## 12. Tabs and secondary navigation

shadcn-svelte `Tabs` (`npx shadcn-svelte add tabs`) for in-page views; plain links with `aria-current="page"` when each tab has its own URL. Visual source: P-agent secondary nav, plus an underline so "selected" isn't colour-only.

| Part | Classes |
| --- | --- |
| Bar | `flex gap-x-6 overflow-x-auto border-b border-white/10 px-4 sm:px-6 lg:px-8` |
| Tab | `-mb-px border-b-2 border-transparent px-1 py-4 text-sm/6 font-semibold whitespace-nowrap text-gray-400 hover:border-white/20 hover:text-white`. The `px-1` keeps the inset focus ring (§17) clear of the label (decided 11 Nov, S2 validation). |
| Selected | `border-indigo-400 text-white` (`aria-selected="true"` / `aria-current="page"`) |
| Not available yet | Not shown. (If one must be shown, `text-gray-400 cursor-not-allowed` + `aria-disabled` + tooltip with the reason; never gray-500.) |
| Count | `ml-2 rounded-full bg-white/5 px-2 py-0.5 text-xs font-medium text-gray-300` |

**Agents | Org chart.** On `/agents` and `/org-chart`, directly under the top bar, before the page header: a link tab bar "Agents" (→ `/agents`) and "Org chart" (→ `/org-chart`). The sidebar keeps **Agents** current for both. The page header follows with the view's title and count ("All agents · 8 agents · 1 paused"; "Org chart · 5 teams · 8 agents · 3 people"). Org chart's own views (Teams / People / Reporting lines) are a second-level shadcn `Tabs` in the header's trailing slot, styled as the filter control (§5) or as this underline bar inside the content — not as a filled pill group.

---

## 13. Dialogs, sheets, menus, toasts, tooltips

| Component | Use | Classes (on the shadcn part) |
| --- | --- | --- |
| `Dialog` | Confirmations and short forms (≤ 5 fields). | Overlay `fixed inset-0 bg-gray-900/80` (no blur). Content `rounded-lg bg-gray-800 p-6 outline-1 -outline-offset-1 outline-white/10 sm:max-w-md` (`sm:max-w-lg` for forms), no shadow. Title `text-base/7 font-semibold text-white`; description `mt-2 text-sm/6 text-gray-400`; footer `mt-6 flex justify-end gap-x-3` (primary last); close = ghost icon button, mini `x-mark`. |
| `Sheet` (right) | Longer forms and detail you return from (Add agent, Task, Request, Office details). | Overlay as Dialog. Panel `w-full max-w-md bg-gray-800 ring-1 ring-white/10` (`max-w-2xl` for detail), header `px-6 py-6 border-b border-white/5`, body `px-6 py-6 space-y-6`, sticky footer `border-t border-white/5 px-6 py-4 flex justify-end gap-x-3`. |
| `Sheet` (left) | Mobile navigation drawer (§20). | P-layout: overlay `bg-gray-900/80`; panel `max-w-xs` (320 px) with the sidebar inside, close button in the 64 px strip to its right (`size-6 text-white`, outline `x-mark`). |
| `DropdownMenu` | Sort, filter, row actions. | Content `w-40`–`w-56 rounded-md bg-gray-800 py-2 outline-1 -outline-offset-1 outline-white/10` (no shadow); item `px-3 py-1 text-sm/6 text-white data-highlighted:bg-white/5`; checked mark mini `check size-4 text-indigo-400`; destructive item `text-red-300`; separator `my-1 h-px bg-white/5`. |
| `Sonner` toast | Outcome of an action the user started ("Added Noor Haddad."). | P-layout toast: bottom-right (`sm:justify-end px-4 py-6 sm:px-6`), `max-w-sm rounded-lg bg-gray-800 p-4 outline-1 -outline-offset-1 outline-white/10`; icon outline 24 `size-6` (`check-circle text-green-400`, `information-circle text-indigo-400`, `exclamation-triangle text-amber-300`); title `text-sm font-medium text-white`; body `mt-1 text-sm text-gray-400`; dismiss ghost `x-mark`. Not for errors that block: use an attention callout. |
| `Tooltip` | Labels for icon-only controls (rail items) and reasons for disabled ones. | `rounded-md bg-gray-950 px-3 py-2 text-xs text-white` (current rail tooltip), `side="right"` in the rail, delay 150 ms. Never the only place information lives. |

Focus moves into an opened dialog/sheet and back to its trigger on close (shadcn handles it; keep it). Escape closes. One overlay at a time.

---

## 14. Typography

Inter Variable (`@fontsource-variable/inter`, already in `app.css`). Mono: the system mono stack (`font-mono`), used only for ids, hashes and code in content.

| Token | Size / line height | Weight | Use |
| --- | --- | --- | --- |
| `text-[0.625rem]` | 10 / – | 600 | Decorative avatar and project-chip initials only (`aria-hidden`) |
| `text-xs/4` | 12 / 16 | 500–600 | Pills, nav count badge |
| `text-xs/5` | 12 / 20 | 400 | Meta lines, help text, feed time |
| `text-xs/6` | 12 / 24 | 400–600 | Header count line, sidebar section labels |
| `text-sm/6` | 14 / 24 | 400 body · 500 controls · 600 titles, nav | Body, rows, buttons, nav, table cells |
| `text-base/7` | 16 / 28 | 600 | Page `<h1>`, section `<h2>`, logo name, dialog titles |
| `text-2xl` / `text-4xl` | 24 / 32 · 36 / 40 | 600, `tracking-tight` | Stat numbers only (P-agent stats) |
| `text-3xl` | 30 / 36 | 600, `tracking-tight` | Full-screen error and onboarding titles only (P-error) |

Rules: text inputs `text-base` below 640 px (no iOS zoom), `sm:text-sm/6` above. Nothing smaller than 12 px except decorative initials. Truncate single-line titles with `truncate` + `title`; never break words mid-letter (`[overflow-wrap:anywhere]` only on ids/paths). Sentence case everywhere. Numbers that change use `tabular-nums`.

---

## 15. Colour tokens

Dark only (`<html class="dark">`, `color-scheme: dark`). Tailwind palette names are the design values; the shadcn variables in `app.css` map onto them. **Already correct in v1 unless marked Change.**

| shadcn variable | Value | Tailwind | Hex (sRGB) | Use |
| --- | --- | --- | --- | --- |
| `--background` | `var(--color-gray-800)` | gray-800 | #1e2939 | Page, top bar, dialogs |
| `--foreground` | white | white | #ffffff | Titles, primary text |
| `--card`, `--popover` | gray-800 | gray-800 | #1e2939 | Same as page: cards are separated by outline, not fill |
| `--primary` | indigo-500 | indigo-500 | #615fff | Primary button fill, checked controls, Logo |
| `--primary-foreground` | white | | | Text on primary (4.58) |
| `--secondary` | white / 10% | | #343e4c over page | Outline button hover |
| `--muted` / `--accent` | white / 5% | | #293443 | Hover, current nav item, highlighted menu item |
| `--muted-foreground` | gray-400 | gray-400 | #99a1af | Meta, labels, placeholders (5.64) |
| `--accent-foreground` | white | | | |
| `--border` | white / 10% | | | Section borders, outlines of panels/menus |
| (divider) | white / 5% | | | Row dividers, header and top-bar bottom border — use `border-white/5` |
| `--input` | **Change** → `var(--color-gray-500)` | gray-500 | #6a7282 | Form control borders (3.04 vs page) |
| `--ring` | **Change** → `var(--color-indigo-400)` | indigo-400 | #7c86ff | Focus ring (4.71 page, 4.29 current nav item) |
| `--destructive` | **Change** → `var(--color-red-300)` | red-300 | #ffa2a2 | Destructive text, error text and borders (7.64) |
| `--sidebar` | black / 10% | | #1b2533 | Sidebar and feed column |
| `--sidebar-border` | white / 5% | | | Sidebar ring |
| `--sidebar-accent` | white / 5% | | | Current/hover nav item |
| `--sidebar-primary`, `--sidebar-ring` | indigo-500 / **Change** indigo-400 | | | |
| `--radius` | 0.5rem | | | `rounded-md` = 6.4 px ≈ prototype 6 px; `rounded-lg` 8 px |

Accent text: links and selected marks `indigo-400` (4.71); text on tinted indigo `indigo-300`. Status tones: §7 table only. No other hues in chrome (no violet, no department tints, no blue `#…` literals). Raw hex values never appear in components; use the Tailwind names or the variables.

---

## 16. Spacing and sizes

4 px grid (Tailwind units). Values used by the prototype:

| Value | Use |
| --- | --- |
| 2 (8 px) | Nav item padding, pill horizontal padding, gap in title separators |
| 2.5 (10 px) | Meta-line gap |
| 3 (12 px) | Title-line gap, title→meta (`mt-3`), header title↔count gap |
| 4 (16 px) | Row vertical padding, mobile gutter, footer avatar gap |
| 5 (20 px) | Sidebar block gap (`gap-y-5`) |
| 6 (24 px) | Tablet gutter, header vertical padding ≥ 640, tab gap, dialog padding, form field gap |
| 7 (28 px) | Sidebar section gap |
| 8 (32 px) | Desktop gutter, section spacing (`space-y-8`) |
| 11 (44 px) | Section top padding after a border (`pt-11`) |
| 12 / 24 (48 / 96 px) | Empty-list padding / empty-page padding (`py-12` / `py-24`) |

Fixed sizes: sidebar 288 (`w-72`), rail 64 (`w-16`), feed 384 (`w-96`), top bar 64/56, control 36, mobile target 44, nav item 40, row 88 (two lines), pill 24, avatar 24/32/40, icon 24 (nav), 20 (in controls), 16/14 (in pills).

---

## 17. Focus

One focus style everywhere: `focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400`. Inset (`-outline-offset-2`) where the outline would be clipped: sidebar items, list rows, menu items, tabs. `:focus` without `-visible` never shows a ring on click. The ring is never removed without a replacement. shadcn components get their `focus-visible:ring-3 ring-ring/50` replaced by the outline above (the 50% ring is ~2:1). Skip link "Skip to content" first in tab order (`rounded-md bg-indigo-500 px-3 py-2 text-sm font-semibold text-white`, P-layout).

---

## 18. Contrast

Computed from Tailwind v4 OKLCH values converted to sRGB, alpha composited over gray-800 (#1e2939). Minimum: text 4.5:1, large text (≥ 18.66 px bold / 24 px) 3:1, UI boundaries, icons that carry meaning and focus 3:1.

**Text on surfaces**

| Text | gray-800 page | sidebar black/10 | white/5 (hover, current, muted) | white/10 | Verdict |
| --- | --- | --- | --- | --- | --- |
| white | 14.68 | 15.47 | 12.65 | 10.76 | ✔ |
| gray-300 | 9.98 | 10.51 | 8.59 | 7.31 | ✔ |
| gray-400 | 5.64 | 5.95 | 4.86 | 4.14 ✘ | ✔ except on white/10 |
| gray-500 | 3.04 ✘ | 3.20 ✘ | 2.62 ✘ | 2.23 ✘ | **Not for text**; borders/decorative only |
| gray-600 | 1.94 ✘ | 2.05 ✘ | 1.67 ✘ | 1.42 ✘ | Never |
| indigo-300 | 7.31 | 7.70 | 6.30 | 5.36 | ✔ |
| indigo-400 | 4.71 | 4.96 | 4.05 ✘ | 3.45 ✘ | ✔ on page and sidebar only |
| indigo-500 | 3.21 ✘ | 3.38 ✘ | 2.76 ✘ | | Fill only, never text |
| amber-200 / 300 / 400 | 11.78 / 10.15 / 8.55 | | 10.15 / 8.75 / 7.37 | | ✔ |
| red-300 / red-400 | 7.64 / 5.08 | | 6.58 / 4.37 ✘ | | Use red-300 for text |
| green-400 | 8.27 | 8.72 | 7.12 | | ✔ |
| sky-400 | 6.73 | | 5.79 | | ✔ |

**Pills** (text on tone/10 over page / over white/5): see §7 table; all chosen pairs ≥ 5.4. Rejected prototype pairs: indigo-400 on indigo/10 4.07, red-400 on red/10 4.48, gray-400 on gray/10 over white/5 4.12.

**Buttons:** white on indigo-500 4.58; on indigo-500/80 hover > 5.1; white on indigo-400 3.12 ✘ (prototype hover, rejected); white on white/5 outline button 12.65; red-300 on red-400/10 6.75; white on red-500 3.82 ✘.

**Non-text (3:1):** focus indigo-400 vs page 4.71, vs current nav item 4.29 ✔ (indigo-500: 3.21 / 2.92 ✘ on current item); input border gray-500 vs page 3.04 ✔ (white/10: 1.36 ✘); green-400 dot 8.27; amber-400 dot 8.55; gray-400 dot/icon 5.64; gray-500 dot 3.04 (borderline, replaced by gray-400). Dividers (white/5) and pill rings are decorative: they don't carry information, so they're exempt; every pill also has its words.

---

## 19. Motion

| Motion | Rule |
| --- | --- |
| Working pulse | §7, 2 s, only on the Working dot. |
| Spinners | Heroicons `arrow-path` + `animate-spin` for in-flight actions and transitional statuses; `motion-reduce:animate-none`. The word ("Checking…", "Saving…") is always there. |
| Overlays | shadcn fade/zoom 100 ms is fine; no slide longer than 200 ms. |
| Hover/press | Colour changes only, no movement (drop shadcn's `active:translate-y-px`). |
| Reduced motion | `@media (prefers-reduced-motion: reduce)`: no pulse, no spin, overlays appear without zoom, `scroll-behavior: auto`. |

---

## 20. Narrow widths

The v1 breakpoints stay (they're tested and match the 960 px minimum window of the desktop app); the prototype's own `xl` drawer breakpoint is **not** used.

| Width | Shell | Content |
| --- | --- | --- |
| ≥ 1280 | Sidebar 288 + content; Agents home shows the feed column (§2). | Gutters 32 |
| 960–1279 | Sidebar 288 + content; feed stacks under the list. | Gutters 32 (≥1024) / 24 |
| 640–959 | **Rail**: `w-16`, icons only, each item `size-12` centred, label `sr-only`, right-side `Tooltip` with the name; logo becomes a 48 px link; Projects section hidden; footer hidden (only the logo switch, which goes to `/welcome`). | Gutters 24 |
| < 640 | **Top bar + Menu drawer**: no rail; top bar 56 px with a 44 px **Menu** button (opens the left `Sheet` with the full sidebar, §13); search collapses to a 44 px icon button that expands over the bar; primary action becomes an icon button with the label as `aria-label` + tooltip. | Gutters 16; tables → rows; meta facts beyond the 2nd hide; pills wrap below the title instead of squeezing |

Also: 200% text zoom must not clip or overlap (rows wrap, pills keep whole words). Minimum window 960 × 640 in Tauri; the web build supports down to 320 px.

---

## 21. Empty, loading and error states

| State | Pattern | Source / classes |
| --- | --- | --- |
| Empty page / not built yet | Header strip (§5) with the page title, then a centred block: outline 24 icon `mx-auto size-12 text-gray-400`; `mt-2 text-sm font-semibold text-white` title; `mt-1 max-w-sm mx-auto text-sm text-gray-400` sentence; `mt-6` one primary button if there's a real next step. No "Coming soon" pill. | P-soon: `px-4 py-24 text-center sm:px-6 lg:px-8` |
| Empty list | One centred row (§6). Words say what this place is for and the next action: "No agents yet. Add your first agent." | P-home |
| No search results | "No agents match “{query}”." + polite live region with the count. | P-home `sr-only aria-live` |
| Loading (first load) | Skeleton rows in the real layout: same row padding, `h-4 w-48 rounded bg-white/5` bars, `animate-pulse motion-reduce:animate-none`, and an `sr-only` "Loading agents…". After 2 s, a words line ("Still loading…"). | |
| Loading (action) | The button keeps its width, shows a spinner and the -ing verb ("Pausing…"), `aria-busy`. | |
| Error (section) | Attention callout (§9) in place of the content: what happened, why if known, the next step as a button ("Try again"), copyable details in a `<details>`. | |
| Error (route) | Full page: `py-24 text-center`; status `text-base font-semibold text-indigo-400`; `mt-4 text-3xl font-semibold tracking-tight text-white` message; `mt-6 text-sm/6 text-gray-400` explanation; `mt-8` **primary button** "Back to agents". | P-error |
| Unavailable (browser build) | Same as route error with the product logo above, one sentence and the command in an inline code span; no nav list, no card. | |
| Paused live updates | Shell banner (§9). | |

---

## 22. Icons

**Migration (Eden, 2026-11-11):** chrome (sidebar, top bar, page headers, pills, buttons, menus) uses Heroicons now. Lucide icons still inside screen content may stay until that screen is restyled in BUG-70; every restyled screen moves to Heroicons. Don't mix the two sets on one screen.

- Set: **Heroicons v2** (the prototype's set; MIT). Outline 24 (`stroke-width 1.5`) for nav, empty states, toasts; solid 20 "mini" for icons in buttons, inputs, menus, chevrons; solid 16 "micro" for pills and meta. Paths live in `lib/components/icons.ts` (extend it, or add `@heroicons` SVGs inline); no new `@lucide/svelte` imports. Existing Lucide uses are migrated screen by screen (gap list).
- Decorative icons are `aria-hidden="true"`; an icon-only control has an `aria-label` and a tooltip.
- Colour: `text-gray-400` by default, `text-white` when its control is current/hovered, a tone colour only when it reinforces a status word next to it.
- Mapping for common needs: add `plus`, close `x-mark`, more `ellipsis-vertical`, back `chevron-left`, open `chevron-right`, sort `chevron-up-down`, select `chevron-down`, search `magnifying-glass`, info `information-circle`, warning `exclamation-triangle`, error `x-circle`, success `check-circle`, pause `pause`, sleep `moon`, person `user`, people `users`, team `user-group`, office `building-office`, project `folder`, task `clipboard-document-list`, session `command-line`, activity `signal`, settings `cog-6-tooth`, chat `chat-bubble-left-right`, thread `chat-bubble-bottom-center-text`, mention `at-symbol`, pinned `bookmark`, attach `paper-clip`, starred/needs you `bell`, lock `lock-closed`, owner `user-circle`.

---

## 23. Vocabulary (kept from v0.1, updated to v1)

| Term | Means | Don't say |
| --- | --- | --- |
| Organization | One workspace: the whole AI company | Company, tenant |
| Owner | The human running the organization | Admin, user (in the UI) |
| Agent | An AI employee with a home, memory and backlog | Bot, assistant, worker |
| Person | A human member of the organization | User |
| Team | A group with a lead; teams own projects | Department (in v1 UI), squad |
| Project | A body of work with its own brief and teams | Workspace, initiative |
| Office | Where agents run: HQ, or another office; "This Mac" names the computer | Server, node, host, machine (in the UI) |
| Session | A run of an agent (Claude Code, Codex…) | — |
| Chat | Messages between the owner, people and agents: channels, DMs, threads | Session |
| Task · Bug · Request | Work items on a project | Ticket, issue, assignment |
| Goal | The completion condition an agent sets for a task | Objective, OKR |
| Add / Offboard | Bring an agent into the organization / remove it (its records are kept) | Create / delete agent |
| Needs you | Waiting on the owner | Pending, action required |

**Not a product concept:** the subagents an agent starts internally. The app never shows them (user decision, 2026-11-07).

**Status meanings** (truth rule): "Active" is the admin flag, never a status word. Until the office reports live runs, an on-call agent without a run is **Asleep · wakes when called**, an agent a person starts is **Started by a person**, everything else **Status unknown** (PROF-1, RUN-2).

---

## 24. Writing and content rules

- Sentence case; no trailing period on titles, buttons, pills.
- Dates: relative in rows ("5m", "2h ago"), absolute in details ("Sep 14, 2026"). `<time datetime>` always.
- Ids, hashes and messaging names appear in `font-mono` only in detail views (`<code class="rounded-sm bg-white/5 px-1 py-px font-mono text-[0.8em] text-gray-300">`, P-inline), never as filters or buttons.
- Confirmations name the object and effect; the primary button repeats the verb ("Offboard ada").
- Errors: what happened, why, what to do. No raw stack traces as the message.

---

## 25. Do / don't

| Do | Don't |
| --- | --- |
| Full-bleed divided rows with a meta line | Rounded tinted cards around lists, tables, sections or single rows |
| One pill per row, trailing | Pill + coloured word + icon for the same status; several pills per row |
| Quiet neutral callout for context | Yellow/brown/amber-filled boxes; amber text paragraphs as notes |
| Sort/filter as text button + DropdownMenu | Code-style chips (`from:@name`, `has:thread`) as clickable filters in normal UI; segmented pill groups for filters |
| Heroicons with words | Emoji as UI icons (⭐ 🧵 👥 📌 📎 📣 🔁 ❓ ✅ in chrome); Lucide for new work; robot/crown glyphs |
| Round gray-800 avatars with tinted initials | Square indigo tiles, gradient avatars |
| gray-400 minimum for text | gray-500/600 text, indigo-400/500 text on tinted or white/5 fills |
| Page title in the page header | Page title in the top bar plus another title in the content |
| Hide what isn't built yet, or explain it once | Rows of disabled tabs ("Debt", "Docs", "Insights"…) and "Coming soon" pills |
| shadcn Dialog/Sheet with `bg-gray-900/80` overlay | Backdrop blur, glows, drop shadows on dark |
| `focus-visible` indigo-400 outline | Removing outlines; 50 % rings |
| Real counts and states | Invented metrics, "Working" without a live run |

---

## 26. Component checklist (what to reuse)

| Need | Use | Prototype source |
| --- | --- | --- |
| Shell frame, top bar, toast | `(shell)/+layout.svelte`, `ShellTopbar`, Sonner | P-layout |
| Sidebar | `Sidebar.svelte` + `nav.ts` | P-sidebar |
| Page header | (new) `PageHeader.svelte`: title, count, trailing snippet | P-home header |
| Row | `DividedRow.svelte` (+ title/meta snippets) | P-home `li` |
| Status | `StatusPill.svelte` with a `tone` prop (§7), `StatusDot` (copy P-dot) | P-badge, P-dot |
| Avatar | (new) shared `Avatar.svelte` replacing the chat avatar and MemberTile letters | P-avatar |
| Callout | `Callout.svelte` (+ `tone="attention"` icon colour only) | — |
| Empty / not built | (new) `EmptyState.svelte` | P-soon |
| Error page | `routes/+error.svelte` | P-error |
| Tabs | shadcn `Tabs` (add) | P-agent nav |
| Menus | shadcn `DropdownMenu` | P-home sort menu |
| Forms | shadcn `Input`, `Label`, `Checkbox`, styled native `select` | Tailwind Plus forms |
| Dialog / Sheet | shadcn `Dialog`, `Sheet` | P-layout drawer |
| Icons | `icons.ts` (Heroicons) | P-icons |

---

## Product decisions behind this standard (Eden, 2026-11-11)

- Navigation (owner): Agents (Home), Chat, Projects, Work, Sessions, Activity, Settings. Org chart is a tab on Agents, Offices sits under Settings, Processes is hidden. No route moves (Rowan): `/agents`, `/org-chart`, `/offices` and `/processes` keep working.
- Agent rows open the existing agent details sheet in v1.0. A standalone profile page (`/agents/[id]`, as in the prototype) is a later follow-up, not v1.0.
- "Status unknown" is neutral (gray), not amber. Amber means only "waiting on the owner".
- A single-word pill too long for its row truncates with an ellipsis, keeping its full text as the accessible name and `title`. Multi-word pills wrap the row, never the pill.
- Last resort (decided 11 Nov, BUG-76 validation): if a pill is still wider than its row after the row has wrapped, it truncates with an ellipsis whatever its word count, keeping the full text as the accessible name and `title`. A pill is never clipped without an ellipsis (its container must let it shrink: `min-w-0 max-w-full` on the pill, `truncate` on the label) and never breaks mid-word.
- Page content uses the same gutters as the bars above it (`px-4 sm:px-6 lg:px-8`, §2), never a fixed `px-8`, so the tab bar, the page and the top bar line up at every width (decided 11 Nov, S2 validation). On Offices this also lets short statuses (Online, Starting, Invited, Offline, Stopped) sit beside their icon at 320 px with 200% text.
- Lists come before boards: Project tasks default to divided rows grouped by status; a board view is optional, behind "View".
- Saved views and filters use the "Show ▾" menu in the page header (§5), not a left list or segmented pills.
- Top-bar search (`Search agents…`) filters the Agents list, so from any other page it opens `/agents?q=`.
- The mobile drawer's close button may stay inside the panel: one 44 px target.
- Features not built yet keep the existing spec behaviour: a disabled control says why ("Available in a later release"), and whole areas show the unavailable page. Don't hide them silently.
- Inputs use a gray-500 border (3.04:1) instead of white/10 (1.36:1), to meet 3:1.
- The product name stays as v1 ships it. The logo block shows the organization's name.
