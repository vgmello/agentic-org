# Components

Reference for the [principal designer](SKILL.md): what each component should look like,
when to choose it, and when the usual rule breaks. Values fixed by
`docs/specs/app-shell-ux.md` section 6a win over the general ranges here. Reuse the
primitives in `apps/agentic-org-ui/src/lib/components/ui` before drawing a new one.

## Buttons

### Anatomy

- **Height in a short scale**, shared with inputs so they line up in a row: about 32 px
  compact, 36 to 40 px default, 44 to 48 px for touch. Express it in rem so it grows with
  the person's text size.
- **Horizontal padding is roughly the text size** (12 to 16 px for 14 px text), which comes
  out near twice the visual vertical padding. Less looks cramped; much more looks like a
  banner.
- **Icon and label.** The icon is about the label's line height or slightly smaller (16 px
  with 14 px text, 20 px with 16 px, in general; this product uses solid 20 px icons in
  controls and solid 16 px in status and metadata). The gap is 6 to 8 px. Trim the padding on the icon's
  side by a couple of pixels, because an icon carries its own empty space.
- **Label.** Medium or semibold weight, one to three words, written to fit on one line and
  never truncated: shorten the words instead. In this product a fixed label wraps between
  words at narrow widths and large text; it does not shrink or clip.
- **Radius** is the same family as inputs and cards. A small radius reads neutral, a full
  pill reads friendly and consumer, square reads formal. Pick one and hold it.
- **Minimum width.** A two-letter label should not make a tiny target. *Breaks for*
  icon-only buttons, which are square.
- **The target may be larger than the shape.** WCAG 2.5.8 (level AA) requires 24 by 24
  CSS px. WCAG 2.5.5 (level AAA) and Apple ask for 44; Material asks for 48. This product
  requires 24 px on desktop and 44 px below 640 px. Meet the target by making the
  interactive element that size and drawing a smaller shape inside it, not by inflating
  what is drawn. A button grown to 44 px
  inside a container that has its own padding makes the container 52 px, and it becomes the
  heaviest thing in its row.
- **Separate touch targets need room between them**, about 8 px, so a thumb does not land
  on the neighbor. *Breaks when* they are segments of one control, which touch by design.
- **Controls that sit together share one outer height.** A search field, a segmented
  control, a select and a button in the same row or stack have the same outer height and
  the same radius family, so they read as one set. Measure the outer box, including a
  group's own padding and border. After any size fix, look at the control beside its
  neighbors again. *Breaks when* one control is deliberately the primary action and larger
  by a whole step.

### Hierarchy

- **One primary action per view.** Weight ranks actions: filled, then outline or tonal,
  then ghost or text. Two filled buttons side by side means nobody decided. *Breaks when*
  two choices are genuinely equal, such as "Sign in" and "Create account" for an unknown
  visitor. Then make them equal on purpose and stack them.
- **Secondary actions lose weight, not size.** Keep heights equal so the row aligns.
- **Group by relatedness, order by convention.** Actions on the same object sit together.
  Which end the primary sits at matters less than it sitting there every time.
- **More than three actions**: keep the most used visible and put the rest in a menu.

### Labels

- **Verb plus object, naming the outcome**: "Create team", "Delete 3 files". Avoid OK, Yes
  and Submit when a specific verb exists. A dialog's button repeats the verb in its title.
  *Breaks when* space is extremely tight and context is unmistakable, such as "Save" at
  the end of a form titled "Edit profile".
- **Buttons do, links go.** Match look to behavior. *Breaks for* a navigation step that is
  the screen's primary action ("Continue"), which may look like a button.

### States

- Design default, hover, focus-visible, pressed, disabled and loading.
- **Loading** keeps the button's width, shows the action in progress ("Saving") and blocks
  a second submit.
- **A disabled button hides its reason.** Keep it enabled and explain on press, or show the
  reason beside it. *Breaks when* the reason is plain from the screen, such as an empty
  required field directly above.
- **Hover is not available on touch.** Nothing essential may live only in a hover state.

### Destructive actions

- Quiet in ordinary views, unmistakable in their confirmation, where the button names the
  object and the effect.
- Kept apart from the safe action by space or position, because adjacency invites slips.
- **Prefer undo to confirmation.** Reserve confirmation for what cannot be undone, since
  routine confirmations train people to click through. For the gravest cases, ask the
  person to type the object's name.

## Calls to action

- A call to action earns attention by contrast with its surroundings and the space around
  it, not by size alone.
- State the specific value: "Hire an agent", not "Get started".
- One per viewport. A second of equal weight splits attention and both lose.
- Place it at the point where the person has just understood why they would want it.
  *Breaks when* the page is long: repeat it at the end, since someone who read to the
  bottom is the most interested.
- An empty state's call to action is the single most useful next step, and it must work.

## Inputs and forms

### Anatomy of a field

- **Label above, always visible**, in medium weight. Help text below the label or below the
  field. An error appears with the field and names the fix.
- **A placeholder is not a label.** It vanishes when typing starts and usually fails
  contrast. Use it only for a format hint the label cannot carry.
- **The boundary must be findable**: a visible border meeting 3:1, or a filled field
  clearly distinct from the surface.
- **Width hints at the answer.** A postcode field is short; a description is wide and tall.
- **Same height and radius as buttons.**

### Structure

- **One column.** Multi-column forms get fields skipped and read in the wrong order.
  *Breaks for* short fields that form one thought, such as first and last name or city and
  postcode, which may share a row at wide widths.
- **Every field costs completion.** Ask for less, infer what you can, set honest defaults.
- **Group and order by how the person thinks about the task**, not by the database schema.
- **Mark the minority.** If most fields are required, mark the optional ones.
- **The primary action sits at the end of the form**, aligned with the fields.

### Choosing the control

| Situation | Control |
| --- | --- |
| Two exclusive options, saved with the form | Radios, or a single checkbox when it is yes or no |
| On or off, taking effect immediately | Switch |
| Up to about five exclusive options | Visible radios; a segmented control when they are short and peers |
| More than that | Select |
| Many, or the person knows the name | Searchable combobox |
| Several independent choices | Checkboxes |
| A value in a known range, precision unimportant | Slider, with the value shown |

Visible options beat hidden ones whenever they fit, because the person sees the range of
answers before choosing.

### Segmented controls and filter groups

For two to five short, peer, exclusive options that change the view in place: a filter such
as All, Active, Paused, Former, or a switch between views.

- **The track matches its neighbors.** The container has the same outer height and radius
  as the field or button beside it: 36 px on desktop, 44 px for touch in this product.
- **The selected segment is a tile inset inside the track**, by 2 to 4 px, so it is shorter
  than the track: a 36 px tile in a 44 px track. Its radius is the track's radius minus the
  inset, but no smaller than the product's other tiles of this kind; match them.
- **Each segment's hit area fills the full height of the track**, inset included, and
  segments touch, leaving no dead gap between them. That is how a 36 px tile meets a 44 px
  target without the control growing.
- **Width.** Sized to content when there is room. At narrow widths the track fills the
  available width and the segments share it equally, so its edges align with the field
  above it and the targets are as wide as they can be.
- **Spacing.** The gap between the control and a related field is the small step used
  between related controls, and clearly smaller than the gap to the content they act on.
- **Label and count.** The label carries the weight; a count is regular weight, quieter and
  in tabular figures. Both must pass contrast in the selected and the unselected state.
- **Selected is more than a color**: a filled tile plus weight.
- **One line only.** *Breaks when* the options do not fit on one line at the narrowest
  width and largest text, or there are more than five: use a select instead of wrapping.

### Validation

- **Validate when the person leaves a field, not while they type.** Once a field is in
  error, re-check on each keystroke so the error clears the moment it is fixed. *Breaks
  where* live feedback helps the person succeed: password strength, username availability,
  character count against a limit. Show those as progress, not as errors.
- **Error text says what is wrong and how to fix it**, sits with the field, and keeps what
  was typed. After a failed submit, move focus to the first invalid field. Summarize at the
  top only when the form is long.
- **Use the correct input type, `autocomplete` and `inputmode`.** The right keyboard and
  autofill are the cheapest usability gain available.
- **Never clear a form on failure or disconnect.**

## Navigation components

- **Tabs** switch between a few peer views of the same thing. Short noun labels, the
  current one marked by an underline or shape as well as color. *Breaks when* there are
  more than fit: use a select or a side list rather than a scrolling strip people will not
  find the end of.
- **Sidebar items** pair an icon with a word. An icon alone is a guess for everything
  except a handful of universal symbols.
- **Breadcrumbs** help only in deep hierarchies people enter from the side. In a shallow
  product, one named back link is clearer.
- **Pagination, load more, endless scroll.** Pagination suits finding and returning to a
  place. Endless scroll suits browsing a feed and makes anything below it unreachable.
  "Load more" is the middle ground: the person stays in control and keeps their place.

## Overlays: choosing the lightest one that works

| Need | Use | It breaks when |
| --- | --- | --- |
| Change one value in place | Inline edit | The change needs explanation or has consequences elsewhere. |
| A few related controls or details, without losing the page | Popover, anchored to its trigger | It holds a real form: it closes on an outside click and the work is lost. |
| Pick one of several actions | Dropdown menu. Group with separators; destructive last and separated | More than about eight items, or items needing explanation. |
| A decision that must be made before continuing | Dialog. One task, a title naming it, two buttons | The content is long, or the person needs to look at the page behind it. |
| Create or edit while keeping the list in view | Sheet from the side | Narrow widths (it becomes full width) or multi-step work (give it a page). |
| Complex or multi-step work, or anything worth linking to | Full page | It is a small edit: the navigation costs more than the task. |
| Name an icon-only control | Tooltip | It carries essential or interactive content. Tooltips do not exist on touch. |
| Confirm a completed action | Toast, long enough to read, with undo where possible | It reports an error that needs action. |
| A condition affecting the whole page | Inline callout at the top of the content | There are several: they become wallpaper. |

- **Never stack overlays.** An overlay that opens another has become a flow and wants a
  page.
- **An overlay traps focus, closes on Escape and returns focus to what opened it.**
- **Blocking is expensive.** A dialog takes away the context the person may need in order
  to answer it. Use one only when continuing without an answer would be wrong.

## Feedback and state

- **Spinner, skeleton, progress.** Under about a second show nothing extra beyond the
  pressed state, since a flash of spinner is worse than none. For one to ten seconds use a
  skeleton shaped like the result. Beyond that show real progress and let the person leave
  and return. *Breaks when* the result's shape is unknown: a labeled spinner is more honest
  than a skeleton that guesses.
- **Nothing moves under the pointer.** Reserve the space for content that arrives late
  (images, counts, banners, validation messages), so the layout does not jump as it loads.
  A jump at the moment of a click sends the click to the wrong control. *Breaks when* the
  size cannot be known; then insert below the point of attention, never above it.
- **Optimistic updates** are for operations that rarely fail and are cheap to reverse.
  Never present an unconfirmed result as confirmed.
- **Design the whole set of states**: empty, loading, partial, error, success, stale or
  offline, not permitted, too much data, very long content. The ideal state is the one that
  occurs least in the first week of use.
- **An empty state** says what this place is, why it is empty and offers one real next
  step. A first-use empty state teaches; a "no results" empty state helps the person widen
  the search.
- **Errors** use plain words: what happened, why if known, what to do next. Blocking errors
  stay inline near their cause.
- **Status** uses a small fixed vocabulary. Each status is a word plus a color plus,
  ideally, a shape. One status per row; other facts are text.
- **A value that means "this is not working" must not look like an ordinary value.** An
  agent with no office, a connection that is missing, a check that did not run: if it sits
  in a column styled exactly like its healthy neighbors, nobody sees it. Give it a word
  that says what is wrong and a mark beyond color. Use the product's own meanings for
  color; do not borrow a hue that already means something else. *Breaks when* the empty
  value is normal and harmless, such as an optional field left blank: keep that quiet.
- **Counts and badges** show real numbers and disappear at zero. A badge that is always
  present stops being read.

## Rows, cards and tables

- **Rows for comparable things, cards for self-contained things.** A list of similar items
  is scanned down aligned columns, which rows give and cards destroy. Cards suit items with
  mixed content that are considered one at a time. Do not put a card around something
  merely to decorate it.
- **A row** leads with what identifies the item, puts metadata in a quieter color, ends
  with one status and, if needed, one menu. The whole row may be a link, with independent
  controls remaining separately operable.
- **Tables.** Text aligns to the leading edge, numbers to the trailing edge, units go in
  the header. The first column identifies the row. Rows are about 32 to 40 px compact and
  44 to 52 px comfortable. Use dividers or zebra striping, not both. Keep the header
  visible while the body scrolls. *Breaks at* narrow widths: turn rows into stacked items
  rather than making the page scroll sideways.
- **Actions revealed on hover** must also be reachable by keyboard and on touch.
- **A metric tile** answers one question: the number, what it is, and what to compare it
  with. Use a chart only when the shape of the data is the message.

## Motion

- Motion explains a change: where something came from and where it went.
- About 100 to 200 ms for small state changes. Larger surfaces may take up to about 300 ms
  in general practice; this product caps overlay movement at 200 ms.
- Ease out when entering, ease in when leaving. Exits are faster than entrances.
- Never block input behind an animation, and never move a control on hover or press: the
  target should not shift under the pointer.
- Honor reduced-motion preferences and keep the state words when the animation is gone.
- *Breaks when* motion is the content, as in a chart animating between states to show what
  changed. Even then it must be skippable.

## Words

- Sentence case. Verbs for actions, nouns for places.
- The same word for the same thing everywhere. A synonym reads as a different thing.
- Specific over clever. Real, plural-correct counts.
- No blame in error text. Say what the system could not do, not what the person did wrong.
- Write the words before the layout. If the label cannot be made clear, the design is not
  clear yet.
