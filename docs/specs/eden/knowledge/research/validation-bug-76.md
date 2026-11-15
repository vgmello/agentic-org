# Validation · BUG-76 StatusPill single-line label (v1 2efce5d5 vs 905dfb2f)

Requested by ada (01M760GMFF817R7S988P8CDVE6). Jules, jules/bug76-single-line-pill @ e7531f9a, Tier B, Sol READY.

**Verdict: fixed.** The pill no longer breaks mid-word or letter by letter:
- on the /agents roster at 320, 390, 640 and 960 px;
- at 100% and 200% text;
- in WebKit and in Chromium.

A too-long single word truncates with an ellipsis and keeps its full text. There is one new edge-case problem, on Offices at 320 px with 200% text: a long status is clipped with no ellipsis. It's filed as a follow-up below. I also amended the spec for multi-word pills that don't fit even after the row wraps.

Method: 200% text means the root font-size set to 200%, the way the e2e tests do it. Sample data has one agent status ("Asleep · wakes when called") and one office status ("Online"). Longer labels were tested two ways:
- the repo's look-primitives fixture and a controlled StatusPill;
- real status words written into the live Offices pill.

## Requirements (evidence I opened)

| Req | Before (905dfb2f) | After (2efce5d5) |
| --- | --- | --- |
| R1: never breaks mid-word | All 8 roster pills 19–20 lines (one letter per line) at every width and both text sizes, both engines. "Online" in a narrow box: 3 lines "On/lin/e" | 1 line in all 16 cases per engine (WebKit `probes/{before,after}-*.json`, Chromium `summary.json`, screenshots `before/after-960-100-cells.png`) |
| R2: a too-long single word truncates, with full name and title | 70-character word: 4–7 lines, no `title` | 1 line with an ellipsis (e.g. 465/122 px), `title` = full text, full text in the accessibility tree. On the roster the table cell grows and scrolls instead, so the ellipsis doesn't appear there |
| R3: multi-word pills wrap the row, not the pill | Pill broke inside | "On hold" and "Ready now" stay on 1 line and the row wraps. When the pill is wider than its whole row even after wrapping (320/200%: "Asleep · wakes when called", 317/212 px; "Status unknown", 185/160 px), it truncates with an ellipsis and keeps its full title and name. The spec didn't cover this case; it's now decided (below) |
| R4: decorative icons stay hidden; a role keeps its name | Met | Met: every icon `aria-hidden`; a role=button pill keeps "Filter: Paused" |
| R5: 100% and 200% text | Fails at both | Met at both |
| R6: no regressions elsewhere | — | StatusPill renders only on Agents (roster) and Offices (row and sheet). No pill is truncated where there's room, and none gains an ellipsis at 1280, 960 or 390 px. The exceptions are F1 and F3 |

Tests: look-primitives and offices specs with `--repeat-each=3` gave "405 passed (15.1m)": Chromium 237, WebKit 168, 0 flaky.

## Findings

1. **S3, layout, new (after only).** Offices row at 320 px with 200% text. A long status runs past the row and the list's `overflow-hidden` clips it with no ellipsis: "Status unknown" loses 56.5 px and "Unreachable" 17.9 px. Before, it broke mid-word ("unknow/n").
   - Expected: an ellipsis with the full name and `title` (design-system §7 and the product decisions).
   - Fix: let the pill shrink inside its container (`min-w-0 max-w-full` on the pill, `truncate` on the label).
   - Evidence: sweep `probes/offices-row-clip-320@200.json` and `shots/{before,after}-offices-row-320@200-Status_unknown.png`.
2. **S3, layout, pre-existing (better after).** Offices row at 200% text (390 and 960 px). For labels longer than "Online", the icon wraps above the word and the pill grows to 84 px tall, because the row is too narrow for one line ("Update needed" needs 248 px; the row is 256 px wide but only 192 px inside its padding. Corrected 11 Nov: I first compared with the outer width). Before, the pill was 116–148 px tall and the word broke. Expected: icon and label on one line. Evidence: sweep `probes/offices-row-pill-wrap.json`.
3. **S4, layout, new.** /agents roster at 960 px and 100% text: the table is now 756 px wide in a 608 px area, so it scrolls sideways. Before, it fitted only by stacking the pill's letters. Neighbouring cells squeeze, and Teams words already broke mid-word ("Peopl/e", "Desig/n") through that column's `overflow-wrap:anywhere`. The S4 row layout in BUG-70 (§6 DividedRow) replaces this table, so I'm not filing a separate bug. Evidence: WebKit `shots/after-960-100-cells.png`.

## Spec amendment (design-system.md, product decisions)

A pill that is still wider than its row after the row has wrapped truncates with an ellipsis, whatever its word count. It keeps its full text as the accessible name and `title`. A pill is never clipped without an ellipsis and never breaks mid-word.

## Catalog

Updated at 2efce5d:
- **Agents roster:** validated.
- **Offices:** implemented, with a note on F1.
