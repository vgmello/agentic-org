# Tailwind

Reference for the [principal designer](SKILL.md). You write Tailwind as a design tool and
can read a class string as a rendered result. This project pins Tailwind CSS v4 with
tokens in `apps/agentic-org-ui/src/app.css`, and builds component variants with
`tailwind-variants` and `cn()`.

## Reading the scales as design decisions

- **The spacing scale is the grid.** One unit is 4 px, so `2`, `4`, `6`, `8` are the 8 pt
  steps and `1`, `3`, `5` are fine-tuning. Prefer `gap-*` on a flex or grid parent to
  margins on children: the parent then owns the rhythm. An arbitrary value such as
  `mt-[13px]` needs a reason you can state.
- **Type sets size and leading together.** `text-sm/6` is 14/24, `text-xs/5` is 12/20 and
  `text-base/7` is 16/28, which are this product's body, metadata and heading sizes. Use
  `font-medium` or `font-semibold` for hierarchy, `tabular-nums` for figures,
  `text-balance` for headings, `text-pretty` for paragraphs and `max-w-prose` for the
  reading measure.
- **Targets.** `size-6` is 24 px, `size-9` is 36 px, `min-h-11` or `size-11` is 44 px. Raise
  targets for touch with a narrow-only variant.
- **Sizes are rem.** They grow with the person's text size, which is what you want. A fixed
  pixel height in brackets does not, so it needs a reason.

## Color and surfaces

- **Tokens before palette.** Use the semantic utilities the project defines
  (`bg-background`, `text-muted-foreground`, `border-border`, `bg-primary`,
  `text-destructive`) so a role changes in one place. Reach for a raw palette step only
  where section 6a of the app shell spec names one.
- **Opacity modifiers make tints that follow the surface**: `border-white/10`, `bg-white/5`,
  `bg-black/10`. Here row dividers are `divide-y divide-white/5` on the list, not a border
  on every row.
- **No shadow, gradient or glow utilities on dark surfaces in this product.** Elevation is a
  lighter surface plus a hairline border or ring.
- **Check contrast on the tint, not on the page.** Text that passes on the page background
  can fail on `bg-indigo-500/10`.

## Layout

- **Flex for one dimension, grid for two.** `grid` with named column counts for forms and
  galleries, `flex` for rows of controls.
- **Overflow is a design decision.** Put `min-w-0` on the flex child that should shrink,
  then `truncate` or `line-clamp-*`. `shrink-0` keeps icons and pills whole.
  `wrap-anywhere` is for IDs and paths. Words in a label wrap between words.
- **Responsive.** Mobile-first breakpoint variants. This product's navigation breakpoints
  are 640 and 960 px; the code uses `sm:` and `max-[640px]:` for the first and
  `min-[960px]:` and `max-[960px]:` for the second. Match what the surrounding code does.
- **Container queries** (`@container` on the parent, `@md:` on the child) when a
  component's layout depends on its own width, such as a card used in both a wide page and
  a narrow aside.
- **One scroll region.** `overflow-y-auto` on the content column, `overflow-x-auto` only on
  a region holding two-dimensional content.

## State

- **Focus.** Style `focus-visible:` and never remove an outline without replacing it. The
  product's pattern is `focus-visible:outline-2 focus-visible:outline-offset-2
  focus-visible:outline-indigo-400`, with `-outline-offset-2` where a row, tab or menu
  would clip it.
- **Drive styles from real state** so the look cannot disagree with what assistive
  technology reports: `aria-invalid:`, `aria-busy:`, `aria-selected:`,
  `aria-[current=page]:`, `data-[state=open]:`, `disabled:`.
- **Relationships.** `group` and `group-hover:` for a row whose parts react together,
  `peer-*` for a field and its message, `has-[...]` for a container that reflects a child's
  state.
- **Hover is an enhancement.** Anything revealed by `group-hover:` must also appear on
  `group-focus-within:` and be reachable on touch.
- **Screen-reader text** uses `sr-only`. Decorative icons take `aria-hidden="true"`.

## Motion

- `transition-colors` with `duration-150` for small state changes. Avoid `transition-all`,
  which animates layout properties you did not intend.
- Wrap movement in `motion-safe:` or cancel it with `motion-reduce:`.
- No `hover:scale-*` or `active:translate-*` on controls in this product: hover and press
  do not move them.

## Components and variants

- **Use the existing primitive first.** Button, input, label, checkbox, dialog, sheet,
  dropdown menu, alert, badge and toast live under `src/lib/components/ui`.
- **Express variants with `tailwind-variants`**, named for their role (`default`,
  `outline`, `ghost`, `destructive`), never for their color. Merge caller classes with
  `cn()`.
- **Do not concatenate class names from data.** Tailwind only generates classes it can
  find as whole strings in the source. Map a status to a complete class string.
- **Keep class strings readable**: layout, then box, then type, then color, then state.
  When a string cannot be read aloud as a description of the element, extract a component.

## Tailwind v4

- Theme values live in CSS under `@theme` and are available as CSS variables
  (`var(--color-indigo-500)`).
- Several utilities were renamed from v3: the shadow and radius scales shifted one step
  down (`shadow-sm` became `shadow-xs`, `rounded-sm` became `rounded-xs`), `outline-none`
  became `outline-hidden`, and bare `ring` is now 1 px (`ring-3` for the old width).
- The spacing scale accepts any whole number of units, so `p-13` works without
  configuration. That makes off-grid values easy to write by accident.
- Check the installed version and the project's own usage before trusting memory.
