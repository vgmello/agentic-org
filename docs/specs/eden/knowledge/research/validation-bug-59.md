# Validation · BUG-59 shell focus (v1 504f359 vs 8ef34f8)

Requested by ada (01M7443G0TFMNGQMFRZ5QKQPGB). Jules, jules/bug59-shell-focus @ 6ae081d, Tier B. BUG-69 (unindexed fragment Back) out of scope.

**Verdict: fixed.** All four BUG-59 requirements are met on WebKit and Chromium, and the before build fails where my BUG-52/53 report said. Back with other modals open still leaves the page (SHELL-28). That happens in both builds, so it's not a regression.

## Evidence I opened (WebKit probe JSON, scratchpad bug59/shots)
- **R1, focus after a mouse-opened Menu:** before, Escape, scrim and Close each leave focus on BODY (pass false). After, focus is on BUTTON "Menu" all three ways, with the ring showing only after Escape.
- **R2, rail tooltips by keyboard at 720×450:** before, `missingTooltip: ["Activity"]` (scrollTop 34). After, every item shows its tooltip and the rail scrolls 34 → 86 → 138 → 190, so `missingTooltip: []`.
- **R3, Back with the drawer open:** before, the URL is /agents with the drawer still open. After, the URL stays /offices with the drawer closed and the Offices heading present, and history length stays 3 → 3. Mixing Escape, Back and Close keeps focus on Menu and the length at 4.
- **R4, the optional badge:** `nav.ts` adds `badge?: {text,label}`, and no item has one by default (0 badge spans). With a supplied badge it shows in the full nav, the rail and the drawer, described as "3 need you".
- `registerModalBack` is used only by `MobileNavigation.svelte:28`. `modal-back.ts:50` awaits `traverseTo(source).finished` in a try/catch.

## New findings
1. **S3, a11y/navigation, pre-existing:** Back with any other modal open leaves the page.
   - Repro: on /offices, open an office's details sheet and press Back. Or on /work, open Requests, then New request, and press Back.
   - Expected (SHELL-28): Back closes the modal only and returns focus to its opener.
   - Actual: the page goes to /agents with focus on BODY. Probe `R3-other`: both cases fail, before and after.
   - Fix: reuse `registerModalBack` for every shell modal.
2. **S4, focus, new:** rapid Back and Forward (×6) with the drawer open.
   - The drawer closes and the page stays on /work, which is right, but focus ends on BODY (expected: Menu, SHELL-11).
   - WebKit logged "Unhandled Promise Rejection: AbortError: Navigation aborted" in 2 of 3 runs, with `traverseTo().committed` unhandled.
3. **S4, focus, pre-existing:** Menu then Escape eight times in a row leaves focus on BODY. A normal open and close afterwards restores it.

The delegate's full report follows in my session transcript. Covered: WebKit and Chromium, before and after, at 1440, 960, 720, 600, 390 and 320 px, 200% text, resize across 640, reload with the drawer open.
