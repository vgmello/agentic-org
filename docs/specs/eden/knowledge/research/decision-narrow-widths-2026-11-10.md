# Narrow widths: product decision (Eden, 10 Nov), for BUG-52 (Jules), BUG-53 (Jules) and BUG-49 (Avery). Breakpoints are viewport widths in CSS px. I'll add this to the spec as #50; build to this message meanwhile.

SHELL NAV (BUG-52, Jules)
- 960 and wider: the full nav, as today.
- 640 to 959: the nav collapses to a 64 px icon rail. Each item is the icon only, with its name as the accessible name and a tooltip on hover and focus. aria-current marks the current page, and the "Needs you" count badge stays on the icon. The org switcher becomes the logo button. No expand toggle.
- Below 640: the rail is hidden. A 56 px top bar has a "Menu" button (aria-expanded, aria-controls) and the page title. Menu opens the full nav as a modal overlay from the left. Focus goes to the current item and is trapped while open. Esc, the scrim or choosing an item closes it, and focus returns to Menu (or, after choosing an item, to the new page's h1).
- Test widths: 959, 720 (that's 200% zoom of 1440), 639, 600 and 320 (that's 400% zoom of 1280).

320 px EXPECTATION (WCAG 1.4.10), everyone
- The page never scrolls sideways, and no text column is narrower than 200 px. Where a column would be, the panel overlays instead.
- Two-dimensional content may scroll inside its own region (1.4.10 exempts it): the org chart canvas and wide tables such as the Agents roster. The page around it doesn't scroll.
- Dialogs and sheets take the full width below 640, with their buttons wrapping and still visible.

CHAT (BUG-49, Avery): your proposal is approved, with these details.
- Content 768 px and wider (the area right of the nav): the conversation list is inline at 256 px. Below that, it's a 256 px overlay, capped at the available width.
- Toggle: a "Conversations" button left of the conversation title. When something needs the owner, its accessible name adds the count ("Conversations, 2 need you"), with the same amber badge as the list.
- Opening: the list overlays the conversation from the left (modal), and focus goes to the selected conversation's item.
- Closing: choosing a conversation closes it and moves focus to that conversation's heading. Esc, Close or the scrim close it and return focus to the toggle.
- Thread and Members stay overlays below 1280, as now, and below 640 they take the full width.

UNAVAILABLE PAGES (BUG-53, Jules): exact copy.
- Root "/", heading: "Agentic Org isn't ready in the browser yet"
  Body: "This server is running, but this release doesn't serve its pages in the browser yet. To see what it can do now, run agentic-org doctor on the HQ machine."
  ("agentic-org doctor" in a code span.)
- An area, heading: "<Area> isn't available yet"
  Body: "This release of the server doesn't serve <Area> in the browser. Nothing is wrong with your organization. To see what this server can do now, run agentic-org doctor on the HQ machine."
- Tab titles: "Agentic Org" for the root, and "<Area> · Agentic Org" for an area. The 404 gets "Page not found · Agentic Org".
- Keep the nav, so the other areas stay reachable. No buttons. role="status" isn't needed on static copy, so drop it.
- N3: the header link uses the --ring focus style (2 px, 2 px offset).

Questions to me.

## Follow-ups (same day)
- (a) The nav drawer is min(288 px, 100vw − 56 px) wide; the full-width rule is for dialogs and sheets only.
- (b) There's no global "Needs you" badge in v1.0. The rail and the full nav render a badge only when a nav item supplies one.
- (c) The unavailable pages keep their wrapping text nav, with no Menu.
- (d) Back with any modal open (dialog, sheet, drawer or Conversations overlay) closes only the modal and returns focus to its opener. This replaces the earlier "focus the new page's h1". Spec: SHELL-28.
- (e) The countdown is rounded down to 10 s steps and reads "Restarting shortly" under 10 s.
