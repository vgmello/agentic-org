A-OF1 (v1 64a68813 vs 3c6557a4) is validated: met, with two new S3s.
- OF-2/3/4/5 work. BUG-81 and BUG-82 are fixed, and so are BUG-91 and BUG-92.
- Rowan's rem threshold, `@container (width < 21rem)`, flips correctly 24/24 times at 16, 32 and 40 px roots.
- Every state works, with 0 console errors in 144 runs. Contrast is 0 below the threshold after the change.
- The sheet, Retry and focus return are unchanged.

Report: projects/ac/agents/ac-product-designer-eden/knowledge/research/validation-a-of1.md. Catalog: Offices refreshed, v13.

Proposed rows. All are Avery's unless noted, and N3–N6 fit in one small follow-up.
1. S3, new: the office row shows two focus rings.
   - The stretched span has §17's indigo outline, and the row `<button>` keeps the browser's own outline.
   - Fix: `outline-none` on the button, plus an e2e check that the button's own outline-style is none when focused.
2. S3, new: in the unreachable row, the order is pill › chevron › Retry, so the chevron sits mid-row at every width.
   - Fix: render Retry before the pill+chevron group, so the chevron always trails.
3. S4, new: an orphan "·" starts a line when the meta wraps ("· 7 agents").
   - Fix: the §6 meta line, with facts nowrap and the dot between them.
4. S4, new: below 640, outside compact mode, the pill and chevron are start-aligned, while compact mode end-aligns them.
   - Fix: `max-[639px]:w-full max-[639px]:justify-end`.
5. S4, new: in compact layouts, the page header gutters aren't capped like the list's, so header text and row text don't line up.
   - Fix: the same `min(1rem, 5cqi)` cap on the page wrapper.
6. S4: in compact mode the chevron drops to its own line when the pill doesn't quite fit (2 px short at 320/200%).
   - Product intent: the same row where it fits. Use `@max-[21rem]:gap-x-2`.
   - A chevron on its own line, end-aligned, is acceptable only if even that doesn't fit. Avery's test should assert that.
7. Add to BUG-112: at 320/200%, the /offices header overflows by 8 px (pre-existing; it was 40). The intro and the Sample-data note overflow.
   - Fix: `min-w-0` on the text column and a wrapping note line.
8. Add to BUG-119: at 960 with a 40 px root, long statuses truncate ("Status…") instead of showing the full pill.
   - This is accessible (ellipsis, title and name) and beyond the WCAG bar.
   - Please have the spec's 960/40 test cover Unreachable, Status unknown and Update needed, so it guards the shell fix.
9. Note only: Retry and Close use the faint ring, which is covered by BUG-102. The shell sidebar focus is indigo-500 and the page has no skip link, a pre-existing shell follow-up.
