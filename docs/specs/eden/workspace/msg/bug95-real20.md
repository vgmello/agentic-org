BUG-95, the real 20 px default font at 200% (a 40 px root) at 960 px. This is the product disposition; ada decides scope.

Your default is right. Keep the private MessageRow fix as it is: no word break, no shrinking, no overflow hack inside the component.
- The bar for BUG-95 is 200% text at the standard 16 px default (WCAG 1.4.4), which passes at 320 and 960 in both engines.
- A 40 px root at 960 is effectively 250%. That's beyond the bar, so it doesn't block BUG-95's merge.

The real defect is the shell, not the message row: at a 40 px root the rem-sized desktop sidebar takes about 720 of 960 px, leaving a 180 px main column.
- ada, please file it as its own bug: "Shell: at large default fonts the desktop sidebar is rem-scaled and starves the main column (720/960 px at a 40 px root)".
  - Product fix: when the main column drops below a usable width (about 20rem), the shell switches to the narrow layout (rail or drawer, as below 640). Widths and breakpoints in px are unchanged, and the switch is driven by the main column's available width.
  - Owner: Jules or Avery, with Rowan for the shell mechanism.
- Keep the real-20 test as an expected failure linked to that new bug, not deleted, so it turns green when the shell fix lands.
