Rowan, design system v2.1 (7de9183, Emery's rev 4) changes some of the freeze notes I sent you. ada is copied.

1. **Owner footer (D21).** It's no longer a switcher link. It's a `<button aria-haspopup="menu">` named "You · Owner, account menu", opening a DropdownMenu upward: Your preferences, Switch organization (goes to /welcome), About, Sign out. Items whose flow doesn't exist yet are left out. Esc returns focus to the button. There's no `/members/you`.
2. **Rail (640–959).** The logo links to /home. The footer collapses to a 48 px owner-avatar button that opens the same menu, plus a 48 px office link to /settings/offices: `building-office` with a status badge, named and tooltipped with the office line. Never colour alone.
3. **Short-window sidebar (new).** The list measures its expanded height with a ResizeObserver: on the ul, after fonts load and when contents change, with 24 px hysteresis. If it doesn't fit, the Teams sub-list collapses first, then Projects only if still needed. Top-level items never scroll away while sub-items show. Focus inside a collapsing sub-list moves to its parent, and the parent becomes current.
4. **Projects sub-list (D35).** A–Z with no "Recent" label (`aria-label="Projects"`) until projects carry last-activity, which is your item.
5. **⌘N (IA-19).** `aria-keyshortcuts="Meta+N"` (Control+N on Linux) in the desktop app only. The web build doesn't declare it, because the browser owns ⌘N.

Notes 2–5 from my last message stand: sidebar scroll, pulse ::after, the /processes redirect, and the v2 notes. Note 4 changed: Agent setup now has a Health column (D20) instead of no column.
