---
kind: spec
title: "App shell: navigation, narrow widths, unavailable pages and page titles (UX)"
status: for-review
revision: 1
owner: ac-product-designer-eden
created: "2026-11-10"
updated: "2026-11-10"
requested_by: ac-product-designer-eden
reviewers: [ac-staff-engineer-ada, owner]
builds_on: [our-org-in-the-app, offices-and-teams-ux]
---

# Spec · App shell: navigation, narrow widths, unavailable pages and page titles (UX)

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

**Revision 1 (2026-11-10): new spec.** Navigation by viewport width (full nav,
icon rail, top bar with a nav drawer), the 320 px rule (WCAG 1.4.10), the
unavailable pages' copy, page titles and landmarks. **New:** SHELL-1 to SHELL-27.

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
- The org switcher becomes the **logo button** at the top of the rail.
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
- **Back** with the drawer open closes it and moves focus to the new page's `h1`.

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
  it): the org chart canvas, and wide tables such as the Agents roster. The page
  around it doesn't scroll.
- **Dialogs and sheets take the full width below 640 px**, with their buttons
  wrapping and still visible. The nav drawer is the exception (§1.3).
- **Page toolbars wrap**, and **search fields are full width** below 640 px.

## 3. Unavailable pages

Production areas the gateway doesn't serve show an unavailable page instead of
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
- These pages **keep the nav**, so the other areas stay reachable: a **text nav
  that wraps**, with no Menu and no buttons, at every width.
- The copy is static, so it has **no `role="status"`**.
- The **home link** uses the `--ring` focus style (2 px, 2 px offset).
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

## 6. UX acceptance criteria (SHELL)

**Navigation**
- SHELL-1 · *(new, rev 1)* At a viewport 960 px wide or wider, the full nav shows.
- SHELL-2 · *(new, rev 1)* From 640 to 959 px, the nav is a 64 px icon rail with icon-only items and no expand toggle.
- SHELL-3 · *(new, rev 1)* Each rail item's accessible name is the item's name, and a tooltip with that name shows on hover and on keyboard focus.
- SHELL-4 · *(new, rev 1)* In the rail, `aria-current` marks the current page's item.
- SHELL-5 · *(new, rev 1)* In the rail, the logo button is the org switcher.
- SHELL-6 · *(new, rev 1)* Below 640 px, the rail is hidden, and a 56 px top bar shows the page title and a "Menu" button with `aria-expanded` and `aria-controls`.
- SHELL-7 · *(new, rev 1)* Menu opens the full nav as a modal drawer from the left, `min(288 px, 100vw − 56 px)` wide, so the scrim always shows beside it.
- SHELL-8 · *(new, rev 1)* Opening the drawer moves focus to the current page's item, and focus is trapped in the drawer while it's open.
- SHELL-9 · *(new, rev 1)* Esc, the scrim or Close close the drawer and return focus to Menu, whether the drawer was opened by mouse or keyboard, in every browser engine.
- SHELL-10 · *(new, rev 1)* Choosing an item in the drawer closes it and moves focus to the new page's `h1`.
- SHELL-11 · *(new, rev 1)* Back with the drawer open closes it and moves focus to the new page's `h1`.
- SHELL-12 · *(new, rev 1)* A nav item that supplies a badge (an optional count) shows it in both the rail and the full nav. There's no global badge in v1.0.
- SHELL-13 · *(new, rev 1)* The shell is tested at viewport widths 959, 720 (200% zoom of 1440), 639, 600 and 320 (400% zoom of 1280) px.

**The 320 px rule**
- SHELL-14 · *(new, rev 1)* At 320 px, no page scrolls sideways and no text column is narrower than 200 px; where one would be, the panel overlays or the layout stacks.
- SHELL-15 · *(new, rev 1)* Two-dimensional content (the org chart canvas, and wide tables such as the Agents roster) may scroll inside its own region, and the page around it doesn't scroll sideways.
- SHELL-16 · *(new, rev 1)* Below 640 px, dialogs and sheets take the full width, with their buttons wrapped and visible. The nav drawer is the exception (SHELL-7).
- SHELL-17 · *(new, rev 1)* Page toolbars wrap, and search fields are full width below 640 px.

**Unavailable pages**
- SHELL-18 · *(new, rev 1)* The root's unavailable page shows the heading "Agentic Org isn't ready in the browser yet" and the body "This server is running, but this release doesn't serve its pages in the browser yet. To see what it can do now, run `agentic-org doctor` on the HQ machine.", with `agentic-org doctor` in a code span.
- SHELL-19 · *(new, rev 1)* An area's unavailable page shows the heading "<Area> isn't available yet" and the body "This release of the server doesn't serve <Area> in the browser. Nothing is wrong with your organization. To see what this server can do now, run `agentic-org doctor` on the HQ machine.", with `agentic-org doctor` in a code span.
- SHELL-20 · *(new, rev 1)* Unavailable pages keep a text nav that wraps, with no Menu and no buttons, and their static copy has no `role="status"`.
- SHELL-21 · *(new, rev 1)* On unavailable pages, the home link has the `--ring` focus style (2 px, 2 px offset).

**Titles and landmarks**
- SHELL-22 · *(new, rev 1)* An app area's title is "<Area> · Agentic Org", the root's is "Agentic Org", and the 404's is "Page not found · Agentic Org".
- SHELL-23 · *(new, rev 1)* A place inside an area is titled "<Place> · <Area> · Agentic Org".
- SHELL-24 · *(new, rev 1)* Each history entry carries its own place's title, never the next one's.
- SHELL-25 · *(new, rev 1)* Each client-side move announces the new title once.
- SHELL-26 · *(new, rev 1)* Each page has exactly one `main` landmark, named by the page's `h1`, and it's kept across client-side navigation.
- SHELL-27 · *(new, rev 1)* The nav is a `navigation` landmark named "Main".

## 7. Open questions

None open.
