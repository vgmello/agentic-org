# Validation · Non-Chat pseudo-icons (v1 08db0992 vs 0a87472f)

Requested by ada (01M76QKH). This is Jules's jules/non-chat-icons, merged as 08db0992 (Tier B, Sol 0/0/0). It replaces the × on the remove buttons (project page team chips and the org chart's Teams cards) and the › breadcrumb separator with Heroicons. Checked 12 Nov 2026.

**Verdict: met. No regressions.**
- No × or › is rendered on these surfaces in either engine, at any width, view or text size.
- The x-mark and chevron-right paths are byte-equal to Heroicons v2.2.0. Every SVG has `aria-hidden="true" focusable="false"`.
- Accessible names are unchanged ("Remove Beacon from Web"). The only change in the accessibility tree is the dropped "×" and "›" text.
- Remove still works: 40/40 removes on 08db0992 (80/80 across both builds) in fresh orgs, each with its toast. A removal that would leave a project with no team asks for confirmation first, and the removal persists after a reload. The breadcrumb link works by click and by Enter.
- Targets grew from 16×20 to 24×24, and at 200% text from 32×40 to 48×48. The focus ring is the indigo-400 2 px outline.
- The unit guard passes: `Tests 20 passed (20)`.

**One S4, pre-existing but now visible:** the project page has a "Projects › Beacon" text breadcrumb, which §5 rules out. Detail pages start with a "← Projects" back link. The new chevron (gray-500, 1.3–2.5 px low) only made it more visible.

**Pre-existing, now easier to see:**
- In the org chart's Fit view, the remove buttons scale down to 18–19.6 px. They pass WCAG 2.5.8 only through the spacing exception.
- Removing a team moves focus to `<body>`, so the focus ring and the screen reader's position are lost.
- Phone targets are 24 px, not 44 px (BUG-89).

**Method:**
- Requirements:
  - my mapping to Jules: x-mark for remove, chevron-right for the breadcrumb, aria-hidden, names unchanged, a 24×24 target;
  - design-system §17, §18, §20 and §22.
- Builds: before is 0a87472f and after is 08db0992. Each ran as its own dev server in e2e mode with sample data ("Acme").
- Three parallel checks:
  - Mapping, names and behaviour: WebKit and Chromium at 1280×800 and 390×844, 8 runs plus 80 removes (40 per build).
  - Layout and reflow: 2 engines × 320/640/960/1280 × 100/200% text × 2 builds = 32 combinations, plus a stress pass with long names.
  - Accessibility, keyboard, blitz and catalog: WebKit 1280/100 and 640/200, plus Chromium 1280/100.
- 200% text means the root font size set to 200%. WebKit is the Tauri engine, and its keyboard walks use Option+Tab.

## Requirements (evidence I opened)

| Req | Before (0a87472f) | After (08db0992) | Result |
| --- | --- | --- | --- |
| No pseudo-icon glyphs | `/projects/beacon` text "›××", chart "××××" | No × or › in `main.innerText` or `::before/::after`, in any combination | met |
| Remove icon | "×" text, `px-1`, 16×20 | Heroicons mini `x-mark` in a `size-6` button (24×24), `title` matches the name | met |
| Breadcrumb separator | "›" text in gray-400 | Heroicons mini `chevron-right`, never at the start of a line (stress names up to 5 lines at 320/200) | met (but the breadcrumb itself breaks §5: N1) |
| Hidden from AT, names unchanged | the button read "×" as text | ariaSnapshot names are identical, with the same line counts (26 and 113), and only the × and › text dropped | met |
| Target ≥24×24 | 16×20, and 13.1×16.4 in the fitted chart | 24×24 on the project page and in the chart at Actual size. 19.6 (1280) and 18 (≤960) in the fitted chart | met, except the fitted chart (P1) |
| Contrast ≥3:1 (§18) | n/a | x-mark 4.16 on the chip and 5.70 on the card. Chevron 3.03 | met |
| Focus (§17) | 16×20 ring | Indigo-400 2 px solid, `:focus-visible`. Offset 0 rather than §17's 2 (P4) | met |
| Behaviour | n/a | 40/40 removes per build. Last-team removal opens a confirm dialog with focus on Cancel, and Esc returns focus to the button. Breadcrumb click and Enter work | met |
| No regressions | n/a | No page scrolls sideways in any of the 32 combinations. 0 console or page errors in the blitz (10 remove/re-add cycles, double-click, 5 reloads, 8 back/forward rounds) | met |

Layout, after vs before:
- Chip height went from 24 to 28 px (48 to 56 at 200%).
- Chart project rows went from 20 to 24 px.
- The icon sits 0–0.5 px from the label centre. It is never orphaned or clipped, and stays inside its chip when the label wraps to 5 lines.

## Findings (verified on the screenshots and the source myself)

| Id | Sev | Where | What I saw | Fix |
| --- | --- | --- | --- | --- |
| N1 | S4, pre-existing (§5) | `/projects/{id}` | The page opens with a "Projects › Beacon" text breadcrumb. §5 rules that out: detail pages start with one back link naming where it goes. The new chevron made it more visible: it is `text-gray-500` (3.03:1, §18 "decorative only") beside gray-400 text, and sits 1.3–2.5 px low. `A/shots/after-webkit-1280-breadcrumb.png` vs `before-…` | Replace the breadcrumb with §5's back link "← Projects": `inline-flex items-center gap-x-1 text-sm/6 font-semibold text-gray-400 hover:text-white max-[639px]:min-h-11`, with mini `chevron-left` `size-4` inheriting the colour, inside the page's existing horizontal padding (§5's `px-4 pt-4 sm:px-6 lg:px-8` only if the container doesn't already supply it). "Beacon" stays the page header title. Today that's an `h2` under the top bar's `h1` "Projects"; the heading levels stay as they are. |
| P1 | S3, improved, pre-existing | Org chart → Teams, default Fit view | The chart is scaled with `transform: scale` (0.75 at ≤960, 0.8185 at 1280), so the 24 px button is drawn at 18–19.6 px at 100% text (at 200% the chart uses 0.75 at every width, so it's 36 px). It passes WCAG 2.5.8 only through the spacing exception: the nearest target is 15–16.4 px from its centre, and 12 px is the minimum. Before, it was 12×15 to 13.1×16.4. `B/shots2/webkit-after-960-100-chart-rows-Design.png` | Counter-scale the button: expose the chart's Fit scale (today an inline `transform: scale(…)` in TeamsChart) as a CSS variable, for example `--fit-scale`, and size the button `calc(1.5rem / var(--fit-scale))`, with the icon unchanged, or record the spacing exception in the chart spec. WCAG 2.5.8 asks for 24×24 unless spaced, and §20 asks for 44 below 640. I prefer the counter-scale, because the chart is a pointer surface. |
| P2 | S4, pre-existing | `/projects/{id}` and `/org-chart`, removing a non-last team with Enter | Focus moves to `<body>` (WCAG 2.4.3). The browsers' focus starting point keeps the next Tab in place (on "Remove Beacon from Design" and "Actions for People"), but the focus ring disappears and a screen reader loses its position. The same happens before the change, in both engines. `C/out/keyboard-webkit.json` (`afterRemoveEnter`) | After the remove, focus the next chip's Remove, else the previous one's, else "Add team…" (the same rule as Chat's attachment remove, P2 in `validation-chat-no-emoji.md`). |
| P3 | S3, pre-existing (BUG-89 family) | Below 640 | The remove buttons are 24×24 (18×18 in the fitted chart), but §20 asks for 44 px. They were 16×20 before. `C/catalog/project-phone.png` | `max-[639px]:size-11` with the BUG-89 primitives. |
| P4 | S4, pre-existing | Remove buttons | The focus outline has offset 0, but §17 asks for `outline-offset-2`. It's still visible: 3.46:1 against the chip and 4.74:1 against the card. | Add `focus-visible:outline-offset-2`. |
| P5 | S4, pre-existing | "Add team…" / "Assign team…" button | Its accessible name is always "Assign a team to Beacon". That contains neither visible text as a contiguous string (WCAG 2.5.3, Label in Name). | Make the name start with the visible words: "Add team to Beacon" when the button reads "Add team…", and "Assign team to Beacon" when it reads "Assign team…". |
| P6 | S4, pre-existing | `/projects` intro at 200% text | At 320/200 and 960/200, the intro and "Sample data" paragraphs run past the viewport and are clipped (they end at x=403 on a 320 viewport and x=979 on a 960 one). In the empty state at 320/200, the body copy wraps one word per line. Before and after are identical. `B/shots2/webkit-after-960-200-projects-intro.png` | Let the intro use `min-w-0 max-w-prose` inside the page column, not a fixed width. Recheck the empty state's column width. |
| F1 | follow-up (BUG-70) | Project profile, org chart | Lucide is still used beside the new Heroicons: `Info` in the project Callout, `FolderOpen` on the project page, and `ChevronDown` in TeamsChart. §22 says not to mix sets on one screen. | Migrate them to `information-circle`, `folder-open` and `chevron-down`. |

Not findings:
- Double-clicking Remove removes the team, and the second click lands on "Add team…". This is the same before the change, and it's ordinary layout shift.
- At 320/200, the chart scrolls sideways inside its own frame (on purpose), so a remove button can be in view while its project name is off-screen. This is the same before the change and is covered by the chart's D1 hint (`validation-a-oc1.md`).

## Evidence

Scratch: `/private/tmp/claude-501/-Users-vgmello-projects-agentic-consulting-agents/bee357c5-98c7-42b2-984f-cfd50ca7c83b/scratchpad/pseudo/`.

| Check | Folder | Contents | Final lines |
| --- | --- | --- | --- |
| Mapping, names, behaviour | `A/` | `results.json`, `behaviour.json`, `upstream/*.svg`, 76 shots | `audit complete: 8 runs`; behaviour.json has 80 removes (40 per build), all ok, 0 errors; vitest `Tests 20 passed (20)` |
| Layout and reflow | `B/` | `matrix.json`, `pass2.json`, `summary*.txt`, shots and shots2 | 32 combinations, 0 script errors; no page scrolls sideways |
| Accessibility, keyboard, blitz, catalog | `C/` | `out/a11y-*.yaml`, `keyboard-*.json`, `contrast.json`, `blitz-*.json`, 4 catalog shots | 0 console or page errors in the blitz |

- The 48 `scrollIntoViewIfNeeded: Timeout` lines in `A/summary.txt` come from the first audit's remove loop. Removals carried over within one browser context, so the buttons were already gone. They are a harness artefact; `behaviour.json` (a fresh org per removal) replaces them.
- Each worktree got its own `bun install --frozen-lockfile`.
- All servers are stopped and all worktrees removed. Nothing in the product repo was edited.
