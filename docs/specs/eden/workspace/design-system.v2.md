# Design system · Agency v1

Owner: Eden · Status: v2 (IA v2) · 2026-11-11 (v1.0 for BUG-70, amended for IA v2). Replaces `design-system.md` v0.1/v0.2 in full. The lavender "Look and feel v0.2", its department tints and its token tables are **withdrawn**.

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

**v2 (11 Nov 2026): information architecture v2.** This amendment folds in IA v2: Emery's PRD rev 3 (`ac-product-manager-emery/knowledge/prds/information-architecture-v2.md`, decisions D9–D19), my review (`knowledge/research/review-ia-v2.md`, page fixes H, I, T, M, the presence table, the D8 positions and the addendum) and the stories (`ia-v2-stories.md`, AC ids `AC-IA01-…` to `AC-IA16-…`). **IA v2 supersedes the nav freeze rev 4** (Agents as landing, Org chart as a tab of Agents, `/settings` holding only Offices); Rowan's nav freeze rev 5 carries the routes. Sections changed: §1 (two clauses), §2 frame and Home, §3 sidebar, §4 top bar, §5 page headers (profile and team variants), §6 list rows, §7 presence and project health, §8 avatars, §9 decision card, §10 and §13 examples, §12 tabs, §15 and §16 one line each, §17 one exception (focused headings), §18 one contrast line, §20 narrow widths, §21 states, §22 icons, §23 vocabulary, §25, §26 and the product decisions. Untouched sections keep their v1.0 text. Prototype for the new pages: `ac-product-manager-emery/knowledge/prototypes/ia-v2/index.html` (draft 2), cited as P2.

---

## 1. Principles

1. **Calm.** A dark gray canvas, one indigo accent, thin white/5 dividers, muted meta text. Colour is reserved for state that matters. No gradients, no glows, no shadows on dark surfaces (`dark:shadow-none` everywhere in the prototype).
2. **Roomy.** Rows are 16 px top and bottom with 12 px between title and meta (88 px for a two-line row). Headers are 24 px top and bottom from 640 px. Don't compress to fit more columns; move detail into the meta line or the detail page.
3. **Flat.** Lists are full-bleed divided rows, not cards. No rounded tinted boxes around lists, tables or sections. A surface change (`bg-black/10`, `bg-gray-700/10`) marks a region (sidebar, profile header), never a single item. Exceptions (v2): Home's Needs you and Projects sections use outlined cards (§2), because they hold few items of mixed data, and the single owner card heads the Teams index and the Org chart (§12); it is the one card with a fill (`bg-white/[0.03]`), because it stands for the owner, not a list item.
4. **One status signal per row.** A row shows its status once: one pill at the trailing edge, optionally with the matching dot before the title. Never a pill and a coloured word and an icon for the same fact. Other facts (severity, kind, version, team) go in the meta line as plain text.
5. **Words carry meaning.** Every status, count and state is written in words. Colour and icons only reinforce them (WCAG 1.4.1).
6. **Truth over decoration.** Show only state the system really has. The prototype's "Working" and its pulse are allowed only when the office reports a live run; until then the v1.0 truth rule (§7) applies: Paused, Needs you and "Working on {task}"; "Status unknown" only once, on the profile header. No invented metrics, no fake actions: a control that does nothing yet is shown disabled with its reason, or not at all.
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
| Page gutters | 16 / 24 / 32 px, the same as the top bar and the tab bars, at every width. Every full-bleed row, header, section and page body uses them so text lines up down the page. Never a fixed `px-8` (decided 11 Nov, S2 validation). | `px-4 sm:px-6 lg:px-8` |
| Max width | Lists, tables and feeds are full-bleed (no max width). Prose is capped: descriptions `max-w-3xl`, journal/notes `max-w-4xl`, empty-state copy `max-w-sm`. Forms inside a page `max-w-2xl`. Search field `max-w-xl`. | |
| Sections | Separated by a top border and space, not by cards. | `border-t border-white/10 pt-11`, section title `<h2 class="text-base/7 font-semibold text-white">` with gutters |
| Section header with a link | Title left, one link right ("Open Inbox (7) →", "All projects →", "View all →"). Throughout this document a trailing "→" in a link label is notation for the chevron icon, never literal text. | `flex items-center justify-between px-4 sm:px-6 lg:px-8`; link §10 `link` + mini `chevron-right` `size-4` (`aria-hidden`), `inline-flex items-center gap-1 max-[639px]:min-h-11` |
| Two-pane pages | Inbox (list + detail), profile (content + Setup aside), project Overview: side by side from 1280 px, stacked below; the Inbox is side by side from 960 px, and below 960 px it shows the list only and an item opens as its own view (§13), never stacked under the list. The aside has `border-l border-white/5` beside, `border-t` when stacked. | Aside `w-[340px]` (`xl:grid-cols-[1fr_340px]`); Inbox container `min-[960px]:grid min-[960px]:grid-cols-[minmax(260px,2fr)_minmax(0,3fr)] xl:grid-cols-[minmax(320px,440px)_minmax(0,1fr)]` (at 960 px that's about 270 / 400 px, so the detail keeps room for its actions), list `border-r border-white/5` |

**Home (landing page, D1).** Route `/home`; `/` redirects to it (D19). It replaces the Agents home and its feed column. Source: P2 `pageHome`.

| Part | Rule |
| --- | --- |
| Header | §5: title **Home**, count line from real data only: "{N} items need you · {N} active projects". Omit "{N} items need you" until the Inbox exists (IA-09), and add "{N} working" only once presence is reported (§7, §1.6). No page primary: the top bar's New (outline) is enough. |
| 1 · Needs you | Section title with a leading mini `bell` `text-amber-400` and the link "Open Inbox ({N}) →" to `/inbox`. Up to 5 decision cards (§9), newest first, in a grid: `grid gap-4 px-4 sm:px-6 lg:px-8 grid-cols-[repeat(auto-fill,minmax(min(300px,100%),1fr))]`. Card buttons are outline only (H2, AC-IA04-4). Apart from the sidebar's Inbox badge, this is the only amber on Home (plus the section's bell icon and the warning icon of an attention callout, toast or banner): no other section uses amber (Happening now included, below). |
| 2 · Projects | Link "All projects →". One outlined card per active project, same grid. Card: `relative flex flex-col gap-2.5 rounded-lg p-4 outline-1 -outline-offset-1 outline-white/10 hover:bg-white/[0.025]`. Line 1: name (`text-sm/6 font-semibold text-white`, stretched link to `/projects/{id}`; the focus ring goes on the `absolute inset-0` span, inset and `rounded-[inherit]`, as §6 rows) and the health pill only when health is set (§7, D15). Line 2: progress = done / total work items: a bar (`h-1.5 rounded-full bg-white/10`, fill `bg-indigo-400`, `role="progressbar" aria-valuemin="0" aria-valuemax="{total}" aria-valuenow="{done}" aria-label="{name} progress" aria-valuetext="{done} of {total} work items done"`; no bar when total is 0) and "{done} of {total} done" `text-xs/5 tabular-nums text-gray-400`; no work items: "No work items yet". Line 3: "{N} open · {N} blocked", then "Due {date}" only when set. Line 4: up to 5 member avatars (24 px, no presence dot, overlapped `-space-x-1`, "+N" after 5, names in an `sr-only` list). (Open, Emery: a "Lead {name}" fact; AC-IA04-3 and PRD §6 ask for avatars only.) |
| 3 · Happening now | Link "Members →" to `/teams/members`. Divided rows (§6) of members who are working, With you included (a session with the owner is work; the row still shows Working, because the section is about who is working, not their top-precedence state): avatar with the **Working** dot only (never the Needs-you or With-you dot here, even when the member also has an open Inbox item; that item is already in section 1), name, role, line "Working on {task} · {project}", and `<span class="sr-only">(Working)</span>` after the avatar. **No pill** on these rows: the section title already says they're working (H5). In v1.0 (truth rule) the section shows only its empty state (§21): no rows, no dots. |
| 4 · Recent activity | Link "View all →" to `/activity`. Feed rows (§6), the latest 6. |
| Layout | Sections 1 and 2 full width. Sections 3 and 4 side by side from 1280 px (`xl:grid-cols-2`, the second with `xl:border-l xl:border-white/5`), stacked below. Each section has its empty, loading and error state (§21); with v1.0 data the empty states are the default view, so design them first (H4). |

---

## 3. Sidebar

Source: P-sidebar. App: `lib/components/Sidebar.svelte`, `lib/nav.ts`.

**Items, in this order (IA v2, PRD §4 as amended by D10–D19; AC-IA01-1).** Three blocks plus a bottom item. No group label repeats an item's name: the second block has no label (it held a "Work" item under a "Work" label in P2 draft 1).

| # | Block | Label | Route | Icon (Heroicons outline 24) | Current when (`aria-current="page"`) |
| --- | --- | --- | --- | --- | --- |
| 1 | — | Home | `/home` | `home` | `/home` |
| 2 | — | Inbox + count badge | `/inbox` | `inbox` | `/inbox…` |
| 3 | (no label) | Projects, then "Recent" sub-list | `/projects` | `folder` | `/projects`; a project page marks its Recent sub-item instead, or Projects when it isn't in Recent |
| 4 | (no label) | Work | `/work` | `clipboard-document-list` | `/work…` |
| 5 | (no label) | Chat | `/chat` | `chat-bubble-left-right` | `/chat…` |
| 6 | (no label) | Activity | `/activity` | `signal` | `/activity…` (a run's log lives here) |
| 7 | Organization | Teams, then the teams sub-list | `/teams` | `user-group` | `/teams`, `/teams/members`, `/teams/org-chart` and any `/members/{id}` (a member profile marks Teams, AC-IA01-4); a team page and its `/teams/{id}/members` mark the team's sub-item instead, or Teams when the team isn't listed. Wherever the sub-lists are hidden (the 640–959 px rail), the parent item, Projects or Teams, is current instead |
| 8 | bottom (`mt-auto`, above the footer) | Organization settings | `/settings` | `cog-6-tooth` | `/settings…` (General, Offices, Agent setup, Roles, Processes, Members and access, Updates) |

Old routes redirect and keep their query string (D19, AC-IA01-3): `/` → `/home`, `/agents` → `/teams/members`, `/org-chart` → `/teams/org-chart`, `/offices` → `/settings/offices`, `/processes` → `/settings/processes` (Rowan, 11 Nov; the target stays "Not in this release" in production, so the redirect never makes Processes usable there), `/sessions` → `/activity`. The nav never links to an old route. Not in the nav: Members and Org chart (views of Teams, §12), Offices and Agent setup (Organization settings), Processes (Organization settings, "Not in this release" in production in v1.0).

| Element | Rule | Classes |
| --- | --- | --- |
| Panel | 288 px wide, its item list scrolls on its own, slightly darker than the page. | `flex grow flex-col gap-y-5 overflow-hidden bg-black/10 px-6 ring-1 ring-white/5`; the panel itself doesn't scroll: the item `<ul>` scrolls and Organization settings and the footer sit outside it, pinned (§3 Sub-lists) |
| Logo block | 64 px tall (96 px with `pt-8` in the Tauri window so it clears the traffic lights, and it is a drag region). Logo mark 32 px + name. The name is the **organization name** (v1 decision), truncated with a `title`. | `flex h-16 shrink-0 items-center gap-x-3` · Logo `h-8 w-auto` · name `text-base/7 font-semibold text-white truncate` |
| Group label | "Organization" only. A sub-list label ("Recent" under Projects) uses the same classes, indented with the sub-items. | `text-xs/6 font-semibold text-gray-400` |
| Section list | Groups 28 px apart; items 4 px apart; items bleed 8 px into the gutter. The outer `<ul>` is the one that scrolls (§3 Sub-lists); it carries the bleed as padding so nothing is clipped and it never scrolls sideways. | `<ul class="-mx-2 flex min-h-0 flex-1 flex-col gap-y-7 overflow-x-hidden overflow-y-auto px-2">`, inner `<ul class="-mx-2 mt-2 space-y-1">` |
| Item | 40 px tall (24 px line + 8 px padding), 44 px in the phone drawer (§20), radius 6 px, 12 px icon gap. Sub-items take the same `max-[639px]:min-h-11`. | `group flex gap-x-3 rounded-md p-2 text-sm/6 font-semibold max-[639px]:min-h-11` |
| Item default | Gray text and icon. | `text-gray-400 hover:bg-white/5 hover:text-white`; icon `size-6 shrink-0 text-gray-400 group-hover:text-white` |
| Item current | Filled with white/5, white text and icon, `aria-current="page"`. No indigo fill, no pill, no glow. | `bg-white/5 text-white`; icon `text-white` |
| Item icon | Heroicons outline 24, `stroke-width="1.5"`, `aria-hidden`. | `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">` |
| Count badge (Inbox only) | Open Inbox items waiting on the owner, the same count as Home's "Open Inbox (N)" (AC-IA09-4). Shown only when the count is > 0. Solid amber, dark number; the number is `aria-hidden` and an `sr-only` ", {N} waiting on you" follows the label. Max one per item. Chat no longer has a badge. | `ml-auto rounded-full bg-amber-300 px-1.5 text-xs/4 font-semibold text-gray-950` (13.9:1) |
| Focus | Inset ring so it isn't clipped. | `focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-indigo-400` (**Fix**: prototype indigo-500 is 2.9:1 on the current item) |

**Sub-lists (Projects "Recent", Teams).** Under Projects: a "Recent" label, then the 5 projects with the most recent activity (D8 as amended: "Recent", not "Pinned"; the owner didn't pin them), then "View all projects" (always shown, AC-IA01-1). Under Teams (inside the Organization group): up to 5 teams, A–Z, then "View all teams" (always shown, AC-IA01-1). The caps keep the list short, but they don't fit everything in a short window (about 1,100 px with both lists full), so the item list scrolls on its own (`min-h-0 flex-1 overflow-y-auto` on the `<ul>`) while Organization settings and the footer stay pinned below it, always on screen at any window height (800 px included). Each sub-item links by id (`/projects/{id}`, `/teams/{id}`, D19) and is current on that page. Sub-item = item classes above, with a 24 px initial chip instead of an icon:

`flex size-6 shrink-0 items-center justify-center rounded-lg border border-white/10 bg-white/5 text-[0.625rem] font-medium text-gray-400 group-hover:border-white/20 group-hover:text-white` (`aria-hidden`), then `<span class="truncate">{name}</span>`.

The initial is decorative; the name is the accessible text. The Teams sub-list is a `<ul aria-label="Teams">`; the Recent sub-list is labelled by its "Recent" label. "View all …" items: `text-gray-400`, no chip, link to `/projects` or `/teams`. None: `<li class="px-2 text-sm/6 text-gray-400">No projects yet</li>` / "No teams yet".

**Footer: owner, org switcher and office.** Bottom of the panel (`-mx-6`, under Organization settings), in the prototype's "You · Owner" style. The owner is never a team member (D17); this footer is one of the three places the owner appears. Two sibling links, never nested (decided with Rowan, 2026-11-11):

| Part | Rule and classes |
| --- | --- |
| Owner link (the switcher) | Goes to `/welcome` (switch organization). Keeps today's behaviour. Its accessible name starts with the visible text (WCAG 2.5.3): `aria-label="You · Owner, switch organization"`. `flex w-full items-center gap-x-4 px-6 py-3 text-sm/6 font-semibold text-white hover:bg-white/5` + inset focus. |
| Avatar | Owner initials, §8 `size-8`: `flex size-8 items-center justify-center rounded-full bg-gray-800 text-xs font-semibold text-indigo-300 outline -outline-offset-1 outline-white/10` |
| Owner text | `You <span class="font-normal text-gray-400">· Owner</span>`, as in the prototype; the link's accessible name is "You · Owner, switch organization" (v2; was "Switch organization" in Rowan's freeze 6c7dc9f, which fails Label in Name now that the text is visible). |
| Office line | Under it, its own link, a block row: "{This Mac} · {HQ} office · {Online}" (AC-IA01-2), `flex min-h-6 items-center px-6 pb-3 max-[639px]:min-h-11 text-xs/5 font-normal text-gray-400 hover:text-white` + inset focus, going to `/settings/offices`. The status word is the office's own status word (§7 office rows), so it reads "Offline" or "Status unknown" when that's true; accessible name "{This Mac}, {HQ} office, {online}. Open Offices". |
| Rail (640-959 px) | Only the logo switch; no footer text. The sub-lists are hidden; the Inbox badge stays on the icon: the badge's tone and size classes without `ml-auto`, placed `absolute -top-1 -right-1` on a `relative` rail item. |

---

## 4. Top bar

Source: P-layout. App: `lib/platform/shell/ShellTopbar.svelte`.

| Element | Rule | Classes |
| --- | --- | --- |
| Bar | 64 px (56 px below 640 px), sticky, bottom divider, same colour as the page. | `sticky top-0 z-40 flex h-16 shrink-0 items-center gap-x-6 border-b border-white/5 bg-gray-800 px-4 sm:px-6 lg:px-8` (`max-[639px]:h-14`) |
| Menu button | **Below 640 px only** (§20), never at 640 px and up (P2 draft 1 showed it at 1280). 44 px target. | Heroicons mini `bars-3` `size-5` centred in `-m-3 inline-flex size-11 items-center justify-center rounded-md text-white min-[640px]:hidden` (44 px); `aria-label="Open navigation"` |
| Search field | Left, grows, max 576 px, 36 px tall. `/` focuses it (not while typing, not with the drawer open). Placeholder "Search projects, work, members…". Use shadcn `Input` inside a `<form role="search" class="relative …">` (the results panel is positioned against it); the input is a combobox (`role="combobox" aria-autocomplete="list" aria-expanded aria-controls aria-activedescendant="{active option id}"`); the panel is `role="listbox"`, each result `role="option"` with `aria-selected`. Below 640 px the field is replaced by a 44 px icon button (`aria-label="Search"`) that opens a full-width field over the bar (AC-IA13-1): the same form, shown when open with `max-[639px]:grid max-[639px]:absolute max-[639px]:inset-0 max-[639px]:z-10 max-[639px]:h-full max-[639px]:max-w-none max-[639px]:rounded-none max-[639px]:bg-gray-800` in place of `max-[639px]:hidden` (opaque, so nothing shows through), so the results panel comes with it. It has a trailing 44 px ghost close button (mini `x-mark`, `aria-label="Close search"`). Focus moves into the input on open and back to the Search icon button on close and on Escape (with no results list open); choosing a result navigates, and focus goes to the new page's `<h1>` (§5); the field never shrinks to a stub. | Form: `relative grid h-9 max-w-xl flex-1 grid-cols-1 rounded-md bg-white/5 outline-1 -outline-offset-1 outline-gray-500 has-[input:focus-visible]:outline-2 has-[input:focus-visible]:outline-indigo-400 max-[639px]:hidden`. Input: `col-start-1 row-start-1 size-full bg-transparent pr-10 pl-9 text-base sm:text-sm/6 text-white placeholder:text-gray-400 outline-hidden`. Icon: mini `magnifying-glass` `ml-2.5 size-5 self-center text-gray-400`. Key hint: `<kbd class="rounded-sm border border-white/10 px-1 font-sans text-xs text-gray-400 max-[639px]:hidden">/</kbd>` (no hint on phones; the open narrow form is `max-[639px]:grid-cols-[1fr_auto]` with the close button in the second column) (`aria-hidden`; the input has `aria-keyshortcuts="/"`). **Fix**: prototype `bg-white/8` + `outline-white/10` + gray-500 placeholder fail (§18). |
| Search results (D16) | Typing opens a listbox under the field, grouped **Projects · Work · Members** (and **Conversations** once they're indexed; until then the group isn't shown). Up to 5 results per group; a group with none is left out. Arrow keys move, Enter on a result navigates to it, Escape closes. Each option is `py-2` and `max-[639px]:min-h-11`. Last row: "See all results" → `/search?q={query}` (same groups, full lists). Result counts go to a polite `sr-only` live region ("6 results"). No results: "Nothing matches “{query}”." | Panel `absolute top-full mt-2 w-full rounded-md bg-gray-800 py-2 outline-1 -outline-offset-1 outline-white/10` (no shadow); group label `px-3 pt-2 text-xs/6 font-semibold text-gray-400`; option `flex items-center gap-x-3 px-3 py-2 text-sm/6 text-white aria-selected:bg-white/5 aria-selected:outline-2 aria-selected:-outline-offset-2 aria-selected:outline-indigo-400` (focus stays in the input, so the active option needs the ring, not only the fill) with a 24 px avatar or outline icon (`folder`, `clipboard-document-list`) and a gray secondary phrase |
| Links | Only links that open something real. Spec / Design / Help from the prototype are **not** shipped until they have a destination. | `text-sm/6 font-semibold text-white hover:text-gray-300` |
| Divider | Between links and the New menu, from 1024 px. | `hidden lg:block h-6 w-px bg-white/10` |
| New menu | **New ▾** as an **outline** button (§10), so a page's own primary stays the only primary on screen. Opens a `DropdownMenu`: Task, Project, Message, separator, Hire an agent, Invite a person (mini icons `clipboard-document-list`, `folder`, `chat-bubble-left-right`, `user-plus`, `user-plus`). Hire an agent opens the existing Describe → Suggestion → Review → Hiring flow (D14). Below 640 px it becomes a 44 px icon button with `aria-label="New"` and a tooltip. Items whose flow doesn't exist yet are shown disabled with their reason (§10). | `Button variant="outline"` + `gap-x-1.5`, mini `plus` `-ml-0.5 size-5`, label, mini `chevron-down` `size-4 text-gray-400`; `aria-haspopup="menu"` |
| Page title | Not in the top bar. The page `<h1>` is in the page header (§5). (v1 currently puts it here; see the gap list.) | |

---

## 5. Page headers

Source: P-home header, P-soon header, P-agent section headings.

```
<header class="flex items-center justify-between border-b border-white/5 px-4 py-4 sm:px-6 sm:py-6 lg:px-8">
  <div class="flex items-baseline gap-x-3">
    <h1 tabindex="-1" class="text-base/7 font-semibold text-white">Members</h1>
    <p class="text-xs/6 text-gray-400">12 members · 3 people · 9 agents</p>
  </div>
  <!-- trailing: sort/filter controls or a secondary action -->
</header>
```

| Part | Rule |
| --- | --- |
| Title | `text-base/7 font-semibold text-white` (16/28). The one `<h1>` of the page, focus target after navigation (`tabindex="-1"`). Focus placed there by the app shows **no ring**: `:is(h1,h2)[tabindex="-1"]:focus { outline: none }` in `app.css`, on every page (H3; the `h2` covers the Inbox detail heading from 960 px, §13). The ring is for keyboard moves between controls (§17); the h1 isn't a control. No 24–32 px page titles inside the shell. |
| Count line | Optional, baseline-aligned, `text-xs/6 text-gray-400`. Real counts only, joined with ` · `. Singular/plural correct ("1 agent"). |
| Description | If the page needs a sentence of context, put it **under** the header strip as the first content line, `px… pt-4 text-sm/6 text-gray-400 max-w-3xl`, or drop it. Not a paragraph in the header. |
| Sort control | Text button + menu (shadcn `DropdownMenu` with `RadioGroup`): `flex items-center gap-x-1 rounded-sm text-sm/6 font-medium text-white max-[639px]:min-h-11 max-[639px]:px-2` → "Sort by <span class="text-gray-400">status</span>" + mini `chevron-up-down` `size-5 text-gray-400`. Menu width `w-40`, checked item shows mini `check` `size-4 text-indigo-400`. |
| Filter control | Same pattern as sort: "Show <span class="text-gray-400">all</span>" + DropdownMenu radio items with counts (Members: All · People · Agents, D10; Inbox: one menu with a Project group and a Team group, I3; Work: its views). Not a segmented pill group, not code chips, not native selects in a row. |
| Actions | At most one primary and one outline button, plus a "⋯" menu for the rest (§10, M1). The top bar's New is outline, so a page-specific primary ("Message" on a profile, "Save changes" in settings) is the only primary on screen. Most list pages have no primary: one outline button ("Hire or invite ▾", "New task") at most. |
| Sample data | When the organization is sample data, a gray pill **Sample data** (§7 gray) after the title, once per page. Not repeated in sub-sections. |
| Breadcrumb / back | Detail pages start with one back link above the header, naming where it goes: "← Projects", "← Teams", "← Members", "← Agent setup", "← Inbox" (narrow Inbox, §13). `inline-flex items-center gap-x-1 px-4 pt-4 text-sm/6 font-semibold text-gray-400 hover:text-white max-[639px]:min-h-11 sm:px-6 lg:px-8`, mini `chevron-left` `size-4`. Not a "Projects › Atlas" text breadcrumb above a second title. |


**Profile header** (member profile `/members/{id}`, the same for people and agents; M1, M2, M4, AC-IA06-1). Source: P2 `pageMember`.

```
<header class="flex flex-wrap items-start gap-4 border-b border-white/5 bg-black/10 px-4 py-6 sm:px-6 lg:px-8">
  <Avatar size="10" presence />                       <!-- 40 px, §8 -->
  <div class="min-w-0 flex-1 basis-60">
    <h1 tabindex="-1" class="flex flex-wrap items-baseline gap-x-2 text-base/7 font-semibold text-white">
      Quinn <span class="text-sm/6 font-normal text-gray-400">Senior Rust Developer</span></h1>
    <p class="mt-0.5 flex flex-wrap items-center gap-x-2.5 text-xs/5 text-gray-400">Agent · Platform, Office · Reports to ada</p>
    <p class="mt-2.5 flex flex-wrap items-center gap-x-2.5 text-sm/6 text-gray-300"><StatusPill …/> Working on T-118 · Agency v1</p>
  </div>
  <div class="flex flex-wrap items-center gap-3"><!-- actions: the buttons themselves wrap at narrow widths and 200% text --></div>
</header>
```

| Part | Rule |
| --- | --- |
| Avatar | 40 px with its presence dot (§8). The dot's word is the pill beside it, so the avatar stays `aria-hidden`. |
| Name and role | Name 16 px (`text-base/7 font-semibold`), role beside it on the same baseline `text-sm/6 font-normal text-gray-400`; wraps under the name when narrow. |
| Meta line | `{Agent or Person}` · `{teams, linked}` · `Reports to {manager, linked}` (`Reports to You`, plain text, for the owner's reports). Facts separated by the 2 px dot (§6). The kind is a word, never a shape or glyph. |
| Status | **One** presence pill and **one** line (M2), never the same fact twice. The line says what the pill can't: Working → "Working on {task} · {project}"; Needs you → "Waiting on you: {item}" (links to the Inbox item); With you → "In a session with you" ("In a session with {name}" when it's someone else); Paused → "Its work waits until it's resumed"; Asleep → "Wakes when called". With no current task, no "Working on" line; Overview reads "Not working on anything right now" (§21). Status unknown (v1.0): the pill reads "Status unknown" **once**, and this is the only place unknown appears (lists show nothing, §7); the line shows "Working on {task}" when `current_task_id` is known, otherwise nothing. Agents only in v1.0: a person's profile shows no presence pill and no status line until people have presence (D12); the owner never has presence (§7). |
| Actions | Agent: **Message** (primary, mini `chat-bubble-left-right`), **Assign task** (outline), and a "⋯" outline icon button (mini `ellipsis-horizontal`, `aria-label="More actions for {name}"`) opening a `DropdownMenu`: Pause or Resume, Edit, separator, Offboard… (`text-red-300`, opens the confirm dialog). Person: Message (primary) and "⋯" holding Edit (diverges from AC-IA06-1, which keeps the More menu for agents; per my review M1; to raise with Emery). Open (Emery, Rowan): whether the owner gets a profile page at all. P2 draft 2 has one at `/members/you`, but D17 doesn't list it and D19 keys member URLs on ids; until it's decided, "Reports to You" is plain text, not a link. Never four buttons in a row. Open (Emery, AC-IA06-1): the story offers Assign task for people too; this spec gives people Message only until a person can take a work item. |
| Narrow | Below 640 px the actions wrap under the name, each `max-[639px]:h-11`; the "⋯" icon button is `max-[639px]:size-11`. |

**Team header** (`/teams/{id}`; T-fixes, AC-IA05-2). The standard header (§5) with:
- Title: the team name.
- Count line: "{N} members · {N} people · {N} agents" (real counts, plural-correct).
- Description line under the strip (`pt-4 text-sm/6 text-gray-400 max-w-3xl`): the purpose, then "Lead: {name, linked} ({Agent|Person})".
- Trailing: one outline button **Hire or invite ▾** (mini `user-plus`, mini `chevron-down`) opening a `DropdownMenu`: Hire an agent, Invite a person. No primary. (Decided 11 Nov: the label from AC-IA05-2 and P2 draft 2 says what the menu does, using the D14 words; it replaces "Add member ▾" from my review.)
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
| Row actions | Not inline buttons in every row. Use a trailing `DropdownMenu` kebab (mini `ellipsis-vertical`, `size-5 text-gray-400`, 36 px target, `max-[639px]:size-11` below 640) or the detail page. | |
| Empty list | One `<li>` centred: `px-4 py-12 text-center text-sm text-gray-400 sm:px-6 lg:px-8`, e.g. "No members match “sam”." | |
| Paths and ids | Home folders, file paths and ids in rows, meta lines and detail lists wrap anywhere instead of pushing the page sideways (M3). | `[overflow-wrap:anywhere]` (plus `min-w-0` on the flex child) |

**Feed row** (Activity, Home Recent activity, profile and team Activity tabs): `px-4 py-4 sm:px-6 lg:px-8`; line 1 `flex items-center gap-x-3`: avatar `size-6` (no presence dot on Home's Recent activity, keeping Home's only amber in section 1, §2; elsewhere the dense-place rule of §7 applies), actor `flex-auto truncate text-sm/6 font-semibold text-white` (link `hover:underline` to the member profile), time `<time class="flex-none text-xs text-gray-400">5m</time>` (**Fix**: prototype gray-600 is 1.9:1); line 2 `mt-3 truncate text-sm text-gray-400` with a `title` holding the full text. A run entry expands in place (a disclosure button, `aria-expanded`) to show its log summary: trigger, duration, outcome (AC-IA12-2).

**Member row** (Members directory, team page Members, Happening now; AC-IA02-2). Replaces the v1.0 "Agent row content".
- Leading: 32 px avatar with its presence dot (§8). The dot is `aria-hidden`; the pill or `sr-only` text carries the word.
- Title: `{name}` (stretched link to `/members/{id}`), then the secondary `{role}`.
- Meta line, in this order: `{Agent|Person}` · `{team}` · `Reports to {manager}` (plain text "You" for the owner's reports, §5) · `Working on {task}` (only when known). On the team page, drop the team fact (T4). Below 640 px: kind and "Working on" only; "Working on" is exempt from the `hidden md:block` rule for later facts (§6), at every width.
- Trailing: **one** presence pill (§7), then a chevron. An unknown presence never shows a pill here (AC-IA08-2, §7), only the "Working on …" fact when known. On Happening now, no pill (H5).
- The owner is never a member row (D17); the owner appears as the owner card (§12).

**Work row** (Work, team Work tab, profile Work tab, project Work; T1, AC-IA05-3).
- Leading: 32 px avatar of the assignee **without** a presence dot (or an empty avatar for "Unassigned").
- Title: the id in the code span (`rounded-sm bg-white/5 px-1 py-px font-mono text-xs text-gray-300`, §24), then the title.
- Meta line: `{project}` · `{assignee name}` (or "Unassigned") · `{priority}` (tasks: "{P} priority"; bugs: the severity word, §7).
- Trailing: **one** pill: the work's own status (§7 Task/Bug/Request chips). It shows **Needs you** (amber) instead, once, only when the item is waiting on the owner. Never the assignee's presence, never a second pill.

**Team row** (Teams index `/teams`, under the owner card; AC-IA05-1). Divided rows, never cards (D8 position). Leading 32 px initial chip (as the sidebar chip, `size-8 text-xs`); title `{team}`; a purpose line `mt-1 text-sm/6 text-gray-400 line-clamp-2`; meta "Lead {name}" · "{N} members · {N} people · {N} agents" · "Projects: {names}" (or "No active projects"); trailing avatar stack (no presence dots; hidden below 640 px) and chevron. No status pill: a team has no status.

**Agent setup row** (`/settings/agents`, below 640 px, AC-IA07-1): title name + role; meta runtime · model · how it runs · office; no presence here (AC-IA07-1 asks for health, not presence; presence lives on Members and the profile). Open (Emery, AC-IA07-1): agent "health" has no defined data or words yet; until it does, no health pill and no trailing cell. From 640 px this is a table with the columns name · role · runtime · model · how it runs · office (below).

**Tables.** Use a real `<table>` only for data people compare across columns (task backlog, bug list). Prototype P-agent table: `w-full text-left`; `thead` `border-b border-white/10 text-sm/6 text-white`, `th` `py-2 font-semibold`; `tbody divide-y divide-white/5`; cells `py-4 text-sm/6`, first cell gutters `pl-4 sm:pl-6 lg:pl-8`; secondary columns `hidden md:table-cell` / `lg:table-cell`; long text `line-clamp-2|3` with a `title`. No rounded enclosure, no zebra stripes. **Below 640 px every table becomes divided rows** (title + meta line + one trailing pill), never a sideways-scrolling table (M3: Agent setup's 6-column table scrolled 177 px at 390 px). Render both and switch with `max-[639px]:hidden` / `min-[640px]:hidden`, or render rows only below 640.

---

## 7. Status pills and dots

Source: P-badge, P-dot, P-status. App: `lib/components/StatusPill.svelte` (composes shadcn `Badge`), `AgentStatusIcon.svelte`, `OfficeStatusBadge.svelte`.

**Pill shape.** `inline-flex flex-none items-center gap-x-1.5 rounded-full px-2 py-1 text-xs/4 font-medium ring-1 ring-inset` + a tone. 24 px tall. The text never wraps letter by letter: `whitespace-nowrap`; when space runs out the **row** wraps the pill under the title, the pill never shrinks below its text (fix for the v1 390 px roster). Optional leading icon: Heroicons micro 16 at `size-3.5`, `aria-hidden`. Task and work-item chips use `rounded-md` instead of `rounded-full` (P-status `TASK_CHIP`), same tones.

**Tones** (dark values; contrast on `gray-800` / on a hovered row, §18):

| Tone | Classes | Text contrast | Note |
| --- | --- | --- | --- |
| indigo (active, in progress, working, with you) | `bg-indigo-400/10 text-indigo-300 ring-indigo-400/30` | 6.32 / 5.47 | **Fix**: prototype `text-indigo-400` is 4.07 |
| gray (neutral, waiting, paused, asleep; wire `Idle` is Available, green) | `bg-gray-400/10 text-gray-300 ring-gray-400/20` | 8.43 / 7.28 | StatusPill default (prototype gray-400 measured 4.11 on tinted rows) |
| outline (off, ended) | `bg-transparent text-gray-400 ring-gray-400/20` | 5.64 / 4.86 | |
| green (done, online, available, on track) | `bg-green-500/10 text-green-400 ring-green-500/20` | 6.98 / 6.05 | |
| amber (needs you; office update needed) | `bg-amber-400/10 text-amber-200 ring-amber-400/30` | 9.62 / 8.30 | **Amber pills, dots and badges mean only "waiting on the owner"** (D8). Paused is gray (v2). Never amber for anything the owner needn't act on. The warning icon in an attention callout, toast or banner (§9, §13) and the Needs-you `bell` (Home's section title, the Needs you pill) are the only other amber uses, as icons only; the bell, too, means waiting on the owner |
| red (stopped, unreachable, blocked, critical, at risk, off track) | `bg-red-400/10 text-red-300 ring-red-400/20` | 6.75 / 5.84 | **Fix**: prototype `text-red-400` is 4.48 |
| sky (starting, checking) | `bg-sky-400/10 text-sky-300 ring-sky-400/20` | 7.37 / 6.39 | Transitional states only |

**Presence: one vocabulary (v2).** One component (`PresencePill` + the avatar dot, IA-08) maps the wire `MemberPresence` enum and the member status to these labels. It is used by the Members directory, team pages, profiles, Org chart nodes, chat avatars and the Inbox detail; Chat's own presence words (Working · Online · Asleep · Offline) are removed (AC-IA08-3). Highest precedence first: when two hold at once, the higher one shows. The one exception is Home's Happening now, which always shows Working (§2 Home, H5).

| # | Label (exact words) | When | Pill tone | Icon (Heroicons micro) | Avatar dot | Wire (`MemberPresence` / member status) |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Needs you | At least one open Inbox item from this member is waiting on the owner | amber | `bell` | amber-400, filled | `NeedsYou`; in v1.0 derived from the member's open Inbox items |
| 2 | With you · With {name} | A person has taken over the agent's session: "With you" for the owner, "With {name}" for another person. Replaces the v1 "Started by a person" (RUN-1) | indigo | `user` | indigo-400, filled, no pulse | `WithYou` |
| 3 | Working | The office reports a live run | indigo | `bolt` | indigo-400, filled, with the pulse when motion is allowed | `Busy` |
| 4 | Paused | The owner paused the agent. Say "Paused", never "Paused by you", unless the data records who paused it | gray | `pause` | gray-300 pause mark (§8), not a dot | member status `paused` |
| 5 | Available | Agent: live session, idle. Person: signed in | green | `check-circle` | green-400, filled | `Idle`, `Online` |
| 6 | Asleep | An on-call agent that isn't running; the profile line adds "Wakes when called" | gray | `moon` | gray-400, filled | `Asleep` |
| 7 | Offline | An always-on agent with no session, or a person who is signed out | outline | `signal-slash` | hollow ring | `Offline` |
| 8 | Status unknown | No live data | outline | `question-mark-circle` | **none** (no dot; AC-IA08-2, which overrides the hollow ring in my IA v2 review table) | `Unknown`, or anything the office doesn't report |

Not presence: **Former** is a membership state (outline pill, micro `arrow-right-start-on-rectangle`), never shown as presence or mixed into the presence precedence; **"Started by a person"** becomes With you, or meta text where With you isn't reported; office words (Online, Checking…, Update needed…) stay office statuses (below). The owner has no presence pill.

**v1.0 truth rule (D12, review addendum).** Until the presence capability flag says the office reports runs (IA-15), nothing reports presence and the server sends `Offline` for everyone. So in v1.0:
- show **Paused** from the member status;
- show **Needs you** derived from the member's open Inbox items (once the Inbox exists, IA-09);
- show **Status unknown** for everything else; never Working, Available, Asleep, With you or Offline, and no dots except Needs you's and the paused mark;
- the member profile header shows "Status unknown" **once** (§5);
- Home's Happening now shows "Live status isn't available yet" (§21);
- humans other than the owner have no presence in v1.0 (D12): no pill, no dot.
When the flag turns on, the full table applies with no layout change.

**At any time (v1.0 and after):** in lists and rows (Members, team Members, Org chart nodes, chat), an unknown status shows **no pill at all**, only the "Working on {task}" fact when `current_task_id` is known (AC-IA08-2, §25). Row 8's outline pill is used only on the profile header.

**Never colour alone.** Every pill has its icon and its word. Every avatar dot has its word next to it (a pill, the profile status, an Org chart node's inline status) **or**, in dense places (feed, chat, board, avatar stacks; the Inbox list shows no dots, §9), the word in the accessible name (`<span class="sr-only">(Working)</span>` after the avatar) and in a `Tooltip` on the avatar ("{name} · {label}"). Working and Available differ by hue (indigo vs green), not only by the pulse; Paused has its own mark; Offline is a hollow ring and Status unknown has no dot. Working and With you share the indigo dot (only the pulse differs, and it stops under reduced motion), so the word is what tells them apart: it is always present, as above.

**Project health (D15, D8 position).** Shown only when health is set on the project; otherwise the card or row shows its open and blocked counts and no health pill. Used on Home project cards, the Projects list and the project header.

| Health | Tone | Icon (micro) |
| --- | --- | --- |
| On track | green | `check-circle` |
| At risk | red | `exclamation-triangle` |
| Off track | red | `x-circle` |

The icon tells the two reds apart; the word is always there.

**Every other status the app uses:**

| Domain | Status (exact words) | Tone | Icon (Heroicons micro) | Dot |
| --- | --- | --- | --- | --- |
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
| Bug severity | Critical · Major · Minor · Trivial · Severity not set | **meta text**, not a pill (one status per row). Critical gets mini `exclamation-triangle` `text-red-300` before the word; Major gets mini `exclamation-circle` `text-gray-300` (decided 11 Nov: amber means only waiting on the owner, so Major is no longer amber; the icon and the word carry it). | | |
| Request | Not routed · Sent to X · not accepted yet · X is scoping | gray | — | |
| Request | Scoped by X *(waiting for your Review)* | amber, word **Needs you** + meta "Scoped by X" (Open, Emery: requests aren't a D11 Inbox kind yet; either they join Approvals or this becomes a gray "Scoped" pill, because Needs you must always be an open Inbox item) | — | |
| Request | Approved | green | — | |
| Request | Declined · Merged · Changed | outline | — | |
| Goal | Active · Goal paused *(amber because it creates a Needs-you item in Inbox Problems, D11; not presence)* | indigo · amber | — | |
| Announcement reason (chat) | Delivered · Ready for review · Blocked · Report · Decision needed | meta text after the kind word, not a pill | | |
| Inbox item (card, detail) | Needs you (open) · Handled | amber · gray + micro `check-circle` | `bell` / `check-circle` | |
| Process *(hidden in v1.0)* | Sealed | outline + micro `lock-closed` | | |

**Dot** (P-dot, leading edge of a row): wrapper `flex-none rounded-full p-1` + tone, inner `size-2 rounded-full bg-current` (`size-1.5` small). Tones: working / with you `bg-indigo-400/10 text-indigo-400`; available `bg-green-400/10 text-green-400`; idle (asleep) `bg-gray-100/10 text-gray-400` (**Fix**: prototype gray-500 dot is 3.04, borderline; use gray-400); attention (needs you) `bg-amber-400/10 text-amber-400`; off (offline) `text-gray-400` with the inner dot `bg-transparent ring-1 ring-current ring-inset` (hollow, matching §8). Paused and Status unknown have no row dot. The dot is `aria-hidden`; the row's link carries `<span class="sr-only">, {status}</span>`. Use the dot only on the leading edge of rows whose trailing pill states the same status, or alone where the status word is right beside it. The avatar presence dot is specified in §8.

**Working pulse** (P-css): `@keyframes soft-pulse { 0%,100% { box-shadow: 0 0 0 0 rgb(124 134 255 / .45) } 50% { box-shadow: 0 0 0 5px rgb(124 134 255 / 0) } }` (indigo-400; was green-400 in v1.0), `@utility animate-soft-pulse { &::after { content: ""; position: absolute; inset: 0; border-radius: inherit; animation: soft-pulse 2s ease-in-out infinite } @media (prefers-reduced-motion: reduce) { &::after { animation: none } } }` (the pulse runs on a pseudo-element so it never replaces the dot's own `ring-2` box-shadow; the class sets no `position`: the dot itself must be positioned in the markup, `absolute` on an avatar dot (§8) or `relative` on a row dot (P-dot), which gives the pseudo-element its box; never put the class on a row). `motion-reduce:animate-none` on the element does **not** stop it; the media rule above does. Only on the indigo Working dot. Nothing else pulses; With you is indigo with no pulse.

---

## 8. Avatars and initials

Source: P-avatar.

| Rule | Classes |
| --- | --- |
| Round for everyone (agents and people alike); the kind is said in words ("Agent"), not shape. | `inline-flex flex-none items-center justify-center rounded-full bg-gray-800 font-semibold outline -outline-offset-1 outline-white/10` |
| Sizes | 24 px `size-6 text-[0.625rem]` (feed, chat compact, project chips, Home card avatar stacks, Inbox list) · 32 px `size-8 text-xs` (footer, chat messages, member and work rows, Org chart nodes) · 40 px `size-10 text-sm` (profile header, Inbox detail, owner card). 40 px is the largest avatar in the app (M4: the prototype's 56 px is not used). |
| Initials | First letters of the first two words, upper case; one word → first letter (`initials()`). |
| Colour | Only the letter is tinted, picked by a stable hash of the id from 7 tints: `text-indigo-300 emerald-300 sky-300 fuchsia-300 rose-300 violet-300 teal-300` (`avatarColor()`); unknown → `text-gray-300`. All ≥ 7:1 on gray-800 (fuchsia-300 ≈ 8.3). **v2:** `fuchsia-300` replaces `amber-300` in the same slot, so the hash and every other avatar's colour stay the same; amber is reserved for Needs you. |
| Decorative | `aria-hidden="true"`; the name is always written next to it. The 10 px initials are the only text under 12 px allowed, because they are decorative. |
| No glyphs | No robot/bot icon overlays (`cpu-chip` is never an avatar, §22), no crown, no square tinted indigo tiles. A badge on an avatar is only the presence dot or the paused mark (below). |
| Presence dot | Bottom-right, `absolute -right-px -bottom-px rounded-full ring-2 ring-gray-800` (the ring separates it from the avatar; on the darker sidebar panel use `ring-sidebar`). Size by avatar: 24 px → 8 px `size-2`; 32 px → 10 px `size-2.5`; 40 px → 12 px `size-3`. Fill per §7: Needs you `bg-amber-400`; With you `bg-indigo-400`; Working `bg-indigo-400 animate-soft-pulse` (reduced motion handled in the class, §7); Available `bg-green-400`; Asleep `bg-gray-400`; Offline hollow `border-[1.5px] border-gray-400 bg-gray-800`; Status unknown: no dot. The avatar needs `relative`. Never the only place the state lives (§7 never colour alone). |
| Paused mark | Replaces the dot when Paused: micro `pause` 2 px smaller than its disc (`size-2` / `size-2.5` / `size-3`) `text-gray-300`, in a disc of the dot's size + 2 px (`size-2.5` on 24 px avatars, `size-3` on 32 px, `size-3.5` on 40 px), `absolute -right-px -bottom-px rounded-full bg-gray-700 ring-2 ring-gray-800` (same position as the dot). Gray, never amber (v2). |

---

## 9. Callouts

App: `lib/components/Callout.svelte` (composes shadcn `Alert`).

| Kind | When | Classes |
| --- | --- | --- |
| **Quiet (neutral)** — default | Context that doesn't need action: "Features for this project come in a later release", "Sample data", "1 project has no team yet", "Shown from HQ". | `Callout`: `rounded-lg border border-white/10 bg-white/[0.03] px-4 py-3 text-sm/6 text-gray-300`, optional mini icon `size-5 text-gray-400` (`information-circle`) |
| **Attention** | Only when the owner must act or something is broken right now: live updates paused, an office unreachable, an action failed. Max one per screen, at the top of the content, under the header. | Same shape, plus a leading micro/mini icon in the tone colour (`exclamation-triangle text-amber-300` or `x-circle text-red-300`). Text stays `text-gray-300`/`text-white`; the **border and fill stay neutral**. Action as a ghost/outline button at the end. `role="status"` (or `alert` for a failure the user just caused); never both a live role and a toast for the same event. |
| **Banner (shell-wide)** | The live-feed "paused" banner only. Full-bleed strip under the top bar. | `flex items-center gap-x-3 border-b border-white/5 bg-black/10 px-4 py-2 text-sm/6 text-gray-300 sm:px-6 lg:px-8` + amber icon + "Retry" ghost button |

Never: yellow/brown/amber-filled boxes, amber text paragraphs as notes, tinted borders, two callouts stacked.

**Decision card (v2).** One owner-waiting item from the Inbox (Approvals, Questions, Decisions, Problems; D11), on Home's Needs you and in the Inbox. App: (new) `DecisionCard.svelte`. Source: P2 `needsCard`, `inboxDetail`; AC-IA09-2.

| Part | Rule and classes |
| --- | --- |
| Container | Home: an outlined card, `relative flex flex-col gap-2.5 rounded-lg p-4 outline-1 -outline-offset-1 outline-white/10` (neutral). **Never a tinted border, top stripe or amber fill** (H1); the amber pill is the only signal. `<article aria-label="{kind} from {name}">`. In the Inbox list the item is a compact row (avatar 24, who · kind, time; the "what" line; meta), and the full card is the detail pane. |
| 1 · Who, kind, age | `flex flex-wrap items-center gap-2`: 24 px avatar (no dot), name (link to the profile, `text-sm/6 font-semibold text-white`), `· {kind} · {age}` in `text-xs/5 text-gray-400` (kind = the source kind: Hire request, Run approval, Permission request, Question, Escalation, Goal paused, Blocked on you, Run error, Crash), then the trailing pill **Needs you** (amber, micro `bell`), or **Handled** (gray, micro `check-circle`) once acted on. Detail pane: 40 px avatar, name · role · kind word, and the member's presence inline (no word when it's unknown, as in lists, §7). |
| 2 · What | One line, `text-sm/6 font-semibold text-white` (`<h3>` on Home, `<h2>` in the side-by-side detail from 960 px, the page's `<h1>` on `/inbox/{id}` below 960 px, §5), e.g. "Approve merging Kai's codec split into v1". |
| 3 · Why | The context: `text-sm/6 text-gray-300`, `line-clamp-3` on Home with a `title`; full in the detail. |
| 4 · Recommendation | Only when there is one: `flex gap-x-2 text-sm/6 text-gray-300`, mini `information-circle` `size-5 text-gray-400` (`aria-hidden`), then the text ("ada recommends: approve"). Not a callout, no fill. |
| 5 · Meta | Project (link) · work item id · expiry (micro `clock` + "Expires in 2h"), facts as in §6. The detail adds a definition list: Kind, Project, Work item, Expires ("No expiry"), Clears (the kind's clearing rule), Conversation. |
| 6 · Actions | Follow **D9**: Approvals show Approve and Deny only where the source operation exists (hire approval, run approvals), calling exactly that operation (AC-IA11-1); otherwise **Open**. Questions: **Reply** (inline field). Decisions: one button per option, or Open. Problems: Open, plus **Resume** for Goal paused; **Blocked on you** (work blocked on the owner, D11) shows Open only and isn't dismissable. **Dismiss** only on Problems that are a run error or a crash (D11); nothing else is dismissable. On Home and in lists every button is **outline** (§10). In the Inbox detail at most **one primary** (the recommended or first action); the rest outline, Open as a ghost link "Open {source}". A deny that needs a reason asks inline; a failure keeps the card open with the server's message (AC-IA11-2). Handled cards replace the actions with a plain `text-sm/6 text-gray-400` line "{outcome}" (the Handled pill already says it's handled; no second icon). |
| Grouping | Inbox list groups "Today" and "Earlier" (`<h2>` `px-4 pt-4 text-xs/6 font-semibold text-gray-400 sm:px-6 lg:px-8`). Selected item: `bg-white/5` + `aria-current="true"`. |

---

## 10. Buttons

shadcn-svelte `Button` (`ui/button`). Size: default `h-9` (36 px), `px-3`, `text-sm font-semibold`, `rounded-md`, `gap-x-1.5`, icons Heroicons mini `size-5 -ml-0.5`. Prototype primary: `rounded-md bg-indigo-500 px-3 py-2 text-sm font-semibold text-white`.

| Variant | Use | Classes (dark) | Contrast |
| --- | --- | --- | --- |
| `default` (primary) | The one main action of a view or dialog (Message on a profile, Create task, Pause Noor Haddad, the Inbox detail's recommended action). | `bg-indigo-500 text-white hover:bg-indigo-500/80` + focus §17 | 4.58; hover 5.1+ (**Fix**: prototype `hover:bg-indigo-400` drops white text to 3.12) |
| `outline` (secondary) | Other actions next to a primary, toolbar actions, the top bar's New ▾, Hire or invite ▾, Assign task, decision-card actions on Home. | `bg-white/5 text-white ring-1 ring-inset ring-white/10 hover:bg-white/10` (no shadow) | 12.65 |
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
| Not available yet | Not shown, unless its page explains itself (the profile's Notes and memory tab in v1.0, below). (If a disabled one must be shown, `text-gray-400 cursor-not-allowed` + `aria-disabled` + tooltip with the reason; never gray-500.) |
| Count | `ml-2 rounded-full bg-white/5 px-2 py-0.5 text-xs font-medium text-gray-300` |

**Which kind of tabs.** A tab with its own URL is a **link tab** (`<nav aria-label="…">` of `<a>`, `aria-current="page"` on the current one); an in-page filter that keeps the URL is shadcn **Tabs** (`aria-selected`). Both use the bar and tab classes above, with `px-1` kept on every tab. Counts use the Count class. Order of a page: back link, header (§5), then the tab bar.

| Where | Tabs (exact labels, in order) | Kind | Notes |
| --- | --- | --- | --- |
| **Teams view switch** (`/teams`, `/teams/members`, `/teams/org-chart`; AC-IA02-1) | Teams (count) · Members (count) · Org chart | Link tabs, `aria-label="Teams views"` | The merged S2 `AgentsTabs` component (v1 `673e1cb7`), relabelled and re-routed; same classes. Sits under the header. The sidebar keeps **Teams** current for all three. Headers: "Teams · {N} teams · {N} people · {N} agents"; "Members · {N} members · {N} people · {N} agents" with "Show ▾" All · People · Agents (D10); "Org chart · By reporting line · {N} members". |
| **Team page** (`/teams/{id}`; T2, AC-IA05-2) | Work (count, default) · Projects · Activity · Conversations | Link tabs | **Members sit above the tabs**, under the header: a 16 px section title "Members" with its count (T3: `text-base/7 font-semibold text-white`, not a 12 px gray band), member rows (§6) capped at **6**, then the link "All {N} members →" to `/teams/{id}/members` (per nav freeze rev 5: the Members directory filtered to that team, back link "← {team}") when there are more. |
| **Member profile** (`/members/{id}`; AC-IA06-2) | Overview · Work (count) · Conversations · Activity · Notes and memory | Link tabs | "Notes and memory" only on agents. In v1.0 it opens the quiet callout "Not available yet: notes and memory arrive in a later release." (D18, §21). The tab is shown, not disabled: the page explains itself. Overview: Working on (or "Not working on anything right now"), goal, recent activity; agents add the Setup aside (§2) with "Open in Agent setup →". |
| **Project page** (`/projects/{id}`) | Overview · Work (count) · Team (count) · Activity · Conversations | Link tabs | Overview: goal, progress (§2 bar), Needs you for this project ("Nothing needs you on this project."), recent activity. |
| **Inbox** (`/inbox`; I3, D11, AC-IA09-1) | All · Approvals · Questions · Decisions · Problems · Handled, each with its open count (Handled: its total) | shadcn **Tabs** (`aria-label="Filter by kind"`) | Project and team filters go in one "Show ▾" menu in the header (§5), not selects. |
| **Activity** (`/activity`) | All · Work · Conversations · Decisions · Runs | shadcn Tabs | Runs appears once runs are recorded (IA-15); until then the tab isn't shown. (Open for Emery: P2 draft 2 also has an "Org changes" tab that AC-IA12-1 doesn't; not in the standard until decided.) |
| **Organization settings** (`/settings/…`; AC-IA03-1) | General · Offices · Agent setup · Roles · Processes · Members and access · Updates | Link nav, `aria-label="Organization settings"` | Below 960 px: the underline bar above. From 960 px: a **left column of links** (`w-56 shrink-0 border-r border-white/5 px-4 py-6 flex flex-col gap-y-1`), each link styled as a sidebar item (§3: `rounded-md p-2 text-sm/6 font-semibold text-gray-400 hover:bg-white/5 hover:text-white`, current `bg-white/5 text-white`, mini icon `size-5`). Not a wrapping chip group. Roles, Processes, Members and access and Updates show "Not in this release" (§21) where not built; in production Processes shows the same "Not in this release" state rather than disappearing, so the sub-nav always has its 7 items (AC-IA03-1, AC-IA03-3). |

**Org chart (D8 position, AC-IA02-3).** A vertical tree replaces the v1.0 "Reporting lines" view; Teams and Members cover the other two old views (Teams / People), so the Org chart has no second-level tabs. Root: the **owner card** (`flex items-center gap-3 max-w-xl rounded-lg px-4 py-3 bg-white/[0.03] outline-1 -outline-offset-1 outline-white/10`, not a link and no hover until the owner-profile Open (§5 Actions) is decided, 40 px avatar, "You · Owner", line "Runs {org} · {N} members report to you: …"); the same card heads the Teams index. Children indent `ml-2 pl-3 sm:ml-5 sm:pl-6 border-l border-white/10` (20 px a level below 640 px) with a connector across the indent (`border-t border-white/10`, `w-3 sm:w-6`); each node is a link to the profile: 32 px avatar with dot, name, "{role} · {Agent|Person}", and the inline presence word (none when unknown, §7). `<ul aria-label="Reporting lines">` nesting, so screen readers hear the levels.

**Work** (`/work`): views go in "Show ▾" (Needs me, Mine, By project, By assignee) and List | Board is a two-button icon toggle (mini `list-bullet` / `view-columns`, `aria-pressed`, labels in `aria-label` + tooltip), not pill groups.

---

## 13. Dialogs, sheets, menus, toasts, tooltips

| Component | Use | Classes (on the shadcn part) |
| --- | --- | --- |
| `Dialog` | Confirmations and short forms (≤ 5 fields). | Overlay `fixed inset-0 bg-gray-900/80` (no blur). Content `rounded-lg bg-gray-800 p-6 outline-1 -outline-offset-1 outline-white/10 sm:max-w-md` (`sm:max-w-lg` for forms), no shadow. Title `text-base/7 font-semibold text-white`; description `mt-2 text-sm/6 text-gray-400`; footer `mt-6 flex justify-end gap-x-3` (primary last); close = ghost icon button, mini `x-mark`. |
| `Sheet` (right) | Longer forms and detail you return from (Hire an agent, Task, Request, Office details). | Overlay as Dialog. Panel `w-full max-w-md bg-gray-800 ring-1 ring-white/10` (`max-w-2xl` for detail), header `px-6 py-6 border-b border-white/5`, body `px-6 py-6 space-y-6`, sticky footer `border-t border-white/5 px-6 py-4 flex justify-end gap-x-3`. |
| `Sheet` (left) | Mobile navigation drawer (§20). | P-layout: overlay `bg-gray-900/80`; panel `max-w-xs` (320 px) with the sidebar inside, close button in the 64 px strip to its right (`size-6 text-white`, outline `x-mark`). |
| `DropdownMenu` | Sort, filter, row actions. | Content `w-40`–`w-56 rounded-md bg-gray-800 py-2 outline-1 -outline-offset-1 outline-white/10` (no shadow); item `px-3 py-1 max-[639px]:min-h-11 text-sm/6 text-white data-highlighted:bg-white/5 data-highlighted:outline-2 data-highlighted:-outline-offset-2 data-highlighted:outline-indigo-400`; checked mark mini `check size-4 text-indigo-400`; destructive item `text-red-300`; separator `my-1 h-px bg-white/5`. |
| `Sonner` toast | Outcome of an action the user started ("Hired Noor Haddad."). | P-layout toast: bottom-right (`sm:justify-end px-4 py-6 sm:px-6`), `max-w-sm rounded-lg bg-gray-800 p-4 outline-1 -outline-offset-1 outline-white/10`; icon outline 24 `size-6` (`check-circle text-green-400`, `information-circle text-indigo-400`, `exclamation-triangle text-amber-300`); title `text-sm font-medium text-white`; body `mt-1 text-sm text-gray-400`; dismiss ghost `x-mark`. Not for errors that block: use an attention callout. |
| `Tooltip` | Labels for icon-only controls (rail items) and reasons for disabled ones. | `rounded-md bg-gray-950 px-3 py-2 text-xs text-white` (current rail tooltip), `side="right"` in the rail, delay 150 ms. Never the only place information lives. |

Focus moves into an opened dialog/sheet and back to its trigger on close (shadcn handles it; keep it). Escape closes. One overlay at a time.

**Inbox item below 960 px (I1; diverges from AC-IA09-2, which says below 640 px; to raise with Emery).** From 960 px the Inbox shows list and detail side by side (§2). Below 960 px the list fills the page; choosing an item opens it **as its own view** (route `/inbox/{id}`, per nav freeze rev 5; on this view the "what" line is the page's `<h1 tabindex="-1">`, §5) with the back link "← Inbox" (§5) above the detail, and the list is hidden. Not a sheet, and never the detail stacked under the whole list. Below 960 px focus moves to that `<h1>` on open (no visible ring, it isn't a control) and back to the item on return. From 960 px the side-by-side detail keeps an `<h2>` for the "what" line and focus moves to it the same way. At 960 px and up, `/inbox/{id}` (the deep link from "Waiting on you: {item}") opens the side-by-side view with that item selected. With nothing selected at 960 px and up, the detail pane reads "Select an item" / "Choose an item to see who is asking, why, and what they recommend."

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
| `--sidebar` | black / 10% | | #1b2533 | Sidebar and the profile header |
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

Fixed sizes: sidebar 288 (`w-72`), rail 64 (`w-16`), aside 340 (`w-[340px]`), Inbox list 260–440, top bar 64/56, control 36, mobile target 44, nav item 40, row 88 (two lines), pill 24, avatar 24/32/40 (dot 8/10/12), icon 24 (nav), 20 (in controls), 16/14 (in pills).

---

## 17. Focus

One focus style everywhere: `focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400`. Inset (`-outline-offset-2`) where the outline would be clipped: sidebar items, list rows, menu items, tabs. `:focus` without `-visible` never shows a ring on click. The ring is never removed without a replacement, with one exception: headings that the app focuses (`h1`/`h2` with `tabindex="-1"`, §5) show no ring, because they aren't controls. shadcn components get their `focus-visible:ring-3 ring-ring/50` replaced by the outline above (the 50% ring is ~2:1). Skip link "Skip to content" first in tab order (`rounded-md bg-indigo-500 px-3 py-2 text-sm font-semibold text-white`, P-layout).

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
| gray-950 on amber-300 (Inbox count badge) | 13.9 on the badge fill | | | | ✔ |

**Pills** (text on tone/10 over page / over white/5): see §7 table; all chosen pairs ≥ 5.4. Rejected prototype pairs: indigo-400 on indigo/10 4.07, red-400 on red/10 4.48, gray-400 on gray/10 over white/5 4.12.

**Buttons:** white on indigo-500 4.58; on indigo-500/80 hover > 5.1; white on indigo-400 3.12 ✘ (prototype hover, rejected); white on white/5 outline button 12.65; red-300 on red-400/10 6.75; white on red-500 3.82 ✘.

**Non-text (3:1):** focus indigo-400 vs page 4.71, vs current nav item 4.29 ✔ (indigo-500: 3.21 / 2.92 ✘ on current item); input border gray-500 vs page 3.04 ✔ (white/10: 1.36 ✘); green-400 dot 8.27; indigo-400 dot (Working, With you) 4.71; amber-400 dot 8.55; gray-300 paused mark on its gray-700 disc ≈ 7.0; gray-400 dot/icon 5.64; gray-500 dot 3.04 (borderline, replaced by gray-400). Dividers (white/5) and pill rings are decorative: they don't carry information, so they're exempt; every pill also has its words.

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
| ≥ 1280 | Sidebar 288 + content; Home shows Happening now and Recent activity side by side; profile and project Overview show their aside (§2). | Gutters 32 |
| 960–1279 | Sidebar 288 + content; Home sections and asides stack; Inbox list and detail side by side; settings navigation as a left column. | Gutters 32 (≥1024) / 24 |
| 640–959 | **Rail**: `w-16`, icons only, each item `size-12` centred, label `sr-only`, right-side `Tooltip` with the name; logo becomes a 48 px link; the Recent and Teams sub-lists hidden; footer hidden (only the logo switch, which goes to `/welcome`). | Gutters 24; Inbox item opens as its own view (§13); settings navigation as an underline bar |
| < 640 | **Top bar + Menu drawer**: no rail; top bar 56 px with a 44 px **Menu** button (opens the left `Sheet` with the full sidebar, §13); search collapses to a 44 px icon button that opens a full-width field over the bar (§4); New ▾ becomes a 44 px icon button with `aria-label="New"` + tooltip. | Gutters 16; tables → rows; meta facts beyond the 2nd hide; pills wrap below the title instead of squeezing; every control 44 px (`max-[639px]:h-11`, including Home card and header buttons); profile actions wrap under the name |

Also: 200% text zoom must not clip or overlap (rows wrap, pills keep whole words). Minimum window 960 × 640 in Tauri; the web build supports down to 320 px.

---

## 21. Empty, loading and error states

| State | Pattern | Source / classes |
| --- | --- | --- |
| Empty page / not built yet | Header strip (§5) with the page title, then a centred block: outline 24 icon `mx-auto size-12 text-gray-400`; `mt-2 text-sm font-semibold text-white` title; `mt-1 max-w-sm mx-auto text-sm text-gray-400` sentence; `mt-6` one primary button if there's a real next step. No "Coming soon" pill. | P-soon: `px-4 py-24 text-center sm:px-6 lg:px-8` |
| Empty list | One centred row (§6). Words say what this place is for and the next action: "No members yet. Hire your first agent." | P-home |
| No search results | Top-bar search: "Nothing matches “{query}”." In a filtered list: "No members match “{query}”." Both with a polite live region holding the count. | P-home `sr-only aria-live` |
| Loading (first load) | Skeleton rows in the real layout: same row padding, `h-4 w-48 rounded bg-white/5` bars, `animate-pulse motion-reduce:animate-none`, and an `sr-only` "Loading {what}…" ("Loading members…", "Loading what needs you…"). After 2 s, a words line ("Still loading…"). Cards (Home) get card-shaped skeletons in the card grid. | |
| Loading (action) | The button keeps its width, shows a spinner and the -ing verb ("Pausing…"), `aria-busy`. | |
| Error (section) | Attention callout (§9) in place of the content: what happened, why if known, the next step as a button ("Try again"), copyable details in a `<details>`. On Home, where several sections can fail at once, each failed section shows a one-line error instead ("Couldn't load {what}." + ghost **Retry**, mini `x-circle text-red-300`), so the page doesn't stack callouts (AC-IA04-2). When **every** section fails for the same reason (the office didn't answer), Home shows **one** attention callout at the top ("Couldn't reach the office. Your data is safe." + **Try again**, which retries all sections) and the sections show nothing else. Prototype draft 2's four stacked callouts are the case this rule rules out (decided 11 Nov; diverges from AC-IA04-2's per-section error with Retry; to raise with Emery). | |
| Error (route) | Full page: `py-24 text-center`; status `text-base font-semibold text-indigo-400`; `mt-4 text-3xl font-semibold tracking-tight text-white` message; `mt-6 text-sm/6 text-gray-400` explanation; `mt-8` **primary button** "Back to Home". | P-error |
| Unavailable (browser build) | Same as route error with the product logo above, one sentence and the command in an inline code span; no nav list, no card. | |
| Paused live updates | Shell banner (§9). | |

**Page and section copy (v2).** Every priority page (Home, Inbox, team page, member profile, Members, Activity) has three designed states: **empty** (the copy below), **loading** (skeleton in the real layout) and **error** (section error above). Titles in `text-sm font-semibold text-white`, sentences in `text-sm text-gray-400`. Small sections use the empty-list row; whole pages use the empty-page block with its icon.

| Place | Empty state (exact words) | Icon (outline 24) |
| --- | --- | --- |
| Home · Needs you | "Nothing needs you" / "When a member asks, needs an approval or is blocked on you, it shows up here and in the Inbox." | `check-circle` |
| Home · Projects | "No projects yet" / "Projects group the work toward one goal." + outline **Create project** (AC-IA04-2) | `folder` |
| Home · Happening now | Presence reported: "No one is working right now." v1.0 (truth rule, §7): "Live status isn't available yet." | — (row) |
| Home · Recent activity | "No activity yet. Hires, work and decisions show up here." | — (row) |
| Inbox (any open tab) | "Nothing waiting on you" / "You're all caught up for this filter." | `check-circle` |
| Inbox · Handled | "Nothing handled yet" / "Items you act on move here." | `inbox` |
| Inbox detail, nothing selected (≥ 960 px) | "Select an item" / "Choose an item to see who is asking, why, and what they recommend." | `inbox` |
| Team page · Members | "No members yet" + the header's Hire or invite ▾ | `user-group` |
| Team page · Work tab | "No work here yet." | — (row) |
| Profile · Overview | "Not working on anything right now." | — (line) |
| Profile · Notes and memory (v1.0) | Quiet callout (§9): "Not available yet: notes and memory arrive in a later release." | `information-circle` |
| Profile · Work / Activity / Conversations | "No work assigned to {name} yet." / "No activity yet." / "No conversations with {name} yet." | — (row) |
| Members filter | "No members match this filter." | — (row) |
| Activity | "No activity yet. Hires, work and decisions show up here." While runs aren't recorded, a description line: "Runs appear here once the office reports them." | `signal` |
| Project · Overview Needs you | "Nothing needs you on this project." | — (row) |
| Organization settings sections not built | "Not in this release" + one sentence saying what will live here (AC-IA03-3). | the section's icon |


---

## 22. Icons

**Migration (Eden, 2026-11-11):** chrome (sidebar, top bar, page headers, pills, buttons, menus) uses Heroicons now. Lucide icons still inside screen content may stay until that screen is restyled in BUG-70; every restyled screen moves to Heroicons. Don't mix the two sets on one screen.

- Set: **Heroicons v2** (the prototype's set; MIT). Outline 24 (`stroke-width 1.5`) for nav, empty states, toasts; solid 20 "mini" for icons in buttons, inputs, menus, chevrons; solid 16 "micro" for pills and meta. Paths live in `lib/components/icons.ts` (extend it, or add `@heroicons` SVGs inline); no new `@lucide/svelte` imports. Existing Lucide uses are migrated screen by screen (gap list).
- Decorative icons are `aria-hidden="true"`; an icon-only control has an `aria-label` and a tooltip.
- Colour: `text-gray-400` by default, `text-white` when its control is current/hovered, a tone colour only when it reinforces a status word next to it.
- Mapping for common needs: add `plus`, close `x-mark`, more `ellipsis-vertical`, back `chevron-left`, open `chevron-right`, sort `chevron-up-down`, select `chevron-down`, search `magnifying-glass`, info `information-circle`, warning `exclamation-triangle`, error `x-circle`, success `check-circle`, pause `pause`, sleep `moon`, person `user`, people `users`, team `user-group`, office `building-office`, project `folder`, task `clipboard-document-list`, run log `command-line`, activity `signal`, settings `cog-6-tooth`, chat `chat-bubble-left-right`, thread `chat-bubble-bottom-center-text`, mention `at-symbol`, pinned `bookmark`, attach `paper-clip`, needs you `bell` (one meaning; starred uses `star`), lock `lock-closed`, owner `user-circle`.
- **v2 mappings (IA v2).** One meaning per icon where it carries status; the word is always beside it or in the accessible name.

| Need | Icon | Where |
| --- | --- | --- |
| Home | `home` | Sidebar |
| Inbox | `inbox` | Sidebar, Inbox empty states |
| Needs you | `bell` | Presence pill, Home section title, Inbox item pill |
| Working | `bolt` | Presence pill |
| With you | `user` | Presence pill (elsewhere `user` still means "a person", e.g. View profile) |
| Paused / Resume | `pause` / `play` | Presence pill, avatar paused mark, profile "⋯" menu |
| Available | `check-circle` | Presence pill (also success, On track, Handled; the word tells them apart) |
| Asleep | `moon` | Presence pill |
| Offline | `signal-slash` | Presence pill (and office Unreachable) |
| Status unknown | `question-mark-circle` | Presence pill, office Status unknown |
| At risk | `exclamation-triangle` | Project health pill |
| Off track | `x-circle` | Project health pill |
| Channel | `hashtag` | Chat, Conversations tabs (replaces a typed "#" used as an icon) |
| Teams | `user-group` | Sidebar, team empty states |
| Chat, Message, direct message | `chat-bubble-left-right` | Sidebar, profile Message button, New menu |
| Announcement | `megaphone` | Chat announcements |
| Hand-off | `arrows-right-left` | Activity, chat hand-off messages |
| Between agents (read-only) | `eye` | Chat "Between agents" group and its note "Reading as the owner. To weigh in, message {agent}" (D13) |
| Attach | `paper-clip` | Composer |
| Mention | `at-symbol` | Chat (replaces a typed "@" used as an icon) |
| Close, remove | `x-mark` | Dialogs, sheets, chips, toasts (replaces "×") |
| Hire an agent, Invite a person, Hire or invite | `user-plus` | New menu, team header Same icon for both on purpose: the words tell them apart. |
| More actions | `ellipsis-horizontal` | Profile header "⋯"; row menus keep `ellipsis-vertical` |
| Expiry | `clock` | Decision card meta |
| List / Board | `list-bullet` / `view-columns` | Work layout toggle |
| Runtime contexts | `cpu-chip` | Only for runtime and machine contexts (Agent setup, Runtime field). **Never as an agent's avatar** or a marker that a member is an agent: the kind is a word (§8). |

- **No emoji anywhere** in the UI: labels, buttons, statuses, empty states, notices, sample and mock data, seeded content (PRD §8, E1–E3); pseudo-icons (×, ↳, ›, @, ✓, ★) become Heroicons. **Agent-authored text** (messages, summaries, journal and memory excerpts) is rendered with emoji sequences removed, at render only, never in storage (D7, D19, AC-IA16-1). **Text a person typed** is shown exactly as typed, emoji included.

---

## 23. Vocabulary (kept from v0.1, updated to v1 and IA v2)

| Term | Means | Don't say |
| --- | --- | --- |
| Organization | One workspace: the whole AI company | Company, tenant |
| Owner | The human running the organization; never a team member row (D17), shown as "You · Owner" | Admin, user (in the UI) |
| Agent | An AI employee with a home, memory and backlog | Bot, assistant, worker |
| Member | Anyone in the organization's teams: a person or an agent (D10) | Employee, resource |
| Person · People | A human member; "People" means humans only, everywhere (D10) | User; "People" for a list that holds agents |
| Members | The mixed directory of people and agents (Teams · **Members** · Org chart; filter All · People · Agents) | People (for the mixed list), Directory |
| Team | A group with a lead; teams own projects | Department (in v1 UI), squad |
| Project | A body of work with its own brief and teams | Workspace, initiative |
| Office | Where agents run: HQ, or another office; "This Mac" names the computer | Server, node, host, machine (in the UI) |
| Run | One working stretch of an agent: trigger, start, end, outcome; listed in Activity and on the profile | Job, execution |
| Session | A run's log, opened from its entry inside Activity (the Sessions screen is gone; `/sessions` → `/activity`) | — |
| Chat | Messages between the owner, people and agents: channels, DMs, threads. "Conversations" is the tab name for a project's, team's or member's chat | Session |
| Task · Bug · Request | Work items on a project | Ticket, issue, assignment |
| Goal | The completion condition an agent sets for a task | Objective, OKR |
| Hire / Offboard | Bring an agent into the organization (Hire an agent) / remove it (its records are kept) (D14). For humans: **Invite a person** | Add agent, Create / delete agent |
| Needs you | Waiting on the owner; amber only for what waits on the owner (Needs you, Update needed, Goal paused) | Pending, action required |
| Inbox | The queue of everything waiting on the owner (Approvals, Questions, Decisions, Problems, Handled) | Notifications, alerts, tasks |
| Agent setup | An agent's configuration face (runtime, model, how it runs, office, home folder), in Organization settings | Agent settings, config |
| Organization settings | Everything about how the organization is set up (General, Offices, Agent setup, Roles, Processes, Members and access, Updates) | Settings (alone), Admin, Preferences |

**Not a product concept:** the subagents an agent starts internally. The app never shows them (user decision, 2026-11-07).

**Status meanings** (truth rule): "Active" is the admin flag, never a status word. Presence words and their v1.0 truth rule are in §7: until the office reports runs, only **Paused**, **Needs you** and the "Working on {task}" fact appear, plus **Status unknown** once, on the profile header. "Started by a person" is replaced by **With you** (RUN-1); "Asleep · wakes when called" becomes the pill **Asleep** with the line "Wakes when called" (v2, superseding PROF-1 and RUN-2 wording).

---

## 24. Writing and content rules

- Sentence case; no trailing period on titles, buttons, pills.
- Dates: relative in rows ("5m", "2h ago"), absolute in details ("Sep 14, 2026"). `<time datetime>` always.
- Ids, hashes and messaging names appear in `font-mono` only in detail views and as the id in a work row (§6) (`<code class="rounded-sm bg-white/5 px-1 py-px font-mono text-xs text-gray-300">`, P-inline), never as filters or buttons.
- Confirmations name the object and effect; the primary button repeats the verb ("Offboard ada").
- Errors: what happened, why, what to do. No raw stack traces as the message.

---

## 25. Do / don't

| Do | Don't |
| --- | --- |
| Full-bleed divided rows with a meta line; outlined (untinted) cards only for Home's Needs you and Projects; the owner card heading the Teams index and the Org chart is the one filled card (`bg-white/[0.03]`, §1.3) | Rounded tinted cards around lists, tables, sections or single rows; team cards on the Teams index or any other list |
| One pill per row, trailing; work rows show the work's status | Pill + coloured word + icon for the same status; several pills per row; the assignee's presence on a work row |
| Quiet neutral callout for context; neutral outline on decision cards | Yellow/brown/amber-filled boxes; amber text paragraphs as notes; amber stripes or tinted borders on cards |
| Amber only for what waits on the owner (Needs you, Update needed, Goal paused); Paused in gray | Amber for Paused or anything the owner needn't act on |
| One primary per view; the top bar's New is outline | A primary New next to a page primary; primary buttons on every Home card |
| An avatar dot with its word nearby, or in the accessible name + tooltip | Presence by dot colour alone; a pill for "Status unknown" in lists |
| Sort/filter as text button + DropdownMenu | Code-style chips (`from:@name`, `has:thread`) as clickable filters in normal UI; segmented pill groups for filters |
| Heroicons with words; agent-authored text rendered without emoji | Emoji anywhere in the UI (⭐ 🧵 👥 📌 📎 📣 🔁 ❓ ✅); Lucide for new work; robot/crown glyphs; `cpu-chip` as an agent avatar |
| Round gray-800 avatars with tinted initials | Square indigo tiles, gradient avatars |
| gray-400 minimum for text | gray-500/600 text, indigo-400/500 text on tinted or white/5 fills |
| Page title in the page header; no ring on the h1 after navigation | Page title in the top bar plus another title in the content |
| Hide what isn't built yet, or explain it once | Rows of disabled tabs ("Debt", "Docs", "Insights"…) and "Coming soon" pills |
| shadcn Dialog/Sheet with `bg-gray-900/80` overlay | Backdrop blur, glows, drop shadows on dark |
| `focus-visible` indigo-400 outline | Removing outlines; 50 % rings |
| Real counts and states; health and due dates only when set | Invented metrics, "Working" without a live run, a health label with no source |

---

## 26. Component checklist (what to reuse)

| Need | Use | Prototype source |
| --- | --- | --- |
| Shell frame, top bar, toast | `(shell)/+layout.svelte`, `ShellTopbar`, Sonner | P-layout |
| Sidebar | `Sidebar.svelte` + `nav.ts` | P-sidebar |
| Page header | (new) `PageHeader.svelte`: title, count, trailing snippet; `ProfileHeader.svelte` for members (§5) | P-home header, P2 `pageMember` |
| Row | `DividedRow.svelte` (+ title/meta snippets); member and work rows are DividedRow content, not new row components (§6) | P-home `li`, P2 `memberRows`, `workRow` |
| Status | `StatusPill.svelte` with a `tone` prop (§7), `StatusDot` (copy P-dot) | P-badge, P-dot |
| Presence | (new) one presence mapping + `PresencePill` used everywhere (IA-08, §7); project health through `StatusPill` | P2 `PRES`, `presence()` |
| Decision card | (new) `DecisionCard.svelte` (§9), card and detail variants | P2 `needsCard`, `inboxDetail` |
| Home cards | Project card and the card grid (§2); outlined, never tinted | P2 `pageHome` |
| Avatar | (new) shared `Avatar.svelte` replacing the chat avatar and MemberTile letters, with the presence dot and paused mark (§8) | P-avatar, P2 `av()` |
| Callout | `Callout.svelte` (+ `tone="attention"` icon colour only) | — |
| Empty / not built | (new) `EmptyState.svelte` | P-soon |
| Error page | `routes/+error.svelte` | P-error |
| Tabs | shadcn `Tabs` (add) for in-page filters; link tabs for routes. Teams view switch = the merged S2 `AgentsTabs`, relabelled (§12) | P-agent nav |
| Menus | shadcn `DropdownMenu` | P-home sort menu |
| Forms | shadcn `Input`, `Label`, `Checkbox`, styled native `select` | Tailwind Plus forms |
| Dialog / Sheet | shadcn `Dialog`, `Sheet` | P-layout drawer |
| Icons | `icons.ts` (Heroicons) | P-icons |

---

## Product decisions behind this standard (Eden, 2026-11-11)

- ~~Navigation (owner): Agents (Home), Chat, Projects, Work, Sessions, Activity, Settings. Org chart is a tab on Agents, Offices sits under Settings, Processes is hidden. No route moves (Rowan): `/agents`, `/org-chart`, `/offices` and `/processes` keep working.~~ **Superseded 2026-11-11** by IA v2 (D1, D19; nav freeze rev 5): see the v2 list below.
- ~~Agent rows open the existing agent details sheet in v1.0. A standalone profile page (`/agents/[id]`, as in the prototype) is a later follow-up, not v1.0.~~ **Superseded 2026-11-11**: member profiles are pages in v1.0 (`/members/{id}`, IA-06); I withdrew the "no profile page" call (AD-1, my review).
- "Status unknown" is neutral (outline), not amber. Amber means only "waiting on the owner". (v1.0 §7/§8 still made Paused amber, contradicting this line; v2 fixes it: Paused is gray.)
- A single-word pill too long for its row truncates with an ellipsis, keeping its full text as the accessible name and `title`. Multi-word pills wrap the row, never the pill.
- Last resort (decided 11 Nov, BUG-76 validation): if a pill is still wider than its row after the row has wrapped, it truncates with an ellipsis whatever its word count, keeping the full text as the accessible name and `title`. A pill is never clipped without an ellipsis (its container must let it shrink: `min-w-0 max-w-full` on the pill, `truncate` on the label) and never breaks mid-word.
- Page content uses the same gutters as the bars above it (`px-4 sm:px-6 lg:px-8`, §2), never a fixed `px-8`, so the tab bar, the page and the top bar line up at every width (decided 11 Nov, S2 validation). On Offices this also lets short statuses (Online, Starting, Invited, Offline, Stopped) sit beside their icon at 320 px with 200% text.
- Lists come before boards: Project tasks default to divided rows grouped by status; a board view is optional, behind the List | Board toggle (§12).
- Saved views and filters use the "Show ▾" menu in the page header (§5), not a left list or segmented pills.
- ~~Top-bar search (`Search agents…`) filters the Agents list, so from any other page it opens `/agents?q=`.~~ **Superseded 2026-11-11** by D16: grouped search across projects, work and members, "See all results" → `/search?q=` (§4).
- The mobile drawer's close button may stay inside the panel: one 44 px target.
- Features not built yet keep the existing spec behaviour: a disabled control says why ("Available in a later release"), and whole areas show the unavailable page. Don't hide them silently.
- Inputs use a gray-500 border (3.04:1) instead of white/10 (1.36:1), to meet 3:1.
- The product name stays as v1 ships it. The logo block shows the organization's name.

**IA v2 decisions (2026-11-11).** Source in brackets: Emery's PRD rev 3 decision (D-number) or my review id.
- Home is the landing page (`/home`; `/` redirects): Needs you, Projects, Happening now, Recent activity. It replaces the Agents home and its feed column. [D1, H4, §2]
- Sidebar: Home, Inbox (amber count), Projects with 5 "Recent", Work, Chat, Activity; group "Organization" with Teams (5 + "View all"); Organization settings at the bottom; footer You · Owner, org switcher, "This Mac · HQ office · Online". No group label repeats an item's name. [D2, D8 position, review "Other pages", §3]
- Old routes redirect indefinitely and keep their query; URLs key on ids. [D19]
- Top bar: grouped search with "See all results" → `/search?q=`; New ▾ as an outline button; menu button below 640 px only; search becomes a 44 px icon button below 640 px. [D16, review "Other pages", §4]
- Profile header variant: 40 px avatar, 16 px name with role, one pill, one line, Message + Assign task + "⋯". [M1, M2, M4, §5]
- No visible ring on the h1 focused after navigation. [H3, §5]
- Work rows show the work's status, never the assignee's presence. [T1, §6]
- Paths and ids wrap anywhere; tables become divided rows below 640 px. [M3, §6]
- One presence vocabulary with precedence Needs you > With you > Working > Paused > Available > Asleep > Offline > Status unknown; Working indigo; Paused gray; never colour alone. [D12, review presence table and addendum 1, §7]
- v1.0 truth rule: Paused, Needs you and "Working on {task}"; "Status unknown" only once, on the profile header; no pill for unknown in lists. [D12, addendum 2, PM call accepted 11 Nov, §7]
- Project health shown only when set: On track green, At risk red `exclamation-triangle`, Off track red `x-circle`. Progress = done / total work items. [D15, D8 position, §7]
- Amber is removed from the avatar tints (fuchsia-300 takes its slot) and from the paused mark. [D8 position, §8]
- Decision card: neutral outline, amber pill as the signal, D9 actions, outline-only on Home, Dismiss only for run error and crash. [H1, H2, D9, D11, §9]
- Inbox: kinds All · Approvals · Questions · Decisions · Problems · Handled; one "Show ▾" for project and team; below 960 px an item opens as its own view with "← Inbox" (960 diverges from AC-IA09-2's 640; to raise with Emery). [D11, I1, I3, §12, §13]
- Teams · Members · Org chart is one view switch (the S2 component relabelled); "Members" is the mixed directory, "People" means humans. [D10, §12, §23]
- Team page: Members above the tabs, capped at 6; the Teams index is divided rows, not cards. [T2, T3, D8 position, §12]
- The Org chart is a vertical tree rooted at the owner card; it replaces the Reporting lines view. [D8 position, §12]
- The owner is never a team member row; the owner appears as the owner card, "Reports to You" and the sidebar footer. [D17, addendum 3]
- Notes and memory: a profile tab for agents, "Not available yet" in v1.0. [D6, D18, §12, §21]
- Agent-to-agent threads stay read-only with the note "Reading as the owner. To weigh in, message {agent}". [D13, §22]
- "Hire" everywhere (Hire an agent, Hire / Offboard); "Invite a person" for humans. [D14, §23]
- No emoji in the UI; agent-authored text has emoji stripped at render; text a person typed is shown as typed. [D7, D19, PRD §8, §22]
