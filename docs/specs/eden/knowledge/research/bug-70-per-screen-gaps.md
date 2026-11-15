# Per-screen gaps · v1 against design-system v1

Eden · 2026-11-11 · v1 at `c1da4893` (origin/v1) · standard: `knowledge/design-system.md` (v1.0) (rule refs are its § numbers).

**Evidence.** Screenshots (dark, Chromium) in `shots/`: v1 `v1-d-*` (1440×900) and `v1-m-*` (390×844) from `bun run dev --mode e2e` after creating the sample organization "Acme"; prototype `proto-d-*` / `proto-m-*` from `prototype-agency` `npm run dev`. Reference also: `prototype-agency/screenshots/home.png`, `agent.png`.

**Columns.** Size: S ≤ 40 non-test lines, M 40–150, L > 150 or several files. Owner: **shell** = shared chrome/primitives/tokens (one owner, lands first), **screen** = that screen's owner. Status: **GM** = Avery's BUG-70 gap map already lists this (row named); **FIXED** = done on v1 (commit); blank = new.

**Order.** Do the shell rows first (SH-1…SH-20, then F-1…F-9 foundations), because most screen rows reuse them.

---

## Shell and navigation

| # | Element | Now (v1) | Should be (rule) | Size | Owner | Status |
| --- | --- | --- | --- | --- | --- | --- |
| SH-1 | Nav items and order | 10 items: Chat, Agents, Org chart, Projects, Work, Sessions, Activity, Processes, Offices, Settings (`lib/nav.ts`) | 7 items: **Agents, Chat, Projects, Work, Sessions, Activity, Settings** (§3 table). Update `nav.ts`, `navigation.spec.ts` SECTIONS and the mobile drawer. | S | shell | GM "Navigation" (it said keep all 10 until an IA ruling; the owner ruled 2026-11-11) |
| SH-2 | Org chart in nav | Own nav item | Removed from nav; reachable as the **Org chart** tab on Agents; Agents is `aria-current` on `/org-chart` (§3, §12). `navFor()` maps `/org-chart` → Agents. | S | shell | |
| SH-3 | Offices in nav | Own nav item | Removed; Settings is current on `/offices`; opened from the footer office link and from Settings (§3). | S | shell | |
| SH-4 | Processes in nav | Own nav item | Hidden in v1.0 (§3). Route left unlinked (engineering call whether to keep it reachable). | S | shell | |
| SH-5 | Home | No Home; `/` goes to `/agents` after onboarding | Agents **is** Home: list + Activity column (§2, §6). Sidebar Agents first. | — | (see AG rows) | GM "Home dashboard" |
| SH-6 | Projects section | None | "Projects" section under the main items: initial chip + name, links to `/projects/<id>`, current on that project, "No projects yet", cap 8 + "View all projects" (§3). Hidden in rail. | M | shell | GM "Navigation" (project shortcuts) |
| SH-7 | Footer | Office icon disc + "This Mac / HQ office · Switch organization", links to `/welcome` | Two sibling links (§3 footer): owner link "{Owner} · Owner" with initials avatar, to `/welcome`, keeping today's accessible name; office link "This Mac · HQ office" to `/offices`. No dropdown. | S | shell | |
| SH-8 | Logo block | Logo + org name, `h-24 pt-8` always | `h-16` in the browser, `h-24 pt-8` only in Tauri (drag region) (§3). | S | shell | |
| SH-9 | Nav focus ring | `outline-indigo-500` inset (2.92:1 on the current item) | `outline-indigo-400` (§17, §18). Same for every `focus-visible:outline-indigo-500` in shell. | S | shell | |
| SH-10 | Nav count badge | Amber-300 pill on Chat only | Keep (§3). Move to whichever item owns "Needs you" after SH-1; no badge elsewhere. | S | shell | |
| SH-11 | Top bar: page title | `<h1>` "Agents" etc. in the top bar | Top bar holds search, links, primary action; the `<h1>` moves to the page header (§4, §5). Keep `#shell-page-title` focus contract on the new h1. | M | shell | FIXED partly 536f36be (bar height/border/padding) |
| SH-12 | Top bar: search | None | Search field "Search agents…", `/` shortcut, filters Agents home (§4). Below 640: icon button that expands. | M | shell | GM "Persistent top bar" (search intent open) |
| SH-13 | Top bar: primary action | None (Add agent sits on the Agents page only) | **Add agent** primary button in the top bar, opens the existing AddAgentSheet from any page (§4). | M | shell | GM "Persistent top bar" |
| SH-14 | Top bar links | None | Keep none until Spec/Design/Help have destinations (§4). No change; record decision. | — | shell | |
| SH-15 | Live paused banner | `bg-amber-400/10 border-amber-400/20 text-amber-200` full-width amber strip | Neutral banner: `bg-black/10 border-white/5 text-gray-300`, amber icon only, Retry ghost button, gutters `px-4 sm:px-6 lg:px-8` (§9). | S | shell | |
| SH-16 | Rail (640–959) | Icons with tooltips; logo link; footer hidden | Keep; only the logo switch, no footer text; hide the Projects section (§20). | S | shell | |
| SH-17 | Mobile (<640) drawer | shadcn Sheet with sidebar, close X inside top-right, `bg-black/10 + blur` overlay | Overlay `bg-gray-900/80` no blur; panel `max-w-xs`; close button in the strip outside the panel (P-layout) or keep inside — either, but one 44 px target (§13, §20). | S | shell | |
| SH-18 | Page gutters on phones | Content wrappers use `px-8` at every width (32 px at 390) and DividedRows add their own `px-4` inside (48 px total) | Full-bleed sections with `px-4 sm:px-6 lg:px-8` gutters; rows not nested in a padded wrapper (§2). Fix per page via F-1. | M | shell + screens | |
| SH-19 | Toasts | Sonner default look, bottom-right | Prototype toast: `rounded-lg bg-gray-800 p-4 outline-white/10`, outline icon, title + body, no shadow (§13). Restyle `ui/sonner` once. | S | shell | |
| SH-20 | Focus/ring tokens | `--ring` indigo-500; shadcn controls use `ring-3 ring-ring/50` (~2:1) | `--ring` indigo-400; controls use the outline focus style (§15, §17). | S | shell | |

## Foundations used by many screens

| # | Element | Now | Should be (rule) | Size | Owner | Status |
| --- | --- | --- | --- | --- | --- | --- |
| F-1 | PageHeader | Each page starts with a gray intro paragraph + "Sample data" pill + buttons in a `px-8 py-6 space-y-5` block | New `PageHeader.svelte`: full-bleed strip, `h1` 16/28, count line, trailing slot (§5). Intro sentence optional, under the strip. | M | shell | |
| F-2 | StatusPill tones | Caller passes colour classes ad hoc (e.g. paused amber on Agents) | `tone` prop with the §7 tones; one mapping per domain (agent, office, task, bug, request) in its module. | S | shell | FIXED partly cc858f0f (pill shape) |
| F-3 | StatusPill wrapping | Label uses `[overflow-wrap:anywhere]`; in the 390 px roster "Asleep · wakes when called" breaks one letter per line (`v1-m-agents.png`) | `whitespace-nowrap`, pill never narrower than its words; row wraps instead (§7). | S | shell | new defect |
| F-4 | StatusDot | None in v1 | Port P-dot with the gray-400 fix and the pulse (§7). | S | shell | |
| F-5 | Avatar | Chat: `bg-indigo-500/20 text-indigo-200`, square for agents; org chart MemberTile: square tiles + robot glyph | One round gray-800 avatar with hashed initial tints (§8). | M | shell | |
| F-6 | EmptyState | Several ad-hoc empty blocks (icon disc `bg-white/5`, "Coming soon" Badge) | `EmptyState.svelte` per P-soon (§21). | S | shell | GM "Sessions / Activity / Settings placeholders" |
| F-7 | Buttons | shadcn defaults: outline `dark:bg-input/30 shadow-xs`; destructive red-400 on red/20 (3.86:1); `active:translate-y-px` | Variants per §10; `--destructive` red-300; no translate. | S | shell | |
| F-8 | Inputs / selects | shadcn Input `border-input` (white/10, 1.36:1), `ring-3`; 16 files use unstyled-ish native `<select>` | §11: gray-500 border (`--input`), outline focus, styled native select with chevron. | M | shell | |
| F-9 | Dialog/Sheet overlay | `bg-black/10 backdrop-blur-xs`; dialog `rounded-xl ring-foreground/10` | `bg-gray-900/80`, no blur; `rounded-lg outline-white/10` (§13). Change in `ui/dialog`, `ui/sheet` once. | S | shell | |
| F-10 | Icons | Lucide in 48 files; shell uses Heroicons paths | Heroicons everywhere (§22). Migrate per screen as touched; shell-owned shared icons (status, severity, callout) first. | L | shell → screens | GM "Emoji → icons" (partly) |
| F-11 | Tabs | Three hand-rolled `role="tablist"`s (project, work, org chart), two styles (underline; filled pill group) | shadcn `Tabs` with §12 styling; keep not-yet-available tabs disabled with a tooltip giving the reason (decision in the standard). | M | shell | |

---

## Welcome / create / open (onboarding)

| # | Element | Now | Should be (rule) | Size | Owner | Status |
| --- | --- | --- | --- | --- | --- | --- |
| ON-1 | Organization list | Rows inside a `rounded-xl bg-white/5` card | Flat divided rows (§6) under a `text-sm/6 font-semibold` label; whole row opens; trailing "Open" stays as outline button or chevron | S | screen | GM "Onboarding / dialogs" |
| ON-2 | Org initials tiles | Square `bg-indigo-500/20` tiles "NL", "GR" | Round avatar §8 (or the Logo mark at 40 px) | S | screen | |
| ON-3 | Create card | Big tinted card with solid indigo "+" tile and a trailing building icon | Primary button "Create organization" under the list, or a divided row with mini `plus`; no tinted card (§10, §25) | S | screen | |
| ON-4 | Title | `text-3xl` "Welcome to Agentic Org" | Allowed (onboarding = full-screen, §14). Keep. | — | — | OK |
| ON-5 | Create form field | Input focus ring is a thick indigo ring (`ring-3`) | §11 outline focus | S | shell (F-8) | |
| ON-6 | "Where its office runs" | Read-only option in a `bg-white/5` card with a laptop icon | Plain text block or quiet Callout (§9); it isn't selectable | S | screen | |
| ON-7 | Back link | `← Back` gray-400 link | Mini `chevron-left` + "Back", same as detail-page back item (§5) | S | screen | |
| ON-8 | Mobile 390 | Fits | Keep | — | — | OK |

## Agents (Home)

| # | Element | Now | Should be (rule) | Size | Owner | Status |
| --- | --- | --- | --- | --- | --- | --- |
| AG-1 | Layout | Intro paragraph + Add agent + filter + 8-column table | Agents \| Org chart tab bar (§12) → PageHeader "All agents · 8 agents · 0 paused" + Show/Sort controls (§5) → DividedRow agent list (§6) + Activity column (§2) | L | screen | GM "Home dashboard" |
| AG-2 | Roster table | 8 columns (Agent, Runs on, How it runs, Status, Now, Teams, Office, Reports to) | Rows: title `{name}` + gray `{title}`; meta `{teams} · reports to {manager} · {office} · {runtime} · {run mode}`; trailing StatusPill + chevron. Drop the "Now —" column until it has data (§1.6). | M | screen | FIXED partly 4562145e (flat table); row redesign new |
| AG-3 | 390 px roster | Table squeezes; status pill letters stack vertically; horizontal scroll | Rows (AG-2) wrap; pill whole (F-3) | — | screen | covered by AG-2 + F-3 |
| AG-4 | Filter | Segmented pill group "All 8 / Active 8 / Paused 0 / Former 0" | "Show all ▾" DropdownMenu with counts (§5) | S | screen | |
| AG-5 | Sort | None (A–Z only) | "Sort by name ▾": Name, Status, Team (§5) | S | screen | |
| AG-6 | Status pill content | Lucide icon + "Asleep · wakes when called"; paused amber via ad-hoc classes | Tone from F-2; Heroicons micro icon (§7) | S | screen | FIXED partly cc858f0f |
| AG-7 | Messaging name | `<code>` mono in the meta line | Plain text in meta or only on detail (§24) | S | screen | |
| AG-8 | Activity column | None | No column until an activity feed exists; the list takes the full width (§2). When the feed ships: column ≥1280 / stacked below, feed rows, "View all →" to `/activity`. | — (later) | screen | GM "Home dashboard" |
| AG-9 | Add agent button | Page-level primary | Moves to the top bar (SH-13); page keeps it only in the empty state | S | screen | |
| AG-10 | Empty state | Icon disc + title + text + button | EmptyState (F-6): "No agents yet. Add your first agent." | S | screen | |
| AG-11 | "Sample data" | Badge + sentence under intro | One gray pill after the title (§5) | S | screen | |

## Agent detail

| # | Element | Now | Should be (rule) | Size | Owner | Status |
| --- | --- | --- | --- | --- | --- | --- |
| AD-1 | Where | No standalone page; the roster links to `/org-chart?member=…` (selected-member strip) | A row opens an agent profile (P-agent): secondary nav "← All agents · Overview · Tasks · Journal", header band `bg-gray-700/10` with dot + name + title, meta line, one StatusPill; stats only where real data exists | L | screen | GM "Agent details / Org chart" (product call Q3) |
| AD-2 | Actions | Pause… / How it runs… / Edit… / Offboard… as small outline chips in a row | Header trailing: one outline "Edit" + kebab DropdownMenu (How it runs, Pause, Offboard — destructive item) (§10, §13) | M | screen | |
| AD-3 | Status words | Status line with Lucide moon + text, plus "Now: —" | One StatusPill in the header; drop "Now: —" (§1.4, §1.6) | S | screen | |

## Org chart (tab of Agents)

| # | Element | Now | Should be (rule) | Size | Owner | Status |
| --- | --- | --- | --- | --- | --- | --- |
| OC-1 | Page frame | Own nav item and page; owner card, search, view switch, sample note, counts, office select, 7 toolbar buttons, amber note, chart | Agents \| Org chart tabs (§12) → PageHeader "Org chart · 5 teams · 8 agents · 3 people" with trailing Teams/People/Reporting lines switch → one toolbar row | M | screen | |
| OC-2 | Owner card | `rounded-xl bg-white/5` card with crown in amber disc | Remove the card; the owner is the root of the chart, shown as a normal avatar row "Alex Kim · Owner" (§8 no crown) | S | screen | |
| OC-3 | View switch | Filled pill group (Teams/People/Reporting lines) | shadcn Tabs underline (§12) or Show ▾ menu | S | screen | |
| OC-4 | Toolbar | New team (primary) + Add person + Add agent + List + Actual size + Expand all + Collapse all; overflows at 390 | Primary "New team"; "Add person" outline; the rest in a "View ▾" DropdownMenu (List, Actual size, Expand/Collapse all); Add agent is the top-bar action (§10, §20) | M | screen | |
| OC-5 | Unassigned note | Bare amber text "1 project has no team yet." | Quiet Callout (§9), same words | S | screen | |
| OC-6 | Team cards | `rounded-xl bg-gray-800/50` cards, square tiles with robot glyph, "+1 team" chips, × buttons, "Shared" sky chip | Flat outlined boxes `rounded-lg outline-white/10` kept (a chart needs boxes), round avatars (F-5), "+1 team" as text, gray-400 not gray-500 text (§8, §18) | M | screen | GM "Agent details / Org chart" |
| OC-7 | Selected member strip | `rounded-lg bg-white/5` strip with 7 chip buttons | Agent detail (AD-1) or a right Sheet with the §13 layout | M | screen | GM |
| OC-8 | Search members | Label "Search" + input on its own row | Field in the header trailing slot, §11 style, placeholder "Search members…" | S | screen | |
| OC-9 | 390 px | Toolbar and chart overflow horizontally; cards cut off | Toolbar per OC-4; chart scrolls in its own `overflow-x-auto` region with a visible focusable frame; List view default below 640 | M | screen | |
| OC-10 | gray-500 text | Team card meta and "—" placeholders gray-500 (3.04:1) | gray-400 (§18) | S | screen | |

## Projects

| # | Element | Now | Should be (rule) | Size | Owner | Status |
| --- | --- | --- | --- | --- | --- | --- |
| PR-1 | List rows | DividedRow, 16 px rows, flat | Keep | — | — | FIXED cc858f0f |
| PR-2 | Header | Intro paragraph + Sample data + disabled "New project" with "Available in a later release." | PageHeader "Projects · 4 projects" (F-1); drop the disabled button (§1.6) | S | screen | |
| PR-3 | Unassigned note | Quiet Callout with amber triangle | Keep (§9) | — | — | FIXED cc858f0f |
| PR-4 | Row content | Name + "Lead: …" right-aligned; team chips with × and "Add team…" chip in every row | Title `{name}`; meta `{teams} · Lead: {lead}`; trailing chevron; team editing on the project page, not in every row (§6 row actions) | M | screen | |
| PR-5 | Footer note | `text-gray-500` "Tasks, bugs and documents are added…" (3.04:1) | Remove, or gray-400 (§18) | S | screen | |
| PR-6 | 390 px | 32 px gutters + 16 px row inset; "New project" floats mid-row | SH-18 gutters; PR-2 | — | screen | |

## Project detail

| # | Element | Now | Should be (rule) | Size | Owner | Status |
| --- | --- | --- | --- | --- | --- | --- |
| PD-1 | Header chrome | Flat header, divided metadata | Keep | — | — | FIXED ea5ca768 |
| PD-2 | Breadcrumb + title | "Projects › Atlas" line, then "Atlas" + Sample data, under a top bar that says "Projects" | Secondary nav "← Projects · Overview · Tasks · Bugs" (§5, §12), then PageHeader "Atlas" + meta "Platform · Lead: Kai Lindqvist" | M | screen | |
| PD-3 | Tabs | 9 tabs, 5 disabled gray-500 (Features, Debt, Docs, Insights, Settings) | Show only Overview, Tasks, Bugs; shadcn Tabs (F-11) | S | screen | |
| PD-4 | Team chips | "Platform ×" + "Add team…" chips | Meta line text; "Edit teams…" outline button in header trailing (§10) | S | screen | |
| PD-5 | Overview notice | Callout with info icon "Features, bugs, debt… later release" | Remove once PD-3 hides those tabs, or keep one quiet Callout (§9) | S | screen | FIXED partly ea5ca768 |
| PD-6 | Tasks board | 4 `rounded-xl bg-white/5` columns + Board/List pill switch + "New task" | List view default: divided rows grouped by status with a section label per group (§6); board optional via "View ▾"; New task primary in header | M | screen | GM "Work / Tasks" |
| PD-7 | Empty tasks | "No tasks." in each column + "No tasks yet. Create the first one." | One EmptyState (F-6) | S | screen | |

## Work

| # | Element | Now | Should be (rule) | Size | Owner | Status |
| --- | --- | --- | --- | --- | --- | --- |
| WK-1 | Header | Intro + Sample data + right "Nothing needs you." | PageHeader "Work · 3 tasks · 1 needs you" (F-1) | S | screen | GM "Work / Tasks / Requests / Bugs" |
| WK-2 | Tabs | Tasks / Bugs / disabled Debt / Requests, underline | shadcn Tabs (F-11); Debt disabled with its reason; counts as tab counts (§12) | S | screen | GM |
| WK-3 | Saved views | Left list "Assigned to me / Blocked tasks / All" with counts, `bg-white/5` selected | Show ▾ DropdownMenu in the header (§5), or keep a left list styled as sidebar items (§3 item classes) at ≥1280 | M | screen | |
| WK-4 | Queues | `rounded-xl bg-white/5` boxed tables/cards (TasksBoard, BugsPanel, RequestsPanel) | Flat divided rows or plain tables (§6); one status pill per row; severity/kind/priority in meta | L | screen | GM |
| WK-5 | Request groups | Collapsible `rounded-xl` cards per group (Needs you, With agents…) | Section labels `text-xs/6 font-semibold text-gray-400` + divided rows; "Needs you" items amber pill (§7 Request) | M | screen | GM |
| WK-6 | Needs-you count | `bg-amber-500/20 text-amber-200` pill on the tab | Tab count style (§12) with amber only for "needs you" | S | screen | |
| WK-7 | Urgent priority | `font-semibold text-amber-300` cell | Meta text "Urgent" + mini `exclamation-triangle` (§7) | S | screen | |
| WK-8 | Error blocks | `rounded-xl bg-white/5 text-red-300` centred boxes | Attention Callout (§9, §21) | S | screen | |
| WK-9 | Disabled tabs text | gray-500 (3.04:1) | Hidden, or gray-400 (§12) | S | screen | |

## Sessions · Activity · Settings (not built)

| # | Element | Now | Should be (rule) | Size | Owner | Status |
| --- | --- | --- | --- | --- | --- | --- |
| NB-1 | Placeholder | Centred h2 + "This part of Agentic Org isn't built yet." + "Coming soon" Badge | PageHeader with the title, then EmptyState: outline icon (`sessions`/`activity`/`settings`), "{Name} is coming soon", one sentence of what it will hold, primary "Back to agents" (P-soon, §21) | S each | screen (F-6) | GM "Sessions / Activity / Settings placeholders" |
| NB-2 | Settings | Placeholder only | Settings page lists **Offices** as its first section (link row to `/offices`) even before other settings exist (§3) | S | screen | |
| NB-3 | Activity | Placeholder | Destination of "View all →"; same feed rows as AG-8 when data exists | M | screen | GM (data separate) |

## Chat (channel, threads, search, DMs)

| # | Element | Now | Should be (rule) | Size | Owner | Status |
| --- | --- | --- | --- | --- | --- | --- |
| CH-1 | Emoji as UI | ⭐ Needs you, 🧵 Threads, 👥 group DM, 📌 Pinned, 📎 Attach, 📣/🔁/❓/✅ kind marks (`KIND_MARK`, AskCard) | Heroicons: `bell`, `chat-bubble-bottom-center-text`, `user-group`, `bookmark`, `paper-clip`, `megaphone`, `arrow-uturn-right`, `question-mark-circle`, `check-circle` (§22). Reactions (✅ 👀) stay: user content. | M | screen | GM "Emoji → icons" |
| CH-2 | Search filters | Clickable `font-mono` chips `from:@name`, `in:#channel`, `has:thread`… | "Filter ▾" DropdownMenu with plain words (From, In, Has thread, Kind, Before/After) that writes the query; the syntax stays typeable (§25) | M | screen | |
| CH-3 | Avatars | Square `bg-indigo-500/20 text-indigo-200` for agents, round for people | Round §8 avatars (F-5); "Agent" said in words where it matters | S | screen (F-5) | |
| CH-4 | Conversation sidebar | Second column with its own section labels (CHANNELS uppercase) | Section labels `text-xs/6 font-semibold text-gray-400`, sentence case ("Channels", "Direct messages"); items §3 item classes at `py-1.5` density | S | screen | |
| CH-5 | Unread counts | Mixed: amber circle (Needs you), indigo circle (general), plain numbers | Plain `text-xs text-gray-400` counts; bold name for unread; amber only for Needs you (§3 badge rule) | S | screen | |
| CH-6 | Hand-off card | Indigo-bordered `rounded` card with ↪ glyph | Quiet Callout-style box `outline-white/10` (§9); micro `arrow-uturn-right` icon | S | screen | GM "Chat" |
| CH-7 | Linked id chip | `ac-T-12` as code chip | Link text `text-indigo-400` (§10 link) | S | screen | |
| CH-8 | Message header kind | "📣 Announcement · Delivered" | "Announcement · Delivered" meta text + micro icon (§7 announcement reason) | S | screen | |
| CH-9 | Composer footer | "@mention · / command", 📎 Attach, checkbox "Enter sends", indigo Send | Keep layout; icon per CH-1; Send = primary button; hint gray-400 | S | screen | |
| CH-10 | Thread panel | Right pane, own header | Keep structure; header as §5 strip, close = ghost `x-mark` | S | screen | |
| CH-11 | Context notices | Quiet callouts | Keep | — | — | FIXED 39709507 |
| CH-12 | "Sample data" | Pill in the conversation header | One per page in the page header (§5) | S | screen | |
| CH-13 | 390 px | "Conversations 2" button, stacked header; hover toolbar overlaps text | Keep pattern; toolbar must not cover message text on touch (show on focus/long-press) | S | screen | |
| CH-14 | Page title duplication | Top bar "Chat" + conversation title "#release-v1" | After SH-11, the conversation title is the h1; no separate "Chat" heading | S | screen | |

## Offices (under Settings)

| # | Element | Now | Should be (rule) | Size | Owner | Status |
| --- | --- | --- | --- | --- | --- | --- |
| OF-1 | Status pill | StatusPill with Lucide icons | Keep shape; Heroicons micro + tones per §7 office rows | S | screen | FIXED 423c3f04 (pill); icons new |
| OF-2 | Office list | Rows inside `rounded-xl bg-white/5` card | Flat divided rows (§6): "HQ" title + meta "This Mac · you're here · 7 agents", trailing pill + chevron | S | screen | GM "Offices" |
| OF-3 | Role tile | Square `HQ` indigo tile | Text in title ("HQ") or round avatar with building icon (§8) | S | screen | |
| OF-4 | Unplaced box | `rounded-xl bg-amber-500/10 outline-amber-400/20` brown box with amber-100 text | Section "Unplaced · 1 agent" (label) + quiet Callout explanation + divided rows; amber only on an icon (§9, §25) | S | screen | GM "Offices" |
| OF-5 | Disabled actions | "Add office" disabled primary + "Available in a later release." ; "Place…" disabled | Keep disabled with the reason (decision in the standard): as outline/secondary buttons, not a disabled primary; one quiet gray-400 note. | S | screen | |
| OF-6 | Navigation | Own nav item | Breadcrumb secondary nav "← Settings · Offices"; Settings current (SH-3) | S | screen | |
| OF-7 | Footer note | "Shown from HQ. Statuses include last-seen information." gray text | Keep, gray-400, or quiet Callout | — | — | OK |

## Processes (hidden in v1.0)

| # | Element | Now | Should be (rule) | Size | Owner | Status |
| --- | --- | --- | --- | --- | --- | --- |
| PC-1 | Visibility | Nav item | Not linked (SH-4). No restyle in v1.0. When it returns: flat rows, "Organization v3" as meta text, Sealed as outline pill with `lock-closed`, no amber | — | — | GM "Processes" (deferred) |

## Unavailable pages (browser build) and errors

| # | Element | Now | Should be (rule) | Size | Owner | Status |
| --- | --- | --- | --- | --- | --- | --- |
| UN-1 | Unavailable | Own header + nav list + `rounded-xl bg-white/5` card with `text-2xl` title | §21 "Unavailable": logo, centred title, one sentence, inline code command; keep the nav list only if engineering needs it for links (then §3 item classes) | S | shell | |
| UN-2 | 404 / error | Status indigo-400, `text-2xl` title, plain white text link "Back to the start" | P-error: `text-3xl tracking-tight` title, explanation gray-400, **primary button** "Back to agents" (§21) | S | shell | |

## Dialogs and sheets

| # | Element | Now | Should be (rule) | Size | Owner | Status |
| --- | --- | --- | --- | --- | --- | --- |
| DL-1 | Overlay | `bg-black/10` + backdrop blur | `bg-gray-900/80`, no blur (F-9) | S | shell | |
| DL-2 | Dialog box | `rounded-xl ring-1 ring-foreground/10`, title `text-sm font-medium` | `rounded-lg outline-white/10`, title `text-base/7 font-semibold` (§13) | S | shell | |
| DL-3 | Confirm (Pause Noor Haddad?) | Good content; Cancel outline + primary verb | Keep content; destructive confirmations (Offboard, Remove) use the destructive variant (§10) | S | screen | |
| DL-4 | New task / Report a bug | Native selects, textareas with `ring-3` focus, "Draft or improve" ghost link with Lucide sparkles | F-8 inputs; Heroicons `sparkles` mini | S | screen | |
| DL-5 | Add agent sheet | Long right sheet; native selects; team checkbox list in a `bg-white/5` box | §13 sheet header/body/footer; sticky footer with primary "Add agent"; checkbox list as plain list with dividers | M | screen | |
| DL-6 | Task / Request / Bug sheets | `bg-white/5` blocks, amber notes | §13 sheet, quiet callouts, one status pill in the sheet header | M | screen | GM "Work" |
| DL-7 | Team/people edit dialogs (8 files) | Mixed `bg-white/5` boxes, amber hints | F-8 inputs; quiet callouts for hints | M | screen | |

---

## Coverage check

Screens covered: welcome, create, open (welcome list), shell (sidebar, rail, drawer, top bar, banner, toasts), Agents, agent detail, Org chart, Projects, project detail (Overview, Tasks), Work (Tasks; Bugs and Requests from code), Sessions, Activity, Settings, Chat (channel, thread, search, DMs, composer), Offices, Processes, unavailable, 404, dialogs and sheets. Not screenshotted: Work Bugs/Requests tabs with data, Offices error states, Chat Needs you/Mentions/Threads views (rows based on code).

Already fixed on v1 and kept: PR-1, PR-3, PD-1, CH-11 (fully); SH-7, SH-11, F-2, AG-2, AG-6, OF-1, PD-5 (partly). Gap-map rows: 25 of the 119 rows here are already listed in the BUG-70 gap map in broader terms; this list splits them into actionable slices.
