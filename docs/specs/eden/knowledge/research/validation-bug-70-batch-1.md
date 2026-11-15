# Validation · BUG-70 look and Offices batch 1 (v1 c1da4893)

Requested by ada (batch: cc858f0f, 536f36be, ea5ca768, 72fd3b55, 46fa5b53, 423c3f04, 4562145e) plus abcdb2be (Offices BUG-25/26/64). Before builds: c02fbfd7 (look merges) and 3a121edd (Offices). WebKit and Chromium at 1440, 960, 720 and 390, against the prototype and design-system.md v1.0.

## Per merge
| Merge | Result | Evidence |
| --- | --- | --- |
| cc858f0f primitives | partly met. The pill matches the prototype StatusBadge at 1440 (24 px, 8/4 padding, inset ring), but breaks at ≤960 (F1) | after/before-webkit-1440-_agents.png |
| 536f36be top bar | met. gray-800, white/5 border, gap 24, padding 16/24/32, 56/64 px (before: gap 12, padding 32) | probe-metrics.json |
| ea5ca768 Project detail | met. Flat header, dividers, neutral callout | after-webkit-1440-_projects_beacon.png |
| 72fd3b55 BUG-74 | met. Focus lands on H1#shell-page-title at 1440, 960 and 720 in both engines (before: BODY) | probe2-after/before.json |
| 46fa5b53 footer | met. Contrast 5.94:1, wraps | after-webkit-390-drawer.png |
| 423c3f04 Offices pill | met | after/before-webkit-1440-_offices.png |
| 4562145e flat roster | partly met. Enclosure gone, rows 16 px, but F1 | after-webkit-960-_agents.png |
| abcdb2be Offices 25/26/64 | partly met. Countdown floors to 10 s steps (45→40, 39.5→30, 9.5→"shortly"); refusal note clears; footer copy right; host naming partly (F2); spoken countdown mismatch (F6) | probe3-offices.json |

## Findings (checked by me)
- **F1 · S2 · new regression · Agents roster.** At 960, 720 and 390 in both engines, the status pill "Asleep · wakes when called" wraps one letter per line (pill 46 × 312 px). Cause: the inner span has `min-w-0 [overflow-wrap:anywhere]` in an auto-width table cell. Expected: the pill never breaks mid-word (design-system §7: whitespace-nowrap, the row wraps instead). The developer's own preflight screenshot shows it too; the tests check padding and radius, not size. I opened after-webkit-960-_agents.png and confirmed it.
- **F2 · S3 · Offices.** The row's accessible name says "the office's machine", but the visible text still says "This Mac (you're here)" (or "studio-mac.local (you're here)"). That breaks label-in-name (WCAG 2.5.3). The details sheet contradicts itself: "Where: This Mac (you're here)" above "…on the office's machine…". probe3: rowText vs rowLabel.
- **F6 · S3 · Offices countdown.** The spoken label doesn't match the visible text. Visible "Restarting in 40 s" is spoken as "restarting shortly", and visible "in 60 s" is spoken as "restarting at 16:28". The label must say what the screen says. probe3: cd45, cd61.
- **F3 · S4.** Between 640 and 1023 px the top-bar heading starts at x=88 and page content at x=96 (bar padding 24, page 32).
- **F4 · S4, pre-existing:**
  - On Agents, the selected filter count is 3.5:1.
  - The Projects footnote is gray-500 (3.03:1).
  - Projects at 390 px and 200% text clips 13 px.
  - There's no skip link.
  - Team names break mid-word.
- **F5 · S4.** Long restarts show "in 120 s" rather than minutes.

Not a finding: in WebKit, Tab after the focused heading reaches BODY only because WebKit's default Tab skips buttons; Chromium moves to the next control.

Remaining look gaps on these screens are covered by knowledge/research/bug-70-per-screen-gaps.md: no search or primary action in the top bar, the Agents table instead of divided rows, double-indented lists, Project detail sub-nav, Offices cards and the amber callout, and the nav.
