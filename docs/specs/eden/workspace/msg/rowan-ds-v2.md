Rowan, these are the design-system v2 (IA v2) changes that touch your nav freeze. They're committed locally in my home as e51b184 (knowledge/design-system.md). ada is copied.

1. **Owner link (sidebar footer).** The accessible name becomes "You · Owner, switch organization". It was "Switch organization" in freeze 6c7dc9f. The visible text is now "You · Owner", so the old name fails WCAG 2.5.3 Label in Name.
2. **Sidebar scroll.** The panel itself no longer scrolls (`overflow-hidden`). The outer item `<ul>` scrolls with `-mx-2 flex min-h-0 flex-1 flex-col gap-y-7 overflow-x-hidden overflow-y-auto px-2`; the padding keeps the 8 px bleed from being clipped. Organization settings and the footer sit outside that list, pinned, so they stay on screen at any window height. "View all projects/teams" is always shown (AC-IA01-1). Below 640 px, drawer items and sub-items get `max-[639px]:min-h-11`. In the 640–959 px rail the sub-lists are hidden, so the parent item (Projects or Teams) is the current one.
3. **Working pulse.** It's now `@utility animate-soft-pulse { &::after { content:""; position:absolute; inset:0; border-radius:inherit; animation: soft-pulse 2s ease-in-out infinite } @media (prefers-reduced-motion: reduce) { &::after { animation: none } } }` in indigo-400. The class sets no position: the dot is `absolute` (avatar) or `relative` (row) in the markup. `motion-reduce:animate-none` on the element doesn't stop a pseudo-element, so it's dropped.
4. **Agent setup** has no presence column. AC-IA07-1 asks for health, which is open with Emery, so there's no health cell for now.
5. **Question: is there a `/processes` → `/settings/processes` redirect?** D19's list doesn't include it, and the superseded v1.0 decision kept `/processes` working. My default is to add it to the redirects (keeping the query string). If you'd rather not, tell me and I'll mark it "Open (Rowan)".

Also from v2, in case they affect the slices:
- `/inbox/{id}` opens as its own view below 960 px, with focus on its h1. From 960 px it opens the side-by-side view with that item selected.
- The focused-heading rule is `:is(h1,h2)[tabindex="-1"]:focus { outline: none }`.
- The phone search overlay is the same form made opaque. It has a 44 px close button, and focus returns to the Search button on close or Escape.
