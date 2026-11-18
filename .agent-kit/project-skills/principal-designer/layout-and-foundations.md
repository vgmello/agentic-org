# Page layout and visual foundations

Reference for the [principal designer](SKILL.md). Each rule gives its reason and when it
breaks. Values fixed by `docs/specs/app-shell-ux.md` section 6a win over the general ranges
here.

## Laying out a page

### Start from the content, not the frame

1. Write the page's one job as a sentence.
2. List what the person needs in order to do it, and rank the list. That rank is the
   reading order.
3. Decide what is always visible, what appears on demand and what belongs on another page.
4. Only then choose a pattern and place things.

A page arranged by data source ("everything from the tasks service, then everything from
chat") makes the person do the ranking. Arrange by what they need next.

### What a page is made of

- **Page header.** A title that names the place using the same words as the link that led
  here. A count when the page is a collection. At most one primary action. Further actions
  go into an outline button or a menu. *Breaks when* the page is a single form or document:
  the primary action then belongs at the end of the form, where the person finishes.
- **Controls for the collection.** Search, filter, sort and view switches sit directly
  above the content they change, never in the page header with the create action. People
  read a control as acting on what is below and next to it.
- **Content.** The largest region, and the only one that scrolls.
- **Aside.** Supporting detail for the selected item. It is the first thing to go at
  narrow widths: it becomes an overlay or moves below.

### The first screen

- Without scrolling, the person should be able to answer: where am I, what is here, what
  can I do.
- **People do scroll**, when the visible part signals there is more. A full-width band, a
  large gap or a block that ends exactly at the bottom edge looks like the end of the page.
  Let content be cut by the edge.
- Put what needs the person's decision above what merely informs.
- *Breaks when* the page is reference material people search within. Then the first screen
  is navigation: a table of contents and search.

### Page patterns

| Pattern | Use it when | It breaks when |
| --- | --- | --- |
| **Collection, then detail on its own page** | Items are worked on one at a time and each has depth. The detail gets a URL and a full width. | People triage many items quickly; the round trip per item is too slow. |
| **Split view (list beside detail)** | People move through many items in a row: an inbox, a review queue. Selection stays visible beside the content. | The width cannot hold both (stack or overlay below the breakpoint), or the detail needs the full width. |
| **Home or dashboard** | The person needs "what needs me, and what changed" across areas. Order blocks by how actionable they are. | It becomes a wall of equal tiles. A tile nobody acts on is decoration; remove it. |
| **Detail page** | One object. Identity, status and the primary action at the top, then sections or tabs for its facets. | Tabs hide something needed to decide the primary action. Bring that fact up to the header. |
| **Settings** | Many independent values. Group by subject, name each group with a noun, keep one save model per page. | Mixing immediate switches with a Save button on one page: people cannot tell what has been saved. |
| **Single form** | The task is one coherent thing and the fields do not depend on each other. Fastest for people who do it often. | Later questions depend on earlier answers, or the form is long enough to intimidate. |
| **Step-by-step flow** | The task is rare, has dependencies between answers, or carries risk. Each step is one decision; progress and a way back are always visible. | Experts do it daily. Steps then cost more than a single form. |
| **Feed or log** | Events in time order, read from the newest. | People are looking for one item. Add filtering and search, or use a table. |

### Regions and scrolling

- **One scrolling region per page** wherever possible. Nested scroll areas trap the wheel
  and hide content. *Breaks for* two-dimensional content such as a chart canvas or a wide
  table, which scrolls inside its own region while the page around it stays still.
- **Keep the consequence and the action in view together.** On a long page a sticky header
  or footer may carry the total and the primary action. *Breaks at* short viewports and
  large text, where sticky bars can cover the content they refer to. Then let them scroll.
- **Product layout is usually three parts**: fixed navigation, a fluid content column, an
  optional fixed-width aside. A twelve-column grid earns its keep on marketing pages and
  complex dashboards, and is overhead elsewhere.
- **Reading starts at the top leading corner and ends at the bottom trailing corner.** Put
  orientation where reading starts and the final action where it ends.

### Navigation and structure

- **Organize by the person's objects and tasks**, not by your teams or your services. The
  test: a new person can predict what is behind each label.
- **Broad and shallow beats narrow and deep.** Each level is a decision made on words
  alone, and people get lost in depth. *Breaks when* one level has more items than can be
  scanned without scrolling or told apart at a glance. Then group.
- **Visible navigation beats hidden navigation.** Destinations behind a menu button are
  used less and discovered later. *Breaks on* a phone, where the space is needed for
  content. Keep the few most used destinations visible even there if you can.
- **Always mark where the person is**, with more than color.
- **Labels are the nouns people already use.** If two labels could plausibly hold the same
  thing, one of them is wrong.
- **A sidebar** suits many destinations and frequent switching. **Top-level tabs** suit a
  few peers. **Tabs inside a page** are facets of one object, never steps in a sequence.

### Responsive behavior

- **Breakpoints go where the content breaks**, not at device names.
- **Stacking order is priority order.** When columns collapse, the more important one comes
  first, which is not always the left one.
- Decide for each region what it does when narrow: stack, become an overlay, or scroll
  inside itself. A table of people becomes divided rows. A canvas scrolls in place.
- Type and controls scale with the person's text size. Test at 200% text, not only at
  narrow widths.

## Spacing

- **Base 8, fine-tune with 4.** A fixed step gives rhythm and removes arbitrary numbers.
  *Breaks in* dense tables, dashboards and inline icon gaps, which need 4 px steps; on very
  small screens, where strict 8 px steps are chunky; and in expressive or editorial work,
  which is spaced by eye.
- **Spacing states relationships.** Label to its field is tighter than field to field,
  which is tighter than section to section. Equal gaps claim equal relationships.
- **Padding inside a container is at most the gap outside it**, or the contents look
  closer to the neighbor than to their own box.
- **Few alignment edges.** One strong leading edge beats centered layouts for anything
  scanned. *Breaks for* a single short message standing alone, such as an empty state or an
  error page, which may be centered.
- **Align optically where the math looks wrong.** Icons with uneven visual weight, text in
  a pill and round shapes next to square ones need a nudge.
- **Nested corners.** Inner radius equals outer radius minus the padding between them.
- **Density is chosen per task.** Comfortable for occasional use, compact for repeated
  expert scanning. Never get density by shrinking type below legibility or targets below
  the minimum.

## Typography

- **A scale stops sizes from looking random.** A ratio of 1.25 is a sound default.
  *Breaks in* product UI, which usually wants a tighter 1.125 to 1.2 or simply a short
  fixed list of sizes; on marketing and editorial pages, which take much larger,
  non-modular jumps; and between phone and desktop, which rarely suit the same ratio.
- **Build hierarchy with weight and color before size.** Two weights are usually enough.
  Secondary text is more often the same size in a quieter color than it is smaller.
- **Reading text: 45 to 75 characters per line, line-height about 1.4 to 1.6.** Shorter
  lines make the eye jump; longer ones lose the return to the next line. *Breaks for*
  labels, buttons, cards and headlines, which are not paragraphs; for CJK and Arabic
  scripts, which measure differently; and by typeface, since x-height and weight change the
  leading a face needs. Leading tightens as size grows, toward 1.1 to 1.3 for headings.
- **Tracking.** Tighten large display sizes slightly, open up small all-caps labels, leave
  body text alone.
- **Numbers.** Tabular figures in columns and in anything that updates in place.
  Right-align numeric columns so magnitudes compare.
- **Avoid** centered paragraphs, justified text on the web and all caps beyond a short
  label. All caps removes the word shapes that make scanning fast.
- **Legibility floor.** About 12 px for anything meant to be read. Inputs below 16 px make
  mobile browsers zoom.
- **Long values.** Decide per field whether it wraps, truncates or clamps, and keep the
  full value reachable. Names wrap between words. IDs and paths may wrap anywhere.

## Color, contrast and elevation

- **Design in grayscale first, then add color for meaning.** If hierarchy fails without
  hue, hue will not fix it.
- **60/30/10 is a starting proportion.** It stops three colors fighting. *Breaks for*
  minimal products that run nearer 90/10, for dark themes, for brands with two strong
  colors and for illustration-led products where color is the content. It says nothing
  about contrast, which matters more than proportion.
- **Contrast is a threshold.** WCAG 2.2 AA: 4.5:1 for text, 3:1 for large text, 3:1 for
  meaningful icons, control boundaries and focus indicators. Check hover, selected, tinted
  and placeholder states, which is where it usually fails. The ratio is a floor: thin or
  small text can pass and still be hard to read.
- **Never color alone.** About one man in twelve has a color-vision deficiency, most often
  confusing red and green. Pair color with a word, an icon or a shape.
- **One meaning per hue.** If amber means "waiting on you", it cannot also mean "new".
- **Shadows match their environment.** On light surfaces pure black shadows look dirty:
  tint them toward the background or brand hue and layer a tight one under a soft one.
  *Breaks on* strongly colored backgrounds, where a tint goes muddy, and on dark surfaces,
  where shadows barely register. There, show elevation with a lighter surface and a
  hairline border.
- **On dark themes**, saturated colors vibrate and fail contrast on tints: use the lighter,
  less saturated steps for text and status. Large areas of pure white text on pure black
  glare. Keep one or the other off the extreme.
- **Borders can be lower contrast than you first make them**, and most layouts need fewer.
  Decorative dividers are exempt from 3:1. A boundary a person needs in order to find a
  control is not.

## Access beyond contrast

These are thresholds from WCAG 2.2, level A and AA. They are requirements, and each one
also makes the product better for everyone.

- **Everything works from the keyboard**, in an order that matches what is seen, with a
  visible focus indicator at every step.
- **The focused control is never hidden** behind a sticky header, footer, banner or
  overlay.
- **Headings form an outline.** One per page at the top level, no skipped levels. People
  using a screen reader navigate by headings the way sighted people scan them.
- **Every control has a name, every field a label, every meaningful image a text
  alternative.** Decorative images and icons beside visible text are hidden from assistive
  technology.
- **Content reflows at 320 px and at 200% text** without loss and without scrolling in two
  directions.
- **Anything done by dragging can also be done with single clicks or the keyboard.**
- **Do not ask twice.** Information the person already gave in the same process is filled
  in or offered for selection.
- **Signing in does not depend on memory or a puzzle.** Allow paste and password managers.
- **Help sits in the same place on every page.**
- **A changed count or status is announced** as a complete phrase, once, without moving
  focus.
