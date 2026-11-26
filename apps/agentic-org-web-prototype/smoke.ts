import assert from "node:assert/strict";
import { resolve } from "node:path";
import { chromium, type Page } from "../agentic-org-ui/node_modules/playwright";
import { servePrototype } from "./server";

const ALL_SUITE: Array<[string, string]> = [
  ["inbox", "ocean"],
  ["work", "graphite"],
  ["activity", "cozy"],
  ["projects/office-federation/activity", "report"],
  ["projects/agency-v1/insights", "report-dark"],
  ["chat", "classic"],
];

const SWEEP_ROUTES = [
  "home", "inbox", "projects", "projects/agency-v1", "projects/agency-v1/work",
  "projects/agency-v1/insights", "projects/office-federation/activity", "projects/office-federation/team",
  "work", "chat", "chat/general", "activity", "insights", "calendar", "projects/agency-v1/calendar", "teams/platform/calendar",
  "teams", "teams/platform", "teams/members", "teams/org-chart",
  "members/ada", "members/ada/sessions", "members/ada/journal",
  "members/ada/memory", "members/ada/knowledge", "members/ada/settings",
  "settings", "settings/offices", "settings/agents", "settings/agents/ada",
  "preferences",
];

const ALL_THEMES = ["ocean", "classic", "report", "report-dark", "graphite", "graphite-light", "cozy"];

async function checkInbox(page: Page) {
  const categories = page.getByRole("group", { name: "Inbox queue controls" });
  const category = (name: RegExp) => categories.getByRole("button", { name });
  const searchToggle = page.getByRole("button", { name: "Search inbox" });
  await searchToggle.click();
  const search = page.getByRole("searchbox", { name: "Search inbox" });
  assert.ok(await search.evaluate((input) => input === document.activeElement), "Search icon opens the row and focuses it");
  await search.press("m");
  await search.press("a");
  assert.equal(await search.inputValue(), "ma");
  await search.press("Escape");
  assert.ok(await search.evaluate((input) => input !== document.activeElement));
  assert.equal(await search.inputValue(), "ma", "Escape leaves the draft search intact");
  await search.clear();
  await search.press("Escape");
  assert.equal(await page.getByRole("searchbox", { name: "Search inbox" }).count(), 0, "Escape on an empty search closes the row");
  assert.equal(await searchToggle.getAttribute("aria-expanded"), "false");

  await page.keyboard.press("m");
  await page.keyboard.press("d");
  await page.keyboard.press("Enter");
  assert.equal(await category(/^Decisions/).getAttribute("aria-pressed"), "true");
  await page.keyboard.press("m");
  await page.keyboard.press("a");
  await page.keyboard.press("a");
  await page.keyboard.press("a");
  await page.keyboard.press("Enter");
  assert.equal(await category(/^Approvals/).getAttribute("aria-pressed"), "true", "Repeated initials select the next match without wrapping");

  await page.keyboard.press("m");
  await page.keyboard.press("d");
  await page.keyboard.press("Escape");
  assert.equal(await category(/^Approvals/).getAttribute("aria-pressed"), "true");
  await page.keyboard.press("m");
  await page.keyboard.press("m");
  await page.getByRole("menu", { name: "More inbox categories" }).waitFor();
  await page.keyboard.press("Escape");
}

async function checkWorkBoard(page: Page) {
  const layout = page.getByRole("group", { name: "Work layout" });
  await layout.getByRole("button", { name: "Board" }).press("Enter");
  const board = page.getByRole("region", { name: "Work board" });
  const columns = board.locator(".bcol");
  const cards = board.locator(".bcard");
  const cardIds = () => cards.evaluateAll((items) => items.map((item) => item.getAttribute("data-work-id")).sort());
  assert.equal(await columns.count(), 4, "Stages board has 4 columns");
  const stageIds = await cardIds();
  const attention = board.locator(".bcard[data-attention]").first();
  await attention.locator(".board-status .pill").waitFor();
  assert.match((await attention.locator(".board-status .pill").textContent())!, /^Needs you · /, "Needs-you badge says what is needed");
  assert.equal(await board.locator(".bcard[data-attention]").count(), Number(await page.locator('[data-work-count="needs-me"]').textContent()),
    "Needs you count and amber cards come from the same pending owner actions");
  const multi = board.locator("button.board-need").first();
  const asked = Number((await multi.textContent())!.match(/(\d+) requests/)![1]);
  await multi.click();
  const requests = page.getByRole("menu", { name: /^Requests waiting on you for / }).getByRole("menuitem");
  assert.equal(await requests.count(), asked, "A badge for several requests lists each of them");
  await page.keyboard.press("Escape");
  await checkBoardRequestMenus(page, board);
  await checkAttentionFirst(page, board);
  await checkBoardCountsAndLabels(page, board);
  assert.match((await page.getByRole("button", { name: /^Work display/ }).getAttribute("aria-label"))!, /Stages columns/, "Display names the column mode");
  const trailingGutter = await board.evaluate((el) => {
    el.scrollLeft = el.scrollWidth;
    const lastColumn = el.querySelector(".bcol:last-child")!;
    return el.getBoundingClientRect().left + el.clientWidth - lastColumn.getBoundingClientRect().right;
  });
  assert.ok(trailingGutter >= 15, "The final board column scrolls clear of the right edge");
  await page.getByRole("button", { name: "Work display" }).click();
  await page.getByRole("menuitemradio", { name: "Status", exact: true }).click();
  assert.ok(await columns.count() > 4, "Status columns split the stages");
  assert.deepEqual(await cardIds(), stageIds, "Status columns keep every card");
  await checkStatusLaneJump(page);
  await page.getByRole("button", { name: "Work display" }).click();
  await page.getByRole("menuitemradio", { name: "Stages", exact: true }).click();
  assert.equal(await columns.count(), 4);
  await checkDoneHistory(page, board);
  await checkArchivedOnlyMatches(page, board);
  await checkBoardSubtaskSearch(page, board);
  await checkSubtaskFocusReturn(page, board);
  await checkBoardClearFilters(page, board);
  await checkBoardReturnFromInbox(page, board);
  await checkPhoneLaneJump(page);
  await checkPhoneSearchRevealsLane(page);
  await checkCompactCardLabels(page);
  await layout.getByRole("button", { name: "List" }).press("Enter");
  await checkProjectWorkTab(page);
}

/* Preset counts are the cards the Board shows; Done names its history window; priority and severity read in words */
async function checkBoardCountsAndLabels(page: Page, board: ReturnType<Page["locator"]>) {
  assert.equal(await board.locator(".bcard").count(), Number(await page.locator('[data-work-count="all"]').textContent()),
    "All counts the cards the Board shows, inside the Done history window");
  assert.match((await board.locator("#bcol-done .bcol-window").textContent())!, /Last 7 days/, "Done names its history window");
  const set = board.locator('.bcard[data-work-id="T-A146"] .bpri');
  assert.equal(await set.getAttribute("aria-label"), "High priority");
  assert.equal((await set.innerText()).trim(), "High", "A set priority is a word beside the bars");
  const bug = board.locator('.bcard[data-work-id="T-A88"]');
  assert.match((await bug.locator(".bpri").innerText()).trim(), /^High · inferred$/, "A missing priority says it is inferred, in text");
  assert.match((await bug.locator(".bpri").getAttribute("aria-label"))!, /^No priority set, inferred High$/);
  assert.equal((await bug.locator(".bsev").innerText()).trim(), "Critical severity", "Bug severity is labelled apart from priority");
}

/* Closing a task returns focus to the link that opened it: a subtask link on its parent card, or the parent card */
async function checkSubtaskFocusReturn(page: Page, board: ReturnType<Page["locator"]>) {
  await page.getByRole("button", { name: "Search work" }).first().click();
  await page.keyboard.type("Inventory CSP");
  const hit = board.locator('.bcard .board-subhits a[data-item="T-A147"]');
  await hit.click();
  const panel = page.getByRole("dialog");
  await panel.waitFor({ state: "visible" });
  await page.keyboard.press("Escape");
  await panel.waitFor({ state: "detached" });
  assert.ok(await focusedIs(hit), "Closing a subtask opened from search returns to its subtask link");
  const search = page.getByRole("searchbox", { name: "Search work" });
  await search.fill("");
  await search.press("Escape");
  await search.waitFor({ state: "detached" });
  const parent = board.locator('.bcard[data-work-id="T-A124"] > .work-item-link');
  await parent.click();
  await panel.waitFor({ state: "visible" });
  await panel.locator('.work-detail-links a[href*="item=T-A147"]').click();
  await page.getByRole("heading", { name: "Inventory CSP fixtures" }).waitFor();
  await page.keyboard.press("Escape");
  await panel.waitFor({ state: "detached" });
  assert.ok(await focusedIs(parent), "A subtask with no link of its own on the board returns to its parent card");
}

/* Phones show one lane: a search whose matches are all in another lane brings that lane into view, keeps focus in
   search and says the result */
async function checkPhoneSearchRevealsLane(page: Page) {
  await page.setViewportSize({ width: 375, height: 812 });
  await page.locator("select[data-board-lane]").selectOption("0");
  await page.waitForFunction(() => document.querySelector(".board")!.scrollLeft === 0);
  const search = page.getByRole("searchbox", { name: "Search work" });
  /* Clear filters leaves an empty search row open; the toggle would close it */
  if (!(await search.count())) await page.locator('[data-act="search-toggle"][data-v="work-search"]').click();
  await search.focus();
  await page.keyboard.type("Inventory CSP");
  await page.waitForFunction(() => /Showing/.test(document.getElementById("work-results-status")?.textContent ?? ""));
  const lane = await page.evaluate(() => {
    const board = document.querySelector(".board")!, view = board.getBoundingClientRect();
    const shown = [...board.querySelectorAll(".bcol")].find((col) => Math.abs(col.getBoundingClientRect().left - view.left) < view.width / 2);
    return { name: shown?.querySelector(".bcol-name")?.textContent, cards: shown?.querySelectorAll(".bcard").length };
  });
  assert.ok(lane.cards! > 0, `The lane in view has the match (${lane.name})`);
  assert.equal(await page.locator("#work-results-status").textContent(), `1 match. Showing ${lane.name}.`, "The result and lane are announced");
  assert.equal(await page.locator("select[data-board-lane]").inputValue(), "2", "The picker names the revealed lane");
  assert.ok(await focusedIs(search), "Focus stays in search");
  assert.equal(await page.locator("#work-results").getByRole("button", { name: "Clear filters" }).count(), 1, "One Clear filters for the whole board");
  /* Touch: project, owner and subtask links have 44px hit areas */
  const sizes = await page.locator(".board .bcard :is(.board-project a, .board-owner-label a, .board-subhits a)").evaluateAll((links) =>
    links.filter((a) => a.getClientRects().length).map((a) => { const r = a.getBoundingClientRect(); return [Math.round(r.width), Math.round(r.height)]; }));
  assert.ok(sizes.length >= 3 && sizes.every(([w, h]) => w >= 44 && h >= 44), `Secondary links are 44px targets (${JSON.stringify(sizes)})`);
  await search.fill("");
  await search.press("Escape");
  await search.waitFor({ state: "detached" });
  await page.setViewportSize({ width: 1440, height: 900 });
}

/* Narrow Status lanes keep a short word on each count and the project on its own line */
async function checkCompactCardLabels(page: Page) {
  await page.setViewportSize({ width: 1024, height: 800 });
  await page.getByRole("button", { name: /^Work display/ }).click();
  await page.getByRole("menuitemradio", { name: "Status", exact: true }).click();
  await page.keyboard.press("Escape");
  const card = page.locator('.board .bcard[data-work-id="T-A146"]');
  const tags = await card.locator(".btag").evaluateAll((els) => els.map((el) => (el as HTMLElement).innerText.replace(/\s+/g, " ").trim()));
  /* innerText also carries the sr-only full wording ("2 subtasks") after the visible short one */
  assert.ok(tags.some((tag) => /^2 sub\b/.test(tag)), `Narrow subtask count keeps a word (${tags})`);
  const [ref, project] = await Promise.all([card.locator(".board-ref").boundingBox(), card.locator(".board-project").boundingBox()]);
  assert.ok(project!.y >= ref!.y + ref!.height - 1, "The project has its own line");
  await page.getByRole("button", { name: /^Work display/ }).click();
  await page.getByRole("menuitemradio", { name: "Stages", exact: true }).click();
  await page.keyboard.press("Escape");
  await page.setViewportSize({ width: 1440, height: 900 });
}

/* The project's Work tab says what its count is */
async function checkProjectWorkTab(page: Page) {
  const back = page.url();
  await page.goto(back.replace(/#.*$/, "#/projects/agency-v1/work"));
  assert.match((await page.locator('.tabs a[href$="/work"]').innerText()).replace(/\s+/g, " "), /^Work \d+ open$/, "Work tab count says open");
  await page.goto(back);
  await page.locator("#main h1").first().waitFor();
}

const focusedIs = (locator: ReturnType<Page["locator"]>) => locator.evaluate((el) => el === document.activeElement);

/* Request menus live in the top layer, stay inside the viewport and hand focus back predictably */
async function checkBoardRequestMenus(page: Page, board: ReturnType<Page["locator"]>) {
  assert.match((await page.locator(".queue-count").textContent())!, /^Across all work: \d+ open · \d+ need you$/, "Headline totals say they cover all work");
  const card = board.locator('.bcard[data-work-id="T-A121"]');
  const trigger = card.locator("button.board-need");
  assert.equal(await card.locator(".board-need-due").textContent(), "1 expires in 4 hours", "The card says when its soonest request expires");
  await page.setViewportSize({ width: 1024, height: 520 });
  await board.evaluate((el) => { el.scrollLeft = el.scrollWidth; });
  await trigger.click();
  const menu = page.locator(".board-need-menu:popover-open");
  const box = (await menu.boundingBox())!;
  assert.ok(box.x >= 0 && box.x + box.width <= 1024 && box.y >= 0 && box.y + box.height <= 520, "The menu stays inside the viewport");
  assert.match((await menu.getByRole("menuitem").first().textContent())!, /Expires in 4 hours/, "The expiring request comes first and says when");
  await page.keyboard.press("Escape");
  assert.equal(await menu.count(), 0);
  assert.ok(await focusedIs(trigger), "Escape returns focus to the badge");
  await trigger.press("Enter");
  await page.keyboard.press("Shift+Tab");
  assert.ok(await focusedIs(trigger), "Shift+Tab leaves the menu for its badge");
  await trigger.press("Enter");
  await page.keyboard.press("Tab");
  assert.equal(await menu.count(), 0, "Tab closes the menu");
  assert.ok(await page.evaluate(() => Boolean(document.activeElement?.closest('.bcard[data-work-id="T-A121"] .board-owner-label'))), "Tab moves on to the next control on the card");
  await page.setViewportSize({ width: 1440, height: 900 });
}

/* Text contrast of an element against its composited background (solid colours only) */
function textContrast(el: Element) {
  const parse = (c: string) => {
    const m = c.match(/[\d.]+/g)!.map(Number);
    return c.startsWith("color(srgb") ? [m[0] * 255, m[1] * 255, m[2] * 255, m[3] ?? 1] : [m[0], m[1], m[2], m[3] ?? 1];
  };
  const layers: number[][] = [];
  for (let n: Element | null = el; n; n = n.parentElement) {
    const bg = getComputedStyle(n).backgroundColor;
    if (bg !== "rgba(0, 0, 0, 0)") layers.push(parse(bg));
  }
  const base = layers.findIndex((l) => l[3] >= 1);
  let acc = base < 0 ? [255, 255, 255] : layers[base].slice(0, 3);
  for (let i = (base < 0 ? layers.length : base) - 1; i >= 0; i--) {
    const [r, g, b, a] = layers[i];
    acc = [r * a + acc[0] * (1 - a), g * a + acc[1] * (1 - a), b * a + acc[2] * (1 - a)];
  }
  const channel = (v: number) => (v / 255 <= 0.03928 ? v / 255 / 12.92 : ((v / 255 + 0.055) / 1.055) ** 2.4);
  const lum = (c: number[]) => 0.2126 * channel(c[0]) + 0.7152 * channel(c[1]) + 0.0722 * channel(c[2]);
  const [a, b] = [lum(parse(getComputedStyle(el).color)), lum(acc)];
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

/* Attention first: an expiring owner ask leads its lane, soonest deadline first, ahead of other asks and priority.
   The request badge keeps its fill and text on hover (no brightness filter), so its contrast holds. */
async function checkAttentionFirst(page: Page, board: ReturnType<Page["locator"]>) {
  const first = (lane: string) => board.locator(`#${lane} .bcard`).first().getAttribute("data-work-id");
  assert.equal(await first("bcol-in-progress"), "T-A121", "The ask that expires in 4 hours leads In progress");
  assert.equal(await first("bcol-blocked"), "T-A124", "The ask that expires tomorrow leads Blocked");
  const display = page.getByRole("button", { name: /^Work display/ });
  await display.click();
  const sorts = page.getByRole("group", { name: "Sort by" });
  assert.equal(await sorts.getByRole("menuitemradio", { name: "Attention first" }).getAttribute("aria-checked"), "true", "Attention first is the default");
  assert.equal(await sorts.getByRole("menuitemradio", { name: "Priority", exact: true }).count(), 1, "Priority stays as an explicit sort");
  assert.equal(await page.getByRole("group", { name: "Direction" }).count(), 0, "Attention first has no direction");
  await page.keyboard.press("Escape");
  const badge = board.locator('.bcard[data-work-id="T-A124"] a.board-need');
  await badge.hover();
  assert.equal(await badge.evaluate((el) => getComputedStyle(el).filter), "none", "The request badge hover has no brightness filter");
  const ratio = await badge.evaluate(textContrast);
  assert.ok(ratio >= 4.5, `Request badge text keeps 4.5:1 on hover (${ratio.toFixed(2)})`);
  await page.mouse.move(0, 0);
}

/* A committed chip jump makes its lane current, even when the board cannot scroll it to the start edge, and moves
   focus to that lane's heading */
async function checkStatusLaneJump(page: Page) {
  const chips = page.locator(".board-lane-chips button");
  const last = chips.last();
  await last.focus();
  await page.keyboard.press("Enter");
  await page.waitForFunction(() => document.querySelector(".board-lane-chips button:last-child")?.getAttribute("aria-current") === "true");
  assert.equal(await page.locator('.board-lane-chips button[aria-current="true"]').count(), 1, "Only the jump target is current");
  assert.ok(await focusedIs(page.locator(".board .bcol").last().locator(".bcol-title")), "A keyboard jump moves focus to the lane heading");
  assert.equal(await page.locator(".board .bcol-title").first().getAttribute("tabindex"), "-1");
  assert.equal(await page.locator(".bcol-older, .bcol-history").count(), 0, "Status lanes have no per-lane history menus");
}

/* One board-level Done history control, in Display; a toolbar chip says when it is not the default */
async function checkDoneHistory(page: Page, board: ReturnType<Page["locator"]>) {
  const done = board.locator("#bcol-done");
  const display = page.getByRole("button", { name: /^Work display/ });
  assert.equal(await page.getByText("Show older").count(), 0);
  await display.click();
  const history = page.getByRole("group", { name: "Done history" });
  assert.equal(await history.getByRole("menuitemradio", { name: "Last 7 days" }).getAttribute("aria-checked"), "true");
  assert.equal(await page.getByRole("menuitemcheckbox", { name: "Show archived" }).count(), 0, "The board's Display has no second archive control");
  const before = await done.locator(".bcard").count();
  await history.getByRole("menuitemradio", { name: "Include archived" }).click();
  assert.ok(await done.locator(".bcard").count() > before, "Include archived reaches back past the last week");
  assert.ok(await focusedIs(display), "Focus returns to Display");
  const chip = page.locator(".work-archived-chip");
  assert.match((await chip.textContent())!, /Done: with archived/, "The toolbar says history is widened");
  await chip.click();
  assert.equal(await done.locator(".bcard").count(), before);
  assert.equal(await chip.count(), 0);
  assert.ok(await focusedIs(display), "Resetting history returns focus to Display");
}

/* Filtering to a status whose only matches are archived offers to show them instead of a dead end */
async function checkArchivedOnlyMatches(page: Page, board: ReturnType<Page["locator"]>) {
  await page.getByRole("button", { name: /^Filter work/ }).click();
  await page.getByRole("group", { name: "Status" }).getByRole("menuitemradio", { name: "Fixed", exact: true }).click();
  const filter = page.getByRole("button", { name: "Filter work, Status Fixed" });
  assert.match(await filter.innerText(), /Fixed/, "The Filter trigger shows the active value");
  const reveal = page.locator("#work-results").getByRole("button", { name: /^Show \d+ archived match/ });
  await reveal.waitFor();
  const n = Number((await reveal.textContent())!.match(/\d+/)![0]);
  assert.ok(n > 0);
  await reveal.click();
  assert.equal(await board.locator(".bcard").count(), n, "Show archived matches reveals each of them");
  assert.ok(await page.evaluate(() => Boolean(document.activeElement?.matches(".bcard .work-item-link"))), "Focus lands on the first revealed item");
  await page.locator(".work-archived-chip").click();
  await page.locator("#work-results").getByRole("button", { name: "Clear filters" }).click();
}

/* Board search that matches a subtask names the subtask on its parent card, with a link */
async function checkBoardSubtaskSearch(page: Page, board: ReturnType<Page["locator"]>) {
  await page.getByRole("button", { name: "Search work" }).first().click();
  await page.keyboard.type("Inventory CSP");
  const hit = board.locator(".bcard .board-subhits a");
  await hit.first().waitFor();
  assert.match((await hit.first().textContent())!, /Inventory CSP fixtures/, "The matching subtask is named on its parent card");
  assert.match((await hit.first().getAttribute("href"))!, /item=/, "The subtask line links to the subtask");
  const search = page.getByRole("searchbox", { name: "Search work" });
  await search.fill("");
  await search.press("Escape");
  await search.waitFor({ state: "detached" });
}

async function checkBoardClearFilters(page: Page, board: ReturnType<Page["locator"]>) {
  const cards = await board.locator(".bcard").count();
  await page.getByRole("button", { name: "Search work" }).first().click();
  await page.keyboard.type("zzzz");
  const clear = page.locator("#work-results").getByRole("button", { name: "Clear filters" });
  await clear.click();
  assert.equal(await board.locator(".bcard").count(), cards, "Clear filters brings every card back");
  assert.ok(await focusedIs(page.getByRole("button", { name: "Filter work" })));
}

/* Following a request to Inbox and coming back keeps the board's place and the control you used */
async function checkBoardReturnFromInbox(page: Page, board: ReturnType<Page["locator"]>) {
  await page.setViewportSize({ width: 1440, height: 600 });
  const badge = board.locator('.bcard[data-work-id="T-A124"] a.board-need');
  await board.evaluate((el) => { el.scrollTop = 120; });
  const top = await board.evaluate((el) => el.scrollTop);
  await badge.focus();
  await page.keyboard.press("Enter");
  await page.getByRole("link", { name: "Back to work board" }).waitFor();
  await page.goBack();
  await badge.waitFor();
  assert.equal(await board.evaluate((el) => el.scrollTop), top, "Back restores the board's scroll");
  assert.ok(await focusedIs(badge), "Back returns focus to the badge you followed");
  await badge.press("Enter");
  await page.getByRole("link", { name: "Back to work board" }).click();
  await badge.waitFor();
  assert.ok(await focusedIs(badge), "Back to work board returns to the same badge");
  await page.setViewportSize({ width: 1440, height: 900 });
}

/* Phones: Filter and Display keep their labels; the selected lane takes the full width with no peek of the next;
   the picker browses with arrow keys and jumps (to the top of the lane, focus on its heading) only on commit */
async function checkPhoneLaneJump(page: Page) {
  await page.setViewportSize({ width: 375, height: 812 });
  const display = page.getByRole("button", { name: /^Work display/ });
  assert.match((await display.innerText()).trim(), /^Stages/, "At 420px and below Display keeps the column mode in compact form");
  assert.match((await display.getAttribute("aria-label"))!, /^Work display, Stages columns/);
  assert.match((await page.getByRole("button", { name: /^Filter work/ }).innerText()).trim(), /^Filter/, "Phones keep the Filter label");
  const board = page.locator(".board");
  const [peek, chipsShown] = await board.evaluate((el) => {
    const view = el.getBoundingClientRect(), next = el.querySelectorAll(".bcol")[1].getBoundingClientRect();
    return [view.right - next.left, Boolean(document.querySelector(".board-lane-chips")!.getClientRects().length)];
  });
  assert.ok(peek <= 0, `The next lane does not peek in (${peek}px)`);
  assert.equal(chipsShown, false, "Phones navigate lanes with the picker");
  await board.evaluate((el) => { el.scrollTop = el.scrollHeight; });
  const picker = page.locator("select[data-board-lane]");
  await picker.focus();
  /* Closed-picker browsing as Windows and Linux do it: the value changes during the arrow key's default action
     (macOS opens the native list instead, which a headless run cannot drive) */
  await picker.evaluate((el: HTMLSelectElement) => {
    el.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    el.value = "1";
    el.dispatchEvent(new Event("change", { bubbles: true }));
  });
  assert.equal(await board.evaluate((el) => "jump" in el.dataset), false, "Arrow keys browse the picker without jumping");
  assert.ok(await focusedIs(picker));
  await page.keyboard.press("Enter");
  assert.ok(await focusedIs(page.locator(".board .bcol").nth(1).locator(".bcol-title")), "Enter commits and focuses the lane heading");
  await picker.selectOption("3");
  await page.waitForFunction(() => document.querySelector(".board")!.scrollTop === 0);
  assert.equal(await picker.inputValue(), "3", "The picked lane stays current");
  await page.setViewportSize({ width: 320, height: 812 });
  const rows = await page.locator(".work-toolbar > *").evaluateAll((els) => els.map((el) => el.getBoundingClientRect().top).sort((a, b) => a - b)
    .filter((top, i, tops) => i === 0 || top - tops[i - 1] > 20).length);
  assert.ok(rows <= 2, `The 320px toolbar keeps two rows (${rows})`);
  await page.getByRole("button", { name: /^Work display/ }).click();
  const calendar = page.getByRole("menuitem", { name: "Calendar" });
  assert.ok(await calendar.isVisible(), "At 320px Calendar is in the Display overflow");
  await page.keyboard.press("End");
  assert.ok(await focusedIs(calendar), "Keyboard reaches the overflow Calendar link");
  await page.keyboard.press("Escape");
  await page.setViewportSize({ width: 1440, height: 900 });
}

async function checkWork(page: Page, href: string) {
  const display = page.getByRole("button", { name: "Work display" });
  const group = page.getByRole("group", { name: "Group by", exact: true });
  const subgroup = page.getByRole("group", { name: "Subgroup by", exact: true });
  const groups = page.locator("#work-results .work-group");
  await groups.first().getByRole("heading", { name: /^Blocked\b/ }).waitFor();
  await display.press("Enter");
  assert.equal(await group.getByRole("menuitemradio", { name: "Status" }).getAttribute("aria-checked"), "true");
  await group.getByRole("menuitemradio", { name: "Project" }).press("Enter");
  await groups.first().getByRole("heading", { name: /^Agentic Org/ }).waitFor();
  assert.ok(page.url().endsWith("#/work?group=project"));
  await display.press("Enter");
  assert.equal(await subgroup.getByRole("menuitemradio", { name: "Project" }).count(), 0);
  await subgroup.getByRole("menuitemradio", { name: "Status" }).press("Enter");
  await groups.first().locator(".work-subgroup > .work-list").first().waitFor();
  assert.ok(await groups.count() > 1);
  assert.ok(page.url().endsWith("#/work?group=project&subgroup=status"));

  const task = page.locator("#work-results .work-item-link").first();
  await task.press("Enter");
  const panel = page.getByRole("dialog");
  await panel.waitFor({ state: "visible" });
  assert.ok(await page.locator("#work-results").isVisible(), "Work stays visible behind the panel");
  await page.keyboard.press("Escape");
  await panel.waitFor({ state: "detached" });
  assert.ok(await task.evaluate((link) => link === document.activeElement), "Focus returns to the task");
  assert.ok(page.url().endsWith("#/work?group=project&subgroup=status"), "Task details preserve both groups");
  await display.press("Enter");
  await group.getByRole("menuitemradio", { name: "No grouping" }).press("Enter");
  await groups.first().waitFor({ state: "detached" });
  assert.ok(page.url().endsWith("#/work?group=none"), "Clearing the default is explicit in the URL");
  await page.reload();
  await page.locator("#work-results .work-list").waitFor();
  assert.equal(await groups.count(), 0, "No grouping survives a reload");
  await page.goto(href);
  await groups.first().getByRole("heading", { name: /^Blocked\b/ }).waitFor();
  await checkWorkBoard(page);
}

async function checkActivityPanelAndSim(page: Page, theme: string) {
  const readiness = page.getByRole("progressbar", { name: "Portfolio readiness" });
  const tools = page.getByRole("group", { name: "Activity sidebar tools" });
  const gates = tools.getByRole("button", { name: "Release gates" });
  const simulation = tools.getByRole("button", { name: "Simulation" });
  const panel = page.locator("#activity-panel");
  const activity = page.locator(".activity-main");
  const assertAssemblyBounds = async () => {
    const [left, width, top, bottom] = await activity.evaluate((element) => {
      const content = element.getBoundingClientRect();
      const assembly = element.querySelector(".assembly-view")!.getBoundingClientRect();
      return [assembly.left - content.left, assembly.width - content.width, assembly.top - content.top, assembly.bottom - content.bottom];
    });
    assert.ok(Math.abs(left) <= 1, "Assembly starts at the activity edge");
    assert.ok(Math.abs(width) <= 1, "Assembly fills the available width");
    assert.ok(Math.abs(top) <= 1, "Assembly starts at the top of the activity pane");
    assert.ok(bottom >= -1, "Assembly fills the available height");
  };
  await assertAssemblyBounds();
  await panel.getByRole("heading", { name: "Release gates" }).waitFor();
  assert.equal(await gates.getAttribute("aria-expanded"), "true", "Release gates opens by default");
  assert.equal(await page.locator(".activity-header").getByRole("button", { name: "Release gates" }).count(), 0);
  const bounds = (await panel.boundingBox())!;
  const header = (await page.locator(".activity-header").boundingBox())!;
  const body = (await page.locator(".activity-body").boundingBox())!;
  const toolbar = (await tools.boundingBox())!;
  assert.ok(Math.abs(bounds.y - header.y - header.height) <= 1, "Panel starts below the Activity header");
  assert.ok(Math.abs(bounds.x + bounds.width - toolbar.x) <= 1, "Panel docks beside the right icon rail");
  assert.ok(Math.abs(toolbar.x + toolbar.width - body.x - body.width) <= 1, "Icons stay at the right edge");
  assert.ok(Math.abs(bounds.height - body.height) <= 1, "Panel fits the available height");
  assert.notEqual(await panel.evaluate((element) => getComputedStyle(element).boxShadow), "none");
  if (theme === "cozy") {
    assert.equal(await panel.locator(".activity-rail").evaluate((element) => getComputedStyle(element).borderTopWidth), "0px");
  }
  await simulation.hover();
  await simulation.locator(".icon-tooltip").waitFor();
  await simulation.press("Enter");
  await panel.getByRole("heading", { name: "Simulation", exact: true }).waitFor();
  await panel.getByRole("button", { name: "Reset simulation" }).hover();
  await panel.getByRole("button", { name: "Reset simulation" }).locator(".icon-tooltip").waitFor();
  await panel.getByRole("button", { name: "Reset simulation" }).click();
  await panel.getByRole("button", { name: "8×" }).click();
  await panel.getByRole("button", { name: "Add sample task" }).click();
  await panel.getByText("joined the queue").waitFor();
  assert.equal(await readiness.getAttribute("max"), "20", "The readiness bar tracks simulation updates");
  await panel.getByRole("button", { name: "Play simulation" }).click();
  await gates.click();
  await panel.getByRole("link", { name: "Release sign-off" }).waitFor();
  await page.waitForFunction(() => document.querySelector('[data-assembly-work="T-S1"]:not([data-stage="queue"])'));
  await simulation.press("Enter");
  await panel.getByText("started agent work", { exact: false }).first().waitFor();
  await panel.getByRole("button", { name: "Reset simulation" }).click();
  await page.keyboard.press("Escape");
  await panel.waitFor({ state: "hidden" });
  assert.ok((await activity.boundingBox())!.width > body.width - toolbar.width - 1, "Closing the panel frees its width");
  assert.ok(await simulation.evaluate((button) => button === document.activeElement), "Focus returns to the selected icon");
  await assertAssemblyBounds();
  await gates.click();
  await panel.getByRole("heading", { name: "Release gates" }).waitFor();
}

async function checkActivity(page: Page, theme: string) {
  await page.getByRole("heading", { name: "Portfolio Assembly" }).waitFor();
  const assemblyTab = page.getByRole("tab", { name: "Assembly line" });
  const feedTab = page.getByRole("tab", { name: "Activity feed" });
  const filter = page.getByRole("button", { name: "Filter activity" });
  const filters = page.getByRole("menu", { name: "Filter activity" });
  assert.equal(await assemblyTab.getAttribute("aria-selected"), "true");
  await filter.press("Enter");
  await filters.getByRole("menuitemradio", { name: /Office federation/ }).press("Enter");
  await page.getByRole("heading", { name: "Office federation" }).waitFor();
  assert.equal(await filter.getAttribute("data-active"), "");
  await filter.press("Enter");
  await filters.getByRole("menuitemradio", { name: "All projects" }).press("Enter");
  await assemblyTab.press("ArrowRight");
  assert.equal(await feedTab.getAttribute("aria-selected"), "true");
  assert.ok(await feedTab.evaluate((tab) => tab === document.activeElement), "Focus follows the active view tab");
  await filter.press("Enter");
  await filters.getByRole("menuitemradio", { name: "Decisions" }).press("Enter");
  assert.deepEqual(await page.locator(".activity-timeline .kind").allTextContents(), ["Decision", "Decision"]);
  await filter.press("Enter");
  await filters.getByRole("menuitemradio", { name: "All events" }).press("Enter");
  await feedTab.press("ArrowLeft");
  await page.getByRole("heading", { name: "Portfolio Assembly" }).waitFor();
  const readiness = page.getByRole("progressbar", { name: "Portfolio readiness" });
  assert.equal(await readiness.getAttribute("max"), "19");
  assert.match((await page.locator(".assembly-milestone").first().textContent())!, /\d+\/\d+ tasks · \d+\/\d+ features/);
  await checkActivityPanelAndSim(page, theme);
}

async function checkProjectActivitySim(page: Page) {
  const release = page.getByRole("region", { name: "Release summary" });
  const releaseReadiness = release.getByRole("progressbar", { name: "Release readiness" });
  const assemblyCycle = page.locator(".assembly-metrics [data-assembly-cycle]");
  const panel = page.getByRole("complementary", { name: "Release gates" });
  const tools = page.getByRole("group", { name: "Activity sidebar tools" });
  const simulation = tools.getByRole("button", { name: "Simulation" });
  const gates = tools.getByRole("button", { name: "Release gates" });
  const throughput = panel.getByRole("region", { name: "Throughput" });
  const lineEvents = panel.getByRole("region", { name: "Line events" });

  await simulation.press("Enter");
  const playback = page.getByRole("complementary", { name: "Simulation" });
  await playback.getByRole("button", { name: "Reset simulation" }).click();
  assert.equal(await assemblyCycle.textContent(), "—");
  await playback.getByRole("button", { name: "8×" }).click();
  await playback.getByRole("button", { name: "Add sample task" }).click();
  await playback.getByText("joined the queue").waitFor();
  assert.equal(await releaseReadiness.getAttribute("max"), "5");
  await playback.getByRole("button", { name: "Play simulation" }).click();
  await page.waitForFunction(() => document.querySelector('[data-assembly-work="T-S1"]:not([data-stage="queue"])'));
  assert.doesNotMatch((await playback.locator(".assembly-event-log").textContent())!, /T-A130|T-A84/);
  await gates.click();
  await lineEvents.getByText("T-S1 passed review and merged").waitFor();
  assert.doesNotMatch((await lineEvents.textContent())!, /T-A130|T-A84/);
  assert.match((await throughput.locator(".project-throughput-metrics dd").first().textContent())!, /\d h$/);
  assert.equal(await assemblyCycle.textContent(), await throughput.locator(".project-throughput-metrics dd").first().textContent());
  assert.match((await throughput.getByRole("listitem").last().textContent())!, /[2-9] tasks merged/);
  await simulation.press("Enter");
  await playback.getByRole("button", { name: "Reset simulation" }).click();
  await page.keyboard.press("Escape");
  assert.ok(await simulation.evaluate((button) => button === document.activeElement));
  await gates.click();
}

async function checkProjectActivity(page: Page) {
  const assembly = page.getByRole("tab", { name: "Assembly line" });
  const feed = page.getByRole("tab", { name: "Activity feed" });
  const release = page.getByRole("region", { name: "Release summary" });
  const releaseReadiness = release.getByRole("progressbar", { name: "Release readiness" });
  /* Readiness has one home on project pages: the release strip */
  const readiness = releaseReadiness;
  const panel = page.getByRole("complementary", { name: "Release gates" });
  const gates = page.getByRole("group", { name: "Activity sidebar tools" }).getByRole("button", { name: "Release gates" });
  await readiness.waitFor();
  const projectHeader = page.locator(".project-header");
  assert.equal(await projectHeader.getByRole("progressbar").count(), 0);
  assert.doesNotMatch((await projectHeader.textContent())!, /\bDue\b|\d+ of \d+ done/);
  assert.equal(await assembly.getAttribute("aria-selected"), "true");
  assert.equal(await projectHeader.locator(".project-activity-controls").count(), 0, "View switch lives in its own toolbar row, not the tab row");
  const viewControls = page.locator(".activity-workspace .project-activity-controls");
  const viewSettings = viewControls.getByRole("group", { name: "Activity view settings" });
  await viewControls.getByRole("tab", { name: "Assembly line", selected: true }).waitFor();
  await viewSettings.getByRole("button", { name: "Assembly line display" }).waitFor();
  const orderedRelease = page.locator(".project-header + .activity-workspace > #project-release-summary:first-child");
  await orderedRelease.waitFor();
  assert.equal(await release.count(), 1, "The release summary moved out of the Assembly line table");
  assert.equal(await page.getByRole("progressbar", { name: "Project readiness" }).count(), 0, "No second readiness bar inside Activity");
  assert.equal(await releaseReadiness.getAttribute("max"), "4");
  assert.deepEqual(await release.getByRole("link").allTextContents().then((links) => links.map((label) => label.trim())), ["File access ruling · Pending"]);
  assert.equal(await page.locator(".assembly-milestone").count(), 1);
  assert.equal(await page.locator('[data-assembly-work="T-A121"]').count(), 0);
  assert.equal(await gates.getAttribute("aria-expanded"), "true");
  assert.deepEqual(await panel.getByRole("region", { name: "Gate status" }).getByRole("link").allTextContents(), ["File access ruling"]);
  assert.equal(await panel.getByRole("region", { name: /^Needs a human/ }).getByRole("link", { name: "Custody lock fix" }).getAttribute("href"), "#/inbox/in2");
  assert.equal(await panel.getByRole("region", { name: "Throughput" }).getByRole("listitem").count(), 4);
  await panel.getByRole("region", { name: "Line events" }).getByText("File access ruling requested").waitFor();

  await checkProjectActivitySim(page);

  await page.getByRole("button", { name: "Review T-A118" }).click();
  assert.equal(await feed.getAttribute("aria-selected"), "true");
  await orderedRelease.waitFor();
  assert.ok(await page.locator("#activity-triage-in2").evaluate((element) => element === document.activeElement));
  assert.equal(await page.locator("#activity-triage-in1").count(), 0);
  await viewSettings.getByRole("button", { name: "Filter activity" }).press("Enter");
  const filterMenu = page.getByRole("menu", { name: "Filter activity" });
  assert.equal(await filterMenu.getByRole("group", { name: "Project", exact: true }).count(), 0);
  await filterMenu.getByRole("menuitemradio", { name: "Decisions", exact: true }).press("Enter");
  await feed.press("ArrowLeft");
  await readiness.waitFor();
  await projectHeader.getByRole("link", { name: "Overview", exact: true }).click();
  await page.waitForURL((address) => address.hash === "#/projects/office-federation");
  await page.locator(".project-header + #project-release-summary + .pj-overview").waitFor();
  assert.equal(await releaseReadiness.getAttribute("max"), "4");
  await release.getByRole("link", { name: /File access ruling/ }).waitFor();
  await projectHeader.getByRole("link", { name: "Activity", exact: true }).click();
  await readiness.waitFor();
}

async function checkProjectInsights(page: Page) {
  const insights = page.getByRole("region", { name: "Insights", exact: true });
  const breakdown = insights.getByRole("region", { name: "Spending breakdown" });
  const period = insights.getByRole("combobox", { name: "Period" });
  const view = insights.getByRole("combobox", { name: "View" });
  const groups = insights.getByRole("combobox", { name: "Group spend by" });
  const usageHeading = insights.getByRole("heading", { name: /^Usage records/ });
  const usageRows = insights.getByRole("table", { name: "Usage records", exact: true }).locator("tbody tr");
  const periodCost = insights.locator(".insight-kpis > div").filter({ hasText: "Period cost" }).locator("dd").first();
  const medianCycle = insights.locator(".insight-kpis > div").filter({ hasText: "Median cycle time" }).locator("dd").first();

  assert.equal(await page.getByRole("link", { name: "Insights", exact: true }).getAttribute("aria-current"), "page");
  assert.equal(await period.inputValue(), "month");
  assert.match((await medianCycle.textContent())!, /\d h$/);
  const monthCost = await periodCost.textContent();
  await period.selectOption("last");
  assert.notEqual(await periodCost.textContent(), monthCost);
  assert.equal(await period.evaluate((input) => document.activeElement === input), true);
  await period.selectOption("custom");
  await insights.getByLabel("From", { exact: true }).fill("2026-11-23");
  await insights.getByLabel("To", { exact: true }).fill("2026-11-22");
  await insights.getByRole("button", { name: "Apply dates" }).click();
  assert.ok(await insights.getByLabel("To", { exact: true }).evaluate((input: HTMLInputElement) => input.validity.customError));
  await insights.getByLabel("To", { exact: true }).fill("2026-11-23");
  await insights.getByRole("button", { name: "Apply dates" }).click();
  assert.equal(await usageRows.count(), 1);
  assert.match((await usageRows.first().textContent())!, /T-A88/);
  assert.equal(await medianCycle.textContent(), "—");
  await period.selectOption("month");
  assert.equal(await periodCost.textContent(), monthCost);

  await view.selectOption("table");
  await breakdown.getByRole("table", { name: "Spending breakdown" }).waitFor();
  await insights.getByRole("combobox", { name: "Group investment by" }).selectOption("milestone");
  await insights.getByRole("table", { name: "R&D investment" }).getByRole("rowheader", { name: "Agentic Org v1.0" }).waitFor();
  await groups.selectOption("attribution");
  await breakdown.getByRole("link", { name: "Cancelled work" }).click();
  assert.equal(await usageHeading.textContent(), "Usage records · Cancelled work");
  assert.equal(await usageHeading.evaluate((heading) => heading === document.activeElement), true);
  assert.equal(await usageRows.count(), 1);
  assert.match((await usageRows.first().textContent())!, /T-A999 \(cancelled\)/);
  await insights.getByRole("button", { name: "Show all records" }).click();
  assert.equal(await usageHeading.textContent(), "Usage records");
  assert.ok(await usageRows.count() > 1);
  const download = page.waitForEvent("download");
  await insights.getByRole("link", { name: "Export CSV" }).click();
  const csv = await download;
  assert.equal(csv.suggestedFilename(), "agency-v1-usage.csv");
  const csvText = await Bun.file((await csv.path())!).text();
  assert.match(csvText, /"cache_read_tokens"/);
  assert.match(csvText, /"Cancelled work"/);
  assert.doesNotMatch(csvText, /office-federation/);
  await insights.getByRole("link", { name: /Highest task spend/ }).click();
  await page.locator("#work-panel").waitFor();
  await page.goBack();
  await view.selectOption("chart");
  await page.getByRole("link", { name: "Activity", exact: true }).last().click();
  await page.getByRole("tab", { name: "Assembly line" }).waitFor();
  await page.getByRole("link", { name: "Insights", exact: true }).click();
  await insights.getByRole("heading", { name: "Spend over time" }).waitFor();
}

/* Chat: ask routing rule, Message entry points, conversation search, source links and the narrow thread dialog. */
async function checkChatAsks(page: Page) {
  const card = (mid: string) => page.locator(`.chat-needs:has(#needs-${mid})`);
  await card("m2").getByRole("link", { name: "Review in Inbox" }).waitFor();
  assert.equal(await card("m2").locator(".chat-needs-route").innerText(), "Decide in Inbox · high-impact release");
  assert.equal(await card("m3").getByRole("button", { name: "Allow test access" }).count(), 1, "Low-risk ask answers inline");
  const replyLabels = await page.locator(".chat-needs button").evaluateAll((els) => els.map((e) => e.textContent?.trim() ?? "").filter((t) => /^Reply/.test(t)));
  assert.deepEqual(replyLabels, [], "No card button labelled Reply");
  await card("m3").getByRole("button", { name: "Ask a follow-up" }).click();
  await page.keyboard.type("Is the sandbox wiped?");
  await page.keyboard.press("Control+Enter");
  await page.locator(".chat-needs-note").waitFor();
  assert.equal(await card("m3").getByRole("button", { name: "Allow test access" }).count(), 1, "A follow-up keeps the ask open");
  await page.evaluate(() => { location.hash = "#/chat/dm-eden"; });
  await page.getByRole("button", { name: "Answer…" }).click();
  assert.equal(await page.locator('.chat-needs [data-act="send-reply"]').innerText(), "Send answer");
}
async function checkChatEntryPoints(page: Page) {
  await page.evaluate(() => { location.hash = "#/chat/general"; });
  await page.getByRole("button", { name: "New message" }).click();
  await page.locator("#chat-to").fill("quinn");
  await page.locator("#chat-to-list").getByRole("button", { name: /Quinn/ }).click();
  await page.locator("#ch-title", { hasText: /^Quinn$/ }).waitFor();
  assert.equal(new URL(page.url()).hash, "#/chat/dm-quinn");
  assert.equal(await page.evaluate(() => document.activeElement?.id), "compose");
  await page.locator("#chat-find").fill("rowan");
  assert.match(await page.locator("#chat-nav").innerText(), /Kai and Rowan/);
  await page.evaluate(() => { location.hash = "#/members/noor"; });
  await page.locator('[data-act="go"][data-v="#/chat/dm-noor"]').click();
  await page.locator("#ch-title", { hasText: /^Noor Haddad$/ }).waitFor(); /* their DM, not #general */
  await page.evaluate(() => { location.hash = "#/chat/a2a-kai-rowan"; });
  await page.locator('.msg[data-mid="k2"]').hover();
  await page.locator('.msg[data-mid="k2"] .msg-tool[data-act="chat-dm-msg"]').click();
  await page.locator("#ch-title", { hasText: /^Rowan$/ }).waitFor();
  assert.equal(await page.locator(".chat-compose-src a").getAttribute("href"), "#/chat/a2a-kai-rowan/k2");
  await page.evaluate(() => { location.hash = "#/inbox/in14"; });
  await page.getByRole("link", { name: "Open source message" }).click();
  await page.locator('.msg.is-linked[data-mid="x3"]').waitFor();
}
async function checkChatNarrow(page: Page) {
  await page.setViewportSize({ width: 375, height: 667 });
  await page.evaluate(() => { location.hash = "#/chat/agency-v1"; });
  await page.locator('.msg[data-mid="m2"] .msg-menu .btn').click();
  assert.match(await page.locator("#menu-msg-m2").innerText(), /Sent Today 09:40/);
  await page.getByRole("menuitem", { name: "Reply in thread" }).click();
  await page.locator('.thread[role="dialog"][aria-modal="true"]').waitFor();
  assert.ok(await page.evaluate(() => Boolean(document.querySelector(".chmain")?.closest("[inert]"))), "Background inert under the thread dialog");
  await page.keyboard.press("Escape");
  assert.equal(await page.evaluate(() => document.activeElement?.getAttribute("aria-controls")), "menu-msg-m2", "Focus returns to the visible trigger");
  await page.locator(".chat-list-btn").click();
  await page.locator("#chat-find").fill("tomas");
  assert.match(await page.locator("#chat-nav").innerText(), /Tomas and Sage/);
  const heights = await page.locator("#chat-nav a").evaluateAll((els) => els.map((e) => e.getBoundingClientRect().height));
  assert.ok(heights.every((h) => h >= 44), "Drawer rows are touch-sized");
  await page.setViewportSize({ width: 1440, height: 900 });
}
/* One response record per ask: the same receipt in Chat and Inbox; approval clears the blocker without inferring a start. */
async function checkChatRecords(page: Page) {
  const nav = (h: string) => page.evaluate((x) => { location.hash = x; }, h);
  await page.locator(".chat-needs .reply textarea").fill("One tree, with the kind word on each node.");
  await page.locator('.chat-needs [data-act="send-reply"]').click();
  const receipt = page.locator(".chat-needs .ask-receipt");
  assert.match(await receipt.innerText(), /^Answered by you · \d\d:\d\d[\s\S]*One tree, with the kind word on each node\.$/);
  const line = await receipt.locator("p > span").innerText();
  await nav("#/inbox/in3");
  const inbox = page.locator(".triage-actions .ask-receipt");
  assert.equal(await inbox.locator("p > span").innerText(), line, "Inbox shows the same receipt line");
  assert.match(await inbox.innerText(), /One tree, with the kind word on each node\./, "Inbox shows the written answer");
  await nav("#/chat/agency-v1");
  await page.locator('.chat-needs:has(#needs-m3)').getByRole("button", { name: "Allow test access" }).click();
  assert.match(await page.locator('.chat-needs:has(#needs-m3) .ask-receipt').innerText(), /^Approved by you · /);
  await page.locator('.msg[data-mid="m3"] .tx a.chat-ref').first().click();
  assert.match(await page.locator("#work-panel").innerText(), /Status\s+To do/, "Approval clears the blocker; no inferred In progress");
}
/* Threads carry the ask's card; cross-posting only from channel threads; Back closes overlays; source links win over search. */
async function checkChatOverlays(page: Page) {
  const nav = (h: string) => page.evaluate((x) => { location.hash = x; }, h);
  await nav("#/chat/agency-v1");
  assert.match(await page.locator(".chat-list-btn").getAttribute("aria-label") ?? "", /^Conversations, \d+ requests? waiting on you in other conversations/);
  assert.deepEqual(await page.locator(".chlist .chat-views .chat-row-name").allInnerTexts(), ["Mentions", "Threads"]);
  await page.locator('.msg[data-mid="m2"] .thr').click();
  await page.locator(".thread .chat-needs").getByRole("link", { name: "Review in Inbox" }).waitFor();
  assert.equal(await page.locator(".thread .chat-also").count(), 1, "Channel threads offer cross-posting");
  await page.goBack();
  await page.locator(".thread").waitFor({ state: "detached" });
  assert.equal(await page.evaluate(() => document.activeElement?.classList.contains("thr")), true, "Back closes the thread and restores focus");
  await page.locator(".chat-context .chat-members-btn").click();
  assert.equal(await page.locator("#chat-members .chat-member").count(), 8, "Members come from the channel's membership");
  await page.keyboard.press("Escape");
  await nav("#/chat/dm-ada");
  await page.locator('.msg[data-mid="x3"]').hover();
  await page.locator('.msg[data-mid="x3"] .msg-tool[data-act="thread"]').click();
  assert.equal(await page.locator(".thread .chat-also").count(), 0, "No cross-posting in a DM thread");
  await page.locator(".thread").getByRole("button", { name: "Accept" }).waitFor();
  await nav("#/chat/dm-noor");
  assert.match(await page.locator(".chat-compose-note").innerText(), /can't run right now/);
  await nav("#/chat/platform");
  await page.locator(".chat-search").click();
  await page.keyboard.type("waiting");
  await nav("#/inbox/in7");
  assert.match(await page.locator(".triage-fields").innerText(), /Direct message with Rowan[\s\S]*Related\s+Rowan's message in Kai and Rowan/);
  await nav("#/inbox/in2");
  await page.getByRole("link", { name: "Open source message" }).click();
  await page.locator('.msg.is-linked[data-mid="p2"]').waitFor();
  assert.equal(await page.locator(".chat-search-row").count(), 0, "A source link opens the unfiltered stream");
  assert.equal(await page.evaluate(() => document.activeElement?.getAttribute("data-mid")), "p2", "Focus moves to the linked message");
}
/* Follow-ups sent from a thread stay in that thread; chips name where an ask is settled; drafts and thread
   positions survive closing; a reply sent in a thread is shown with focus kept in the composer. */
async function checkChatThreads(page: Page) {
  const nav = (h: string) => page.evaluate((x) => { location.hash = x; }, h);
  const threadGap = () => page.evaluate(() => { const s = document.querySelector(".thread .msgs")!; return s.scrollHeight - s.clientHeight - s.scrollTop; });
  await page.reload();
  await nav("#/chat/agency-v1");
  await page.locator('.msg[data-mid="m2"] .thr').click();
  const chip = page.locator('.chmain .msg[data-mid="m2"] .chat-needs-chip a');
  assert.equal(await chip.innerText(), "Needs you · Decide in Inbox", "A consequential ask's chip names its route");
  assert.equal(await chip.getAttribute("href"), "#/inbox/in1");
  await page.locator('.thread [data-act="chat-ask-reply"]').click();
  assert.match(await page.locator(".thread .chat-ask-dest").innerText(), /^Posts in this thread and on the Inbox request\./);
  const streamCount = await page.locator(".chmain .msgs > .msg").count();
  await page.keyboard.type("Which fixtures did Tomas run?");
  await page.keyboard.press("Control+Enter");
  await page.locator(".thread .chat-needs-note").waitFor();
  assert.match(await page.locator(".thread .msgs > .msg").last().innerText(), /Which fixtures did Tomas run\?/, "Follow-up posts in the thread");
  assert.equal(await page.locator(".chmain .msgs > .msg").count(), streamCount, "Follow-up does not post in the channel");
  await page.locator("#thread-compose").fill("Approving after the fixtures review.");
  await page.locator("#thread-compose").press("Control+Enter");
  await page.locator(".thread .msgs > .msg", { hasText: "Approving after the fixtures review." }).waitFor();
  assert.ok(await threadGap() < 2, "A sent thread reply is scrolled into view");
  assert.equal(await page.evaluate(() => document.activeElement?.id), "thread-compose", "Focus stays in the thread composer");
  await page.evaluate(() => { document.querySelector(".thread .msgs")!.scrollTop = 40; });
  await page.locator(".thread [data-act=thread-close]").click();
  await page.locator('.msg[data-mid="m2"] .thr').click();
  assert.equal(await page.evaluate(() => document.querySelector(".thread .msgs")!.scrollTop), 40, "Reopening a thread restores its position");
  await page.locator(".thread [data-act=thread-close]").click();
  await page.locator('.msg[data-mid="m3"]').hover();
  await page.locator('.msg[data-mid="m3"] .msg-tool[data-act="thread"]').click();
  await page.locator('.thread [data-act="deny-arm"]').click();
  await page.keyboard.type("Not this week");
  await page.locator(".thread [data-act=thread-close]").click();
  assert.equal(await page.locator("#deny-msg-in5").inputValue(), "Not this week", "A denial note survives closing the thread");
  await nav("#/chat/dm-eden");
  await page.getByRole("button", { name: "Answer…" }).click();
  await page.keyboard.type("One tree");
  await nav("#/chat/general");
  await nav("#/chat/dm-eden");
  await page.getByRole("button", { name: "Answer…" }).click();
  assert.equal(await page.locator("#r-in3").inputValue(), "One tree", "A written answer is kept per ask");
  await nav("#/chat/a2a-kai-rowan");
  const rowan = page.locator('.msg[data-mid="k2"] .chat-needs-chip a');
  assert.equal(await rowan.innerText(), "Needs you · Answer in Rowan's DM");
  assert.equal(await rowan.getAttribute("href"), "#/chat/dm-rowan/r1");
  await nav("#/chat/dm-rowan/r1");
  await page.locator('.msg.is-linked[data-mid="r1"] .chat-needs a[href="#/inbox/in7"]').waitFor();
}
/* Context and entry points: Ada's other requests, a task from #platform, New message to the open DM. */
async function checkChatContext(page: Page) {
  const nav = (h: string) => page.evaluate((x) => { location.hash = x; }, h);
  await nav("#/chat/dm-ada");
  await page.locator(".chat-context .chat-more-btn").click();
  const more = page.locator("#menu-chat-more-ctx");
  await more.waitFor();
  assert.deepEqual(await more.getByRole("menuitem").evaluateAll((els) => els.map((e) => e.getAttribute("href"))), ["#/inbox/in4"], "Ada's other requests link to their Inbox records");
  await page.keyboard.press("Escape");
  await nav("#/chat/platform");
  await page.locator('.msg[data-mid="p2"]').hover();
  await page.locator('.msg[data-mid="p2"] .msg-tool[data-act="chat-task"]').click();
  assert.match(await page.locator("#work-task-dialog").innerText(), /Office federation/, "A task from #platform starts in its project");
  await page.evaluate(() => (document.getElementById("work-task-dialog") as HTMLDialogElement).close()); /* prefilled: Escape asks to discard */
  await page.locator("#work-task-dialog").waitFor({ state: "detached" });
  await nav("#/chat/dm-maya");
  await page.getByRole("button", { name: "New message" }).click();
  await page.keyboard.type("maya");
  await page.keyboard.press("Enter");
  await page.locator("#chat-picker").waitFor({ state: "detached" });
  assert.equal(await page.evaluate(() => document.activeElement?.id), "compose", "Picking the open DM focuses its composer");
}
/* Opening position, sticky day label and the Tradeoff disclosure at 375. */
async function checkChatLayout(page: Page) {
  for (const width of [1440, 1024]) {
    await page.setViewportSize({ width, height: width === 1440 ? 900 : 768 });
    await page.goto(page.url().split("#")[0] + "#/chat/general");
    await page.goto(page.url().split("#")[0] + "#/chat/agency-v1");
    await page.waitForTimeout(400);
    const pos = await page.evaluate(() => {
      const s = document.querySelector(".chmain .msgs[data-ch]")!, sr = s.getBoundingClientRect();
      const last = [...s.querySelectorAll(":scope > .msg")].pop()!.getBoundingClientRect(), day = s.querySelector(".chat-day")!;
      return { full: last.top >= sr.top && last.bottom <= sr.bottom, dayAtTop: Math.round(day.getBoundingClientRect().top - sr.top), stuck: day.classList.contains("is-stuck"), sticky: getComputedStyle(day).position };
    });
    assert.ok(pos.full, `Newest message fully visible at ${width}`);
    assert.equal(pos.sticky, "sticky", "The day label is sticky");
    if (pos.stuck) assert.equal(pos.dayAtTop, 0, "A stuck day label sits at the stream's top edge");
  }
  await page.setViewportSize({ width: 375, height: 667 });
  await page.goto(page.url().split("#")[0] + "#/chat/platform");
  const summary = page.locator(".chat-needs-why summary");
  assert.ok((await summary.evaluate((e) => e.getBoundingClientRect().height)) >= 44, "Tradeoff is a 44px target at 375");
  const chev = () => summary.locator(".chat-needs-chev").evaluate((e) => getComputedStyle(e).transform);
  assert.equal(await chev(), "none");
  await summary.click();
  await page.waitForTimeout(250);
  assert.notEqual(await chev(), "none", "The chevron turns when the Tradeoff opens");
  await page.setViewportSize({ width: 1440, height: 900 });
}
/* Primary actions keep white text at 4.5:1 or more on hover and keyboard focus in every theme. */
async function checkPrimaryContrast(page: Page) {
  const base = page.url().split("?")[0];
  for (const theme of ALL_THEMES) {
    await page.goto(`${base}?theme=${theme}#/chat/dm-eden`);
    const send = page.locator(".chat-compose .btn.primary");
    const ratio = () => send.evaluate((e) => {
      const rgb = (c: string) => c.match(/[\d.]+/g)!.map(Number);
      const lum = ([r, g, b]: number[]) => [r, g, b].map((v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; }).reduce((a, v, i) => a + v * [0.2126, 0.7152, 0.0722][i], 0);
      const cs = getComputedStyle(e);
      if (cs.backgroundImage !== "none" || rgb(cs.backgroundColor)[3] === 0) return 99; /* gradient/glass fills are measured by the theme audit */
      const [a, b] = [lum(rgb(cs.color)), lum(rgb(cs.backgroundColor))];
      return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
    });
    await send.hover();
    await page.waitForTimeout(250);
    assert.ok(await ratio() >= 4.5, `${theme}: primary hover text contrast`);
    await page.mouse.move(0, 0);
    await page.locator("#compose").focus();
    await page.keyboard.press("Tab");
    await page.waitForTimeout(250);
    assert.ok(await ratio() >= 4.5, `${theme}: primary focus text contrast`);
  }
}
async function checkChat(page: Page) {
  await checkChatAsks(page);
  await checkChatRecords(page);
  await checkChatOverlays(page);
  await checkChatEntryPoints(page);
  await checkChatNarrow(page);
  await checkChatThreads(page);
  await checkChatContext(page);
  await checkChatLayout(page);
  await checkPrimaryContrast(page);
}

const ROUTE_CHECKS: Record<string, (page: Page, href: string, theme: string) => Promise<void>> = {
  inbox: (page) => checkInbox(page),
  work: (page, href) => checkWork(page, href),
  activity: (page, _href, theme) => checkActivity(page, theme),
  "projects/office-federation/activity": (page) => checkProjectActivity(page),
  "projects/agency-v1/insights": (page) => checkProjectInsights(page),
  chat: (page) => checkChat(page),
};

async function runRouteCheck(browser: Awaited<ReturnType<typeof chromium.launch>>, baseUrl: URL, pageRoute: string, theme: string) {
  const route = pageRoute.replace(/^#?\/?/, "");
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, reducedMotion: "reduce" });
  try {
    const errors: string[] = [];
    page.on("pageerror", (error) => errors.push(error.message));
    page.setDefaultTimeout(5000);
    const url = new URL(baseUrl);
    url.searchParams.set("theme", theme);
    url.hash = "/" + route;
    const response = await page.goto(url.href);
    assert.ok(response?.ok(), "Prototype preview loads");
    await page.locator("#main h1").first().waitFor({ state: "visible" });
    assert.equal(await page.locator("html").getAttribute("data-theme"), theme);
    await ROUTE_CHECKS[route]?.(page, url.href, theme);
    await page.evaluate(() => document.fonts.ready.then(() => undefined));
    const screenshot = resolve(import.meta.dir, "../../.playwright", `poc-${route.replace(/[^a-z0-9]+/gi, "-")}-${theme}.png`);
    await page.screenshot({ path: screenshot });
    if (route === "inbox") {
      await page.keyboard.press("s");
      await page.keyboard.press("w");
      await page.waitForURL((address) => address.hash === "#/work");
      await page.getByRole("heading", { name: "Work", exact: true }).waitFor();
    }
    assert.deepEqual(errors, [], "No browser errors");
    console.log(`Passed ${route} (${theme})\nScreenshot ${screenshot}`);
  } finally {
    await page.close();
  }
}

async function runFullSweep(browser: Awaited<ReturnType<typeof chromium.launch>>, baseUrl: URL) {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, reducedMotion: "reduce" });
  try {
    const errors: string[] = [];
    const failed: string[] = [];
    page.on("pageerror", (err) => errors.push(err.message));
    page.on("console", (msg) => { if (msg.type() === "error") errors.push("console.error: " + msg.text()); });
    page.on("response", (res) => { if (res.status() >= 400) failed.push(`${res.status()} ${res.url()}`); });
    page.setDefaultTimeout(5000);
    await page.goto(baseUrl.href);
    for (const r of SWEEP_ROUTES) {
      await page.evaluate((h) => { location.hash = "#/" + h; }, r);
      await page.locator("#main h1").first().waitFor({ state: "visible" });
    }
    for (const t of ALL_THEMES) {
      const u = new URL(baseUrl);
      u.searchParams.set("theme", t);
      u.hash = "#/home";
      await page.goto(u.href);
      await page.locator("#main h1").first().waitFor({ state: "visible" });
      assert.equal(await page.locator("html").getAttribute("data-theme"), t);
    }
    assert.deepEqual(failed, [], "No HTTP >=400 responses");
    assert.deepEqual(errors, [], "No page or console errors");
    console.log(`Passed sweep: ${SWEEP_ROUTES.length} routes x ${ALL_THEMES.length} themes (0 errors)`);
  } finally {
    await page.close();
  }
}

const args = process.argv.slice(2);
const runAll = args.includes("--all");
const server = servePrototype(0, false);

try {
  const browser = await chromium.launch();
  try {
    if (runAll) {
      for (const [r, t] of ALL_SUITE) await runRouteCheck(browser, server.url, r, t);
      await runFullSweep(browser, server.url);
    } else {
      const [pageRoute = "work", theme = "graphite"] = args;
      await runRouteCheck(browser, server.url, pageRoute, theme);
    }
  } finally {
    await browser.close();
  }
} finally {
  await server.stop(true);
}
