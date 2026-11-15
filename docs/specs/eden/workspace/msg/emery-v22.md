Thanks. Your rev 4.1 decisions are in design-system v2.2, committed locally as a2d9a3e (projects/ac/agents/ac-product-designer-eden/knowledge/design-system.md).

What changed:
- New ▾ opens on plain N.
  - Guards: no Ctrl, Alt or ⌘ held, and focus isn't in a text field (input, textarea, select, contenteditable or the session terminal).
  - On non-Latin layouts it falls back to the physical N key.
  - N doesn't fire with the drawer, a menu or a dialog open. Either case works, and it's ignored during IME composition.
  - `aria-keyshortcuts="N"`, with the tooltip "New · N".
  - No ⌘N in v1.0, the Tauri window included; it comes only with a desktop shell.
- The "for Emery" labels on the Mine fallback, Needs setup over Paused and the account menu are removed, and so is the AC-IA05-2 note.
- Sign out appears only once the gateway has owner sign-in.
- Review requested:
  - It's in the Approvals kinds, with Approve / Request changes. Both send the owner's reply, per Rowan's option (a).
  - It clears once answered, here or anywhere, and can't be dismissed.
  - Work's Needs me points to it.
- Tight step: I kept it. It's the last step before the list scrolls, after Teams and then Projects collapse.
  - The panel goes from gap-y-5 to gap-y-2 (20→8 px), and the groups from gap-y-7 to gap-y-3 (28→12 px).
  - Items keep 40 px, and drawer targets keep 44 px.
  - It uses the same measuring and 24 px hysteresis, and it's the first step undone when space returns.

One addition for you (WCAG 2.1.4, Character Key Shortcuts):
- A single-key shortcut must be possible to turn off or remap, unless it only works while its control has focus. N and our existing `/` for search both qualify.
- I added a "Single-key shortcuts" switch (on by default) under a Keyboard section in Your preferences.
  - When it's off, N and `/` do nothing outside text fields.
  - The New button and the search input then drop `aria-keyshortcuts` and the hint.
- It isn't in D21 or any story yet. Could you add an AC (under AC-IA19-2 or the preferences story) so it gets built with N? I wrote that N ships only together with the switch. `/` already ships, so it's a known 2.1.4 gap until then.

One small question: what reply text should Approve send to the agent on a Review requested item? "Approved" is my default. Request changes sends the owner's text.

The independent review is READY after a few rounds. Your message 01M76FYH is closed on my side.
