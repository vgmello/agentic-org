---
kind: spec
title: "App shell: navigation and shared interaction standards (UX)"
status: for-review
revision: 2
owner: lead
created: "2026-11-10"
updated: "2026-11-15"
requested_by: ac-product-designer-eden
reviewers: [ac-staff-engineer-ada, owner]
builds_on: [our-org-in-the-app, offices-and-teams-ux]
---

# Spec · App shell: navigation and shared interaction standards (UX)

The browser app shell: the main navigation at each viewport width, what every
page must do at 320 px, the pages shown for areas this release doesn't serve,
page titles and landmarks. Area specs build on it: `chat-ux.md` and
`processes-ux.md` follow its narrow-width and title rules, and
`server-installation-and-access-ux.md` SRV-18 points here for the unavailable
pages.

**Source:** the Principal Product Designer's decision of 2026-11-10 on narrow
widths (BUG-52 and BUG-53, the shell nav and unavailable pages; BUG-49, Chat),
with its follow-ups. Breakpoints are **viewport widths in CSS px**.

## Changelog

**Revision 2 (2026-11-15):** integrate company-oriented navigation, settings/search/New, and shared visual, accessibility and interaction
requirements from Eden's design system v2.6. These rules apply to supported
screens; they do not enable gated routes, presence reporting or deferred engines.

**Revision 1 (2026-11-10): new spec.** Navigation by viewport width (full nav,
icon rail, top bar with a nav drawer), the 320 px rule (WCAG 1.4.10), the
unavailable pages' copy, page titles, landmarks, and Back with a modal open.
**New:** SHELL-1 to SHELL-28.

## 0. Information architecture

Home (`/home`) is the landing page. The full sidebar, rail and drawer use the same destinations, in order: Home; Inbox; Following when its work service is available; Projects; Work; Chat; Activity; Teams; Organization settings. Do not repeat Work as a group label. Offices and Agent setup belong to Organization settings; runs belong to Activity instead of a separate Sessions screen.

Projects shows up to five projects and View all. Use Recent only when project last-activity timestamps exist; until then list the first five names A–Z under Projects. Teams shows up to five teams and View all once team pages are available. Under vertical pressure, collapse Teams' sub-list, then Projects', before allowing top-level navigation to scroll. Organization settings and the footer stay reachable.

Inbox's amber badge appears only for a nonzero open count; it and Home's Open Inbox (N) use the same source. Following's update badge is neutral and counts updates since last viewed, independently of the Inbox. Hide Inbox until its first source is served and optional feature destinations until their service exists. These gates do not remove Home.

The footer's You · Owner opens an account menu: Your preferences, Switch organization, About, and Sign out when owner sign-in is supported. Omit actions with no flow. An account menu does not imply an employee record. Its separate office line reads “{Office name} office · {status}” and opens `/settings/offices`; machine/host details belong inside Offices.

All entity URLs use stable IDs. Keep the following redirects indefinitely and preserve their complete query strings: `/` → `/home`; `/agents` → `/teams/members`; `/org-chart` → `/teams/org-chart`; `/offices` → `/settings/offices`; `/processes` → `/settings/processes`; `/sessions` → `/activity`. A member profile marks Teams current. Route changes focus the destination heading, including when using the Teams view switch; Back/Forward preserve the destination's state/title.

### 0.1 Organization settings and preferences

`/settings` contains General, Offices, Agent setup, Roles, Processes, Members and access, Costs where authorized and implemented, and Updates. Use an underline navigation bar, or a left column of links from 960 px. General shows organization name/workspace and supported runtime configuration without exposing credentials. `/settings/offices` reuses the office flows in offices-and-teams-ux.md; it changes their location, not their safety requirements. Personal preferences and About belong in the account menu.

A section outside this release uses “Not in this release” and its explanatory sentence; an included section awaiting implementation uses “{Title} isn't built yet” and “It's coming in this release.” Never use Coming soon or mix these states. Processes remains linked from settings but its production route is the root Unavailable page without a navigation list or Back to Home. Processes, Roles, Members and access and Updates do not become active through moving their links. The selected structural process records remain separate from the deferred process engine. Updates does not offer restart-to-update before a supported distribution/update flow exists.

`/preferences` initially contains Keyboard, with a Single-key shortcuts switch, on by default. Persist one device-wide `agentic-org.singleKeyShortcuts` value (`1`/`0`), without server sync; unavailable/unreadable storage falls back to on. Appearance and Notifications sections appear when they have an actual setting. Time zone follows the target requirement below when its account contract is selected.

Actions the viewer is not authorized to perform are hidden. When omission would make the state confusing, show the unavailable action with a visible reason instead. UI visibility never replaces server authorization.

### Target account time display

Account-zone persistence, protected account authorization and account-to-employee mapping require their owning contract before this preference can ship. This display requirement adds no employee, provider or execution timezone field and no account data contract. The separately [deferred private digest](project-status-ux.md#deferred-account-digest-requirements) remains deferred.

At first sign-in without a saved account zone, detect a validated IANA zone and let that authenticated account change it under Your preferences → Time zone. The searchable list shows city/name, current offset and local time. Invalid or missing detection may show **unsaved UTC** with “Time zone not detected: set it in Your preferences”; retry at later sign-in until a valid zone is saved. This display fallback never establishes a budget zone.

Event lists show relative time within 24 hours and absolute time thereafter. Headers, ranges, logs and card/problem times use absolute 24-hour values in the saved viewing-account zone, with full date/time/offset accessible. A separately selected schedule uses its own zone and full date/city/offset at the instant, never time-only or Today/tomorrow. No personal agent timezone is introduced.

Retain temporal instants exactly and apply each instant's DST offset; compute elapsed duration from instants, never by subtracting local clock readings. Changing a display preference cannot mutate retained budget period keys/bounds or old daily-summary snapshots. Travel prompts, people/agent directory zone reads and private digest services remain deferred.

- AC-IA20-1: New York 2026-03-08 01:30 UTC−5 to 03:30 UTC−4 is one hour elapsed; two viewers render the same retained instant in their saved zones without changing it.
- AC-IA20-2: invalid detection shows unsaved UTC while [budget controls](../design/organization-data/17-budgets-and-controls.md) still refuse budget creation without a known selected owner/HQ zone. Preference changes leave retained period keys/bounds and summary snapshots intact; no employee timezone or private digest service appears.

### 0.2 Search and creation

Top-bar Search groups results as Projects, Work, Members; Conversations joins only when indexed with its message authorization filters. Arrow keys and Enter choose results; See all results opens `/search?q=`. Below 640 px use a 44 px icon button that opens a full-width search field.

Top-bar New is outline, with Project, Work item, Request, Message, a separator, Hire an agent, Add a person in that order. Omit flows that do not exist. Request opens the existing typed-request capture dialog; it uses the shared T- identity model. Below 640 px New is a 44 px icon button named New.

N or Shift+N outside editable text opens New; arrows/Enter choose, Escape closes. N ships together with Single-key shortcuts. `/` focuses search only when Search ships and the switch governs it from its first release. With the switch off neither shortcut runs outside editable text; remove their hints and aria-keyshortcuts. Cmd+N/Ctrl+N keep the browser's New window action. Closing/cancelling/completing a flow returns focus to its opener or the next sensible control, never the body.

**IA acceptance criteria:**

- AC-IA01-1–4: verify the sidebar order, bounded sub-lists, common Inbox count, footer, exact redirects with query preservation, current-page marking and heading focus across rail/drawer/full nav.
- AC-IA03-1–3: verify settings destinations, Offices relocation, honest included/deferred copy and the production Processes gate.
- AC-IA13-1–2: grouped keyboard-search results and `/search?q=` work; the narrow search control preserves accessible naming.
- AC-IA19-1–4: verify New's order and omitted unavailable flows, narrow target/name, N/Shift+N behavior, device persistence/fallback, and disabled shortcut hints. Test `/` against the same setting when it is introduced.

## 1. Navigation by viewport width

| Viewport width | Navigation |
| --- | --- |
| 960 px and wider | The full nav, as today |
| 640 to 959 px | A 64 px icon rail |
| Below 640 px | A 56 px top bar with **Menu**, which opens the full nav as a drawer |

### 1.1 960 px and wider

The full nav, as today.

### 1.2 640 to 959 px: the icon rail

```
 ┌────┬──────────────────────────────────────────┐
 │ LL │ Page heading                              │  ← LL: the logo, which is the org switcher
 │ ⌂  │                                           │
 │ 💬 │                                           │  ← icon only; tooltip "Chat" on hover and focus
 │ …  │                                           │
 └────┴──────────────────────────────────────────┘
   64 px
```

- The nav collapses to a **64 px icon rail**. Each item shows its icon only.
- Each item's **accessible name** is the item's name, and a **tooltip** with that
  name shows on hover and on keyboard focus.
- `aria-current` marks the current page's item.
- The rail logo links to Home and is named "{organization} home"; switching
  organization is in the account menu.
- There's **no expand toggle**: the rail doesn't open into the full nav.

### 1.3 Below 640 px: the top bar and the nav drawer

```
 ┌──────────────────────────────────────┐      ┌────────────────────────┬─────┐
 │ [☰ Menu]  Page title                 │      │ Lumen Labs ▾   [Close] │░░░░░│
 ├──────────────────────────────────────┤      │ Home                   │░░░░░│
 │                                      │  →   │ Chat          (current)│░░░░░│  ← scrim
 │ Page content                         │      │ …                      │░░░░░│
 │                                      │      │                        │░░░░░│
 └──────────────────────────────────────┘      └────────────────────────┴─────┘
   56 px top bar                                  min(288 px, 100vw − 56 px)
```

- The rail is hidden. A **56 px top bar** has a **"Menu"** button and the page
  title. Menu has `aria-expanded` (true while the drawer is open) and
  `aria-controls` pointing at the drawer.
- **Menu opens the full nav as a modal drawer** from the left,
  `min(288 px, 100vw − 56 px)` wide, so at least 56 px of the **scrim** always
  shows beside it.
- **Opening:** focus goes to the current page's item, and focus is **trapped** in
  the drawer while it's open.
- **Closing without choosing:** **Esc**, the **scrim** or **Close** close the
  drawer and return focus to **Menu**. This holds however the drawer was opened
  (mouse or keyboard), in every browser engine.
- **Choosing an item** closes the drawer and moves focus to the new page's `h1`.
- **Back** with the drawer open closes the drawer only, doesn't navigate, and
  returns focus to **Menu** (§6).

### 1.4 Badges

A nav item may supply a **badge** (an optional count). When it does, both the
rail and the full nav (and so the drawer) render it. There's **no global badge**
in v1.0.

### 1.5 Test widths

The shell is checked at **959**, **720** (200% zoom of 1440), **639**, **600**
and **320** (400% zoom of 1280) px.

## 2. The 320 px rule (WCAG 1.4.10)

Every page, everyone's:

- **The page never scrolls sideways**, and **no text column is narrower than
  200 px**. Where a column would be narrower, the panel **overlays** instead, or
  the layout **stacks**.
- **Two-dimensional content may scroll inside its own region** (1.4.10 exempts
  it), such as the org chart canvas. Member/agent/task rosters become divided
  rows below 640 px instead of requiring a sideways-scrolling table. The page
  around an independently scrolling region never scrolls sideways.
- **Dialogs and sheets take the full width below 640 px**, with their buttons
  wrapping and still visible. The nav drawer is the exception (§1.3).
- **Page toolbars wrap**, and **search fields are full width** below 640 px.

## 3. Unavailable pages

Production areas the web process doesn't serve show an unavailable page instead of
sample data (`server-installation-and-access-ux.md` SRV-18). Exact copy:

**The root, `/`:**

> **Agentic Org isn't ready in the browser yet**
>
> This server is running, but this release doesn't serve its pages in the browser
> yet. To see what it can do now, run `agentic-org doctor` on the HQ machine.

**An area** (<Area> is the area's name):

> **<Area> isn't available yet**
>
> This release of the server doesn't serve <Area> in the browser. Nothing is wrong
> with your organization. To see what this server can do now, run
> `agentic-org doctor` on the HQ machine.

- The heading is the page's `h1`. `agentic-org doctor` is a code span.
- When the production shell itself is gated, show the logo, heading and sentence
  in one `main`, with **no navigation list and no Back to Home button**: Home is
  gated too. An unavailable feature inside a supported shell keeps that shell's
  navigation.
- The copy is static, so it has **no `role="status"`**.
- Any home link in a supported shell uses the shared focus style (§6a).
- Their titles follow §4: "Agentic Org" for the root and "<Area> · Agentic Org"
  for an area.

## 4. Page titles

| Page | Title |
| --- | --- |
| The root | "Agentic Org" |
| An app area | "<Area> · Agentic Org" |
| A place inside an area | "<Place> · <Area> · Agentic Org", for example "#general · Chat · Agentic Org" (`chat-ux.md` §1.2) |
| Page not found (404) | "Page not found · Agentic Org" |

- **Each history entry carries its own place's title**, never the next one's.
- **Each client-side move announces the new title once.**

## 5. Landmarks

- **Exactly one `main`** per page, named by the page's `h1`, and kept across
  client-side navigation.
- The nav is a **`navigation` landmark named "Main"**.

## 6. Back with a modal open

One rule for every modal: a dialog, a sheet, the nav drawer (§1.3) or Chat's
Conversations overlay (`chat-ux.md` §1.1). **Back with any modal open closes the
modal only.** It doesn't navigate, and focus returns to the control that opened
the modal.

## 6a. Shared visual and interaction rules

**Surfaces and text.** Use a dark gray canvas and one indigo accent. Lists are
flat divided rows, not individually tinted cards; Home's mixed decision/project
cards and the owner account card are exceptions. No gradients, glows or shadows
on dark surfaces. Shared palette tokens, rather than independent hex literals
or department colors, define these roles:

| Role | Design value |
| --- | --- |
| Page, top bar, dialogs and popovers; primary text | gray-800; white |
| Sidebar/profile header | black at 10% over the page |
| Row/header dividers; section outlines | white at 5%; white at 10% |
| Meta/help/placeholders | gray-400 on the page; increase contrast on tints |
| Primary button | indigo-500/white; hover indigo-500 at 80%, never white on indigo-400 |
| Links/focus; text on indigo tints | indigo-400; indigo-300 |
| Form boundaries; error/destructive text | gray-500; red-300 |

Inter Variable is the body font; system mono is for IDs, paths, hashes and code.
Use scalable font units: body/controls 14/24 px, metadata 12/20 px and shell
page/section/dialog headings 16/28 px. No content below 12 px; 10 px is decorative
initials only. Stat numbers and standalone error/onboarding titles may be larger.
Below 640 px inputs use 16 px text to avoid mobile zoom. Align page gutters at
16/24/32 px for mobile/tablet/desktop. At the normal rem baseline, the full sidebar is 288 px wide and the sticky top bar is 64 px high; scale with text and yield to the narrower layout when needed. The content column scrolls. Rows have 16 px vertical padding and a
12 px title-to-meta gap. Lists fill the content width; prose/forms may have a
readable width cap. Header counts are real, plural-correct and baseline-aligned;
optional explanatory prose follows the header. A detail page uses one named
back link, not repeated breadcrumb/title blocks.

**Reflow.** Test 200% text as well as viewport changes, including 16, 32 and
40 px root fonts. Fixed labels wrap between words; a status pill keeps whole
words and wraps below the title instead of shrinking. IDs, paths and user-typed
long names may wrap anywhere. A content-starving desktop sidebar must yield to
the narrower navigation layout even above its normal viewport breakpoint. Do
not impose a minimum browser-window width. An icon and its fixed label stay
together on a line; never shrink them or leave a stranded icon.

**Rows and state.** Show one trailing status pill, optionally with its matching
leading dot. Severity, priority, member kind and version are separate textual
facts, not competing status pills. Amber pills/dots/counts mean **waiting on the
owner**, not generic urgency or a reopened item. Working/in-progress is indigo,
completion/online green, ordinary blocked/error states red, neutral states gray,
and starting/checking sky. Every signal has words; color alone never identifies
the state. Never infer presence from task assignment or an administrative flag.
Agents and people use initial avatars; kind is the word Agent or Person, not a
robot glyph or avatar shape. A team has no invented status. Linked rows have one
stretched title link and independently operable menus/actions. Feed rows expose
actor, action and a `<time datetime>`; truncated text remains available in full.

**Icons and copy.** Use Heroicons outline 24 for navigation/large icons, solid
20 in controls and solid 16 in status/meta. Migrate old Lucide content as its
screen is restyled; do not mix icon families on that screen or add new Lucide
uses. Hide decorative icons from assistive technology; name icon-only controls
and provide tooltips. Close/back/check graphics are icons, not typed pseudo-
icons. Chrome, notices and seed examples contain no emoji. Render agent-authored
messages, summaries and memory/journal excerpts without emoji sequences while
preserving their stored originals; person-authored text is shown as typed.
Use sentence case, with no final period on labels. Everyday work uses project/team/member/task language. Runtime/model/office/process
words belong where configuration, execution or a setup/problem flow is the
subject. Internal subagents are not separate product entities.

**Controls and forms.** Reuse shadcn-svelte/Bits UI primitives. A view has at
most one primary action; global New is outline. Desktop controls have a default
36 px height expressed in scalable rem units, so they grow with text. All
interactive targets are at least 24 px on desktop and 44 px below 640 px. Destructive confirmation names the
object/effect. Disabled controls retain a visible, associated reason or are
omitted. Labels stay visible; help/errors are associated with their fields,
invalid fields use `aria-invalid`, and a refusal focuses the relevant field.
Keep input on failure/disconnect and follow the owning operation's Retry/Check
status contract; a local UI change is not confirmed success. Link tabs use
`aria-current` for routes; in-place filter tabs use `aria-selected`. Selection
has an underline/shape as well as color. Sort/filter use named text buttons and
radio menus, not code-chip controls.

**Feedback.** Empty states explain the place and one real next action. First-
load skeletons match the eventual layout and have an accessible Loading label;
after two seconds show Still loading. Pending buttons retain width, use the
action's -ing verb and expose `aria-busy`. Success toasts require a confirmed
outcome; blocking errors stay inline with next steps and optional copyable
details. Errors explain what failed, why when known and what to do next; never show a raw stack trace. Filtered no-results counts announce politely. Unsupported features
never show invented data or functioning-looking no-op buttons.

**Callouts.** Use quiet neutral callouts by default, with neutral fill, border and text; only the icon carries the tone. At most one attention callout appears at the top below the page header. The shell-wide banner is reserved for live updates paused. User-caused failures use the appropriate status/alert announcement, without duplicating the same message in a live region and toast. Paragraph notes never become amber blocks.

**Focus and overlays.** Keyboard controls use a 2 px indigo-400 outline, 2 px
offset, inset where rows/tabs/menus would clip it; replace low-contrast default
half-opacity rings. Native `:focus-visible` determines the outline. App-focused
headings with `tabindex=-1` show no control ring. Skip to content is first in
keyboard order. Removing an item focuses the next item, else the previous one,
else its list action/heading, never body. Dialogs/sheets trap focus, close on
Escape and restore the opener; keep one overlay at a time. At short heights and
200% text, scrolling content must not hide the title, Close or footer actions.
All dropdown menus are modal: the first outside click closes the menu/restores
its trigger without activating the underlying control; a second click activates
that control. No per-menu click-through exceptions or browser-specific modality
suppression. After keyboard use, menu hover outlines may persist according to
the browser's native focus-visible state.

**Contrast and motion.** Require WCAG 2.2 AA: text 4.5:1, qualifying large text
3:1, meaningful icons/control boundaries/focus 3:1, in hover/selected/tinted
states too. Gray-500 is decorative/border color, not ordinary text. Use red-300
and indigo-300 status text where darker values fail on tints. Only a reported
Working dot may pulse (two seconds); With you does not. Reduced motion stops
pulses, spinners and skeleton animation, removes overlay zoom and smooth
scrolling, and retains state words. Hover/press does not move controls; no
overlay slide lasts longer than 200 ms.

## 7. UX acceptance criteria (SHELL)

**Navigation**
- SHELL-1 · *(new, rev 1)* At a viewport 960 px wide or wider, the full nav shows.
- SHELL-2 · *(new, rev 1)* From 640 to 959 px, the nav is a 64 px icon rail with icon-only items and no expand toggle.
- SHELL-3 · *(new, rev 1)* Each rail item's accessible name is the item's name, and a tooltip with that name shows on hover and on keyboard focus.
- SHELL-4 · *(new, rev 1)* In the rail, `aria-current` marks the current page's item.
- SHELL-5 · In the rail, the logo links to Home and is named "{organization} home". Switching organization is in the account menu.
- SHELL-6 · *(new, rev 1)* Below 640 px, the rail is hidden, and a 56 px top bar shows the page title and a "Menu" button with `aria-expanded` and `aria-controls`.
- SHELL-7 · *(new, rev 1)* Menu opens the full nav as a modal drawer from the left, `min(288 px, 100vw − 56 px)` wide, so the scrim always shows beside it.
- SHELL-8 · *(new, rev 1)* Opening the drawer moves focus to the current page's item, and focus is trapped in the drawer while it's open.
- SHELL-9 · *(new, rev 1)* Esc, the scrim or Close close the drawer and return focus to Menu, whether the drawer was opened by mouse or keyboard, in every browser engine.
- SHELL-10 · *(new, rev 1)* Choosing an item in the drawer closes it and moves focus to the new page's `h1`.
- SHELL-11 · *(new, rev 1)* Back with the drawer open closes the drawer only, doesn't navigate, and returns focus to Menu (SHELL-28).
- SHELL-12 · *(new, rev 1)* A nav item that supplies a badge (an optional count) shows it in both the rail and the full nav. There's no global badge in v1.0.
- SHELL-13 · *(new, rev 1)* The shell is tested at viewport widths 959, 720 (200% zoom of 1440), 639, 600 and 320 (400% zoom of 1280) px.

**The 320 px rule**
- SHELL-14 · *(new, rev 1)* At 320 px, no page scrolls sideways and no text column is narrower than 200 px; where one would be, the panel overlays or the layout stacks.
- SHELL-15 · Two-dimensional content such as the chart scrolls inside its own region. Below 640 px member/agent/task rosters become divided rows; neither they nor the surrounding page require sideways scrolling.
- SHELL-16 · *(new, rev 1)* Below 640 px, dialogs and sheets take the full width, with their buttons wrapped and visible. The nav drawer is the exception (SHELL-7).
- SHELL-17 · *(new, rev 1)* Page toolbars wrap, and search fields are full width below 640 px.

**Unavailable pages**
- SHELL-18 · *(new, rev 1)* The root's unavailable page shows the heading "Agentic Org isn't ready in the browser yet" and the body "This server is running, but this release doesn't serve its pages in the browser yet. To see what it can do now, run `agentic-org doctor` on the HQ machine.", with `agentic-org doctor` in a code span.
- SHELL-19 · *(new, rev 1)* An area's unavailable page shows the heading "<Area> isn't available yet" and the body "This release of the server doesn't serve <Area> in the browser. Nothing is wrong with your organization. To see what this server can do now, run `agentic-org doctor` on the HQ machine.", with `agentic-org doctor` in a code span.
- SHELL-20 · A gated production shell shows its logo and one `main`, no nav list and no Back to Home button. A feature unavailable inside a supported shell keeps that shell's navigation. Static copy has no `role="status"`.
- SHELL-21 · *(rev 2)* Links in supported shells use the shared indigo-400 2 px focus outline (§6a). Root Unavailable pages do not add a home link or navigation list (§3).

**Titles and landmarks**
- SHELL-22 · *(new, rev 1)* An app area's title is "<Area> · Agentic Org", the root's is "Agentic Org", and the 404's is "Page not found · Agentic Org".
- SHELL-23 · *(new, rev 1)* A place inside an area is titled "<Place> · <Area> · Agentic Org".
- SHELL-24 · *(new, rev 1)* Each history entry carries its own place's title, never the next one's.
- SHELL-25 · *(new, rev 1)* Each client-side move announces the new title once.
- SHELL-26 · *(new, rev 1)* Each page has exactly one `main` landmark, named by the page's `h1`, and it's kept across client-side navigation.
- SHELL-27 · *(new, rev 1)* The nav is a `navigation` landmark named "Main".

**Back with a modal open**
- SHELL-28 · *(new, rev 1)* Back with any modal open (a dialog, a sheet, the nav drawer or Chat's Conversations overlay) closes the modal only, doesn't navigate, and returns focus to the control that opened it.

**Shared UI standards**
- SHELL-29 · Supported screens use §6a's dark palette, scalable typography and aligned gutters; lists are flat, and contrast passes in default/hover/selected states.
- SHELL-30 · At 320 and 960 px with 100%/200% text including 32/40 px roots, labels, pills, icons and actions stay readable/operable without page overflow. Content-starving desktop navigation reflows.
- SHELL-31 · Rows expose one status word/signal; amber means an owner action is needed. Severity, priority and member kind are distinct text, not competing status pills.
- SHELL-32 · A restyled screen uses one Heroicons family, named icon-only controls and wrapping icon/label units. No UI emoji; agent emoji filtering changes rendering only, preserving stored source; person text is unchanged.
- SHELL-33 · Desktop controls default to 36 px high in rem units and scale with text; every interactive target is at least 24 px on desktop and 44 px below 640 px. Mobile input text is 16 px. Forms associate labels/help/errors, focus refused fields and retain drafts after failure.
- SHELL-34 · Link tabs and in-place tabs expose distinct semantics; selection isn't color-only, disabled actions have a reason, and one view has at most one primary action.
- SHELL-35 · Loading uses real-layout skeletons and a two-second Still loading message; pending actions expose busy state, confirmed success is required for a success toast, and errors explain what failed/why/what next without raw stack traces. Callouts stay neutral, with at most one top attention callout and no duplicated live-region/toast announcement; only live updates paused uses a shell-wide banner.
- SHELL-36 · Keyboard focus is visible/unclipped, Skip to content is first, and deletion focuses a surviving row or list action/heading, never body.
- SHELL-37 · Every menu closes on the first outside click without activating the control beneath. A second click can activate it. Native focus-visible behavior holds in Chromium, Firefox and WebKit.
- SHELL-38 · At short heights/200% text, dialogs/sheets keep the title, Close and footer actions reachable while content scrolls, and trap/restore focus.
- SHELL-39 · Reduced motion stops pulses/loading animations/overlay zoom/smooth scrolling without removing the state word; otherwise only Working may pulse.

## 8. Open questions

None open.
