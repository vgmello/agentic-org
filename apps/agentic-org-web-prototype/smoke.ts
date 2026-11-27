import assert from "node:assert/strict";
import { resolve } from "node:path";
import { chromium, type Page } from "../agentic-org-ui/node_modules/playwright";
import { servePrototype } from "./server";
import { checkPersonPanel } from "./smoke-person";

const ALL_SUITE: Array<[string, string]> = [
  ["inbox", "ocean"],
  ["work", "graphite"],
  ["activity", "cozy"],
  ["projects/office-federation/activity", "report"],
  ["projects/agency-v1/insights", "report-dark"],
  ["chat", "classic"],
  ["projects/agency-v1", "graphite-light"],
  ["home", "report-dark"],
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
  await attention.locator(".board-card-context .board-facts .board-need").waitFor();
  assert.match((await attention.locator(".board-card-context .board-facts .board-need").getAttribute("aria-label"))!, /^Needs you: /, "The Needs you bell's name says what is needed");
  assert.equal(await board.locator(".bcard[data-attention]").count(), Number(await page.locator('[data-work-count="needs-me"]').textContent()),
    "Needs you count and amber bells come from the same pending owner actions");
  assert.equal(await board.locator(".bcard .board-need").count(), await board.locator(".bcard[data-attention]").count(), "Every card that needs you has its bell");
  const multi = board.locator("button.board-need").first();
  const asked = Number((await multi.getAttribute("aria-label"))!.match(/(\d+) requests/)![1]);
  assert.equal((await multi.locator(".board-need-n").textContent())!.trim(), String(asked), "The bell shows the request count");
  await multi.click();
  const requests = page.getByRole("menu", { name: /^Requests waiting on you for / }).getByRole("menuitem");
  assert.equal(await requests.count(), asked, "A badge for several requests lists each of them");
  await page.keyboard.press("Escape");
  for (const width of [1440, 1024, 420, 320]) await checkToolbarTabOrder(page, width);
  await checkBoardRequestMenus(page, board);
  await checkAttentionFirst(page, board);
  await checkBoardPrioritySort(page, board);
  await checkBoardCountsAndLabels(page, board);
  await checkPendingApprovalAndHold(page, board);
  await checkListBoardPriority(page, board);
  await checkCardLayout(page, board);
  await checkAssigneeRow(page, board);
  await checkCardTooltip(page, board);
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
  await checkMixedArchivedMatches(page, board);
  await checkBoardSubtaskSearch(page, board);
  await checkSubtaskFocusReturn(page, board);
  await checkBoardClearFilters(page, board);
  await checkBoardReturnFromInbox(page, board);
  await checkPhoneLaneJump(page);
  await checkPhoneSearchRevealsLane(page);
  await checkCompactCardLabels(page);
  await checkReturnAfterResolve(page, board);
  await checkDeniedAccess(page, board);
  await checkVerifyApproved(page, board);
  await layout.getByRole("button", { name: "List" }).press("Enter");
  await checkProjectWorkTab(page);
}

/* Preset counts are the cards the Board shows; Done names its history window; priority and severity read in words */
async function checkBoardCountsAndLabels(page: Page, board: ReturnType<Page["locator"]>) {
  assert.equal(await board.locator(".bcard").count(), Number(await page.locator('[data-work-count="all"]').textContent()),
    "All counts the cards the Board shows, inside the Done history window");
  const shownHead = await board.locator("#bcol-done .bcol-h").evaluate((el) => { const c = el.cloneNode(true) as Element; c.querySelectorAll(".sr-only, .menu").forEach((n) => n.remove()); return c.textContent!; });
  assert.doesNotMatch(shownHead, /Last 7 days/, "Done's history window is not inline text");
  const more = board.locator("#bcol-done").getByRole("button", { name: "Done history: Last 7 days. Change" });
  assert.equal(await more.count(), 1, "The history window is in the ⋯ button's name");
  assert.equal(await more.getAttribute("data-icon-tip"), "Done history: Last 7 days", "…and its tooltip");
  const set = board.locator('.bcard[data-work-id="T-A146"]');
  assert.equal(await set.locator(".bpri").getAttribute("aria-label"), "High priority");
  assert.equal((await set.locator(".bpill").innerText()).trim(), "High", "A set priority is a word beside the bars");
  const bug = board.locator('.bcard[data-work-id="T-A88"]');
  assert.equal((await bug.locator(".bpill").innerText()).trim(), "High*", "An inferred priority reads High*");
  assert.equal(await bug.locator(".bpri").getAttribute("aria-label"), "Inferred priority: High (none set)");
  assert.equal(await bug.locator(".bpill").getAttribute("aria-label"), "Bug · Inferred priority: High (none set)", "The pill names the bug and the inferred priority");
  assert.equal(await bug.locator(".bpill .ic").count(), 1, "A bug has a small bug icon in its pill");
  const sev = bug.locator(".board-facts .bsev");
  assert.equal(await sev.getAttribute("aria-label"), "Critical severity", "Bug severity is a card icon, labelled apart from priority");
  assert.equal(await sev.getAttribute("data-tone"), "bad", "Critical severity is red");
}

/* The toolbar's Tab order is its visual reading order (rows top to bottom, each left to right) at every width: no
   control is moved visually away from its place in the DOM */
async function checkToolbarTabOrder(page: Page, width: number) {
  await page.setViewportSize({ width, height: 900 });
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  await page.waitForTimeout(150);
  const visual = await page.evaluate(() => {
    const els = [...document.querySelectorAll<HTMLElement>(".work-toolbar :is(a[href], button, select, input, [tabindex])")]
      .filter((el) => el.tabIndex >= 0 && el.getClientRects().length && !el.closest("[hidden], .menu"));
    const rows: { cy: number; items: { id: string; left: number }[] }[] = [];
    els.forEach((el, i) => {
      el.dataset.tabProbe = String(i);
      const r = el.getBoundingClientRect(), cy = r.top + r.height / 2;
      const row = rows.find((row) => Math.abs(row.cy - cy) < 16);
      if (row) row.items.push({ id: String(i), left: r.left }); else rows.push({ cy, items: [{ id: String(i), left: r.left }] });
    });
    return rows.sort((a, b) => a.cy - b.cy).flatMap((row) => row.items.sort((a, b) => a.left - b.left).map((item) => item.id));
  });
  assert.ok(visual.length >= 6, `The toolbar has its controls at ${width}px (${visual.length})`);
  await page.locator('.work-toolbar [data-tab-probe="0"]').focus();
  const tabbed = [await page.evaluate(() => (document.activeElement as HTMLElement).dataset.tabProbe)];
  for (let i = 1; i < visual.length; i++) {
    await page.keyboard.press("Tab");
    tabbed.push(await page.evaluate(() => (document.activeElement as HTMLElement).dataset.tabProbe));
  }
  assert.deepEqual(tabbed, visual, `Tab order follows the toolbar's visual order at ${width}px`);
  await page.setViewportSize({ width: 1440, height: 900 });
}

/* A pending owner approval on Fixed work stays in view: it counts in Needs you, ignores the Done history window and
   archiving, and Needs you shows a Done lane holding only such cards. A known execution hold is its own cue. */
async function checkPendingApprovalAndHold(page: Page, board: ReturnType<Page["locator"]>) {
  const fixed = board.locator('#bcol-done .bcard[data-work-id="T-A81"]');
  assert.equal(await fixed.count(), 1, "T-A81 (Fixed 21 days ago, fix waiting for approval) stays in Done");
  assert.equal(await fixed.getAttribute("data-attention"), "", "Its pending approval marks it as needing you");
  assert.match((await fixed.locator(".board-need").getAttribute("aria-label"))!, /^Needs you: Fix to verify/);
  assert.equal(await fixed.locator(".bdate").getAttribute("aria-label"), "Waiting on your verification", "Pending verification is not dated as closed history");
  assert.equal(await fixed.locator(".bstatus").getAttribute("aria-label"), "Status: Fixed", "The exact status is a footer icon");
  assert.doesNotMatch(await fixed.innerText(), /Closed \d+d ago|Fixed/, "No closing date or status chip on the card face");
  const hold = board.locator('.bcard[data-work-id="T-A135"]');
  assert.match((await hold.locator(".board-hold").innerText()).replace(/\s+/g, " ").trim(), /^Waiting for office · Build box is offline$/, "A known execution hold says the office is waiting");
  assert.equal(await hold.getAttribute("data-attention"), null, "An execution hold is not Needs you");
  assert.equal(await hold.locator(".board-hold a").getAttribute("href"), "#/inbox/in9", "The hold opens its Inbox explanation");
  await page.locator('[data-act="work-preset"][data-v="needs-me"]').click();
  await page.locator('[data-act="work-preset"][data-v="needs-me"][aria-pressed="true"]').waitFor();
  await board.locator(".bcard").first().waitFor();
  const lanes = await board.locator(".bcol").evaluateAll((cols) => cols.map((col) => [col.id, [...col.querySelectorAll(".bcard")].map((c) => c.getAttribute("data-work-id"))]));
  const done = lanes.find(([id]) => id === "bcol-done");
  assert.deepEqual(done?.[1], ["T-A81"], `Needs you shows a Done lane holding only the pending approval (${JSON.stringify(lanes)})`);
  assert.equal(await board.locator(".bcard").count(), Number(await page.locator('[data-work-count="needs-me"]').textContent()), "Needs you counts every card it shows");
  await page.locator('[data-act="work-preset"][data-v="all"]').click();
  await page.locator('[data-act="work-preset"][data-v="all"][aria-pressed="true"]').waitFor();
  await board.locator('.bcard[data-work-id="T-A112"]').waitFor();
}

/* List and Board read one priority (set, else inferred) and label bug severity apart from it, and sort the same way */
async function checkListBoardPriority(page: Page, board: ReturnType<Page["locator"]>) {
  const boardLabels = Object.fromEntries(await board.locator(".bcard").evaluateAll((cards) => cards.map((c) => [c.getAttribute("data-work-id"), c.querySelector(".bpri")!.getAttribute("aria-label")])));
  const layout = page.getByRole("group", { name: "Work layout" });
  await layout.getByRole("button", { name: "List" }).click();
  const rows = page.locator("#work-results .work-list > .work-item > .work-row");
  await rows.first().waitFor();
  const listLabels = Object.fromEntries(await rows.evaluateAll((els) => els.map((r) => [r.getAttribute("data-work-id"), r.querySelector(".work-priority-mark")!.getAttribute("aria-label")])));
  for (const [id, label] of Object.entries(boardLabels)) assert.equal(listLabels[id], label, `${id} has the same priority in List and Board`);
  const bug = page.locator('#work-results .work-row[data-work-id="T-A84"]');
  assert.equal(await bug.locator(".work-priority-mark").getAttribute("data-level"), "3", "T-A84's bars show its inferred High, not its Minor severity");
  assert.match(await bug.locator(".work-severity").innerText(), /^Minor severity$/, "The List labels severity apart from priority");
  await page.getByRole("button", { name: "Work display" }).click();
  await page.getByRole("group", { name: "Group by", exact: true }).getByRole("menuitemradio", { name: "No grouping" }).click();
  await page.getByRole("button", { name: "Work display" }).click();
  await page.getByRole("group", { name: "Sort by" }).getByRole("menuitemradio", { name: "Priority", exact: true }).click();
  await page.keyboard.press("Escape").catch(() => {});
  const ranks = await rows.evaluateAll((els) => els.map((r) => Number(r.querySelector(".work-priority-mark")!.getAttribute("data-level"))));
  assert.ok(ranks.every((rank, i) => i === 0 || rank <= ranks[i - 1]), `The List's Priority sort follows the displayed priority (${ranks})`);
  await page.goto(page.url().replace(/#.*$/, "#/work"));
  await page.locator("#main h1").first().waitFor();
  await page.getByRole("button", { name: "Work display" }).click();
  await page.getByRole("group", { name: "Sort by" }).getByRole("menuitemradio", { name: "Original order" }).click();
  await page.keyboard.press("Escape").catch(() => {});
  await page.getByRole("group", { name: "Work layout" }).getByRole("button", { name: "Board" }).click();
  await board.locator(".bcard").first().waitFor();
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
  /* Card icons, the bell, a focusable pill, the kind icon and the team dot reach 44px through their hit area (::after) */
  const hits = await page.locator(".board .bcard :is(.bicon, .board-need, .bpill[tabindex], .board-kind, .board-team-tag)").evaluateAll((els) =>
    els.filter((el) => el.getClientRects().length).map((el) => { const a = getComputedStyle(el, "::after"), r = el.getBoundingClientRect();
      return [Math.round(r.width - parseFloat(a.left) - parseFloat(a.right)), Math.round(r.height - parseFloat(a.top) - parseFloat(a.bottom))]; }));
  assert.ok(hits.length >= 3 && hits.every(([w, h]) => w >= 34 && h >= 44), `Card icons have 44px-high touch targets (${JSON.stringify(hits)})`);
  await search.fill("");
  await search.press("Escape");
  await search.waitFor({ state: "detached" });
  await page.setViewportSize({ width: 1440, height: 900 });
}

/* 1024px: lanes keep about 260px and overflow into the column jump. Compact cards keep the team dot and keep the
   progress row on one line; the order stays progress, then the assignee row, its icons on that line or wrapped under it. */
async function checkCompactCardLabels(page: Page) {
  await page.setViewportSize({ width: 1024, height: 800 });
  await page.waitForFunction(() => document.querySelector(".board-lanes")?.hasAttribute("data-overflow"));
  const widths = await page.locator(".board .bcol").evaluateAll((cols) => cols.map((c) => c.getBoundingClientRect().width));
  assert.ok(widths.every((w) => w >= 255), `Lanes keep a readable width at 1024 (${widths.map(Math.round)})`);
  const card = page.locator('.board .bcard[data-work-id="T-A146"]');
  assert.ok(await page.locator('.board .bcard[data-work-id="T-A88"] .bsev').isVisible(), "Compact cards keep severity");
  const teams = await page.locator(".board .bcard .board-team").evaluateAll((els) => els.filter((el) => !el.getClientRects().length).length);
  assert.equal(teams, 0, "No card hides its team dot at 1024");
  const progressHeight = await card.locator(".board-progress").evaluate((el) => el.getBoundingClientRect().height);
  assert.ok(progressHeight <= 24, `The progress row stays on one line (${progressHeight}px)`);
  const [progress, owner, icons] = await card.evaluate((el) => [".board-progress", ".board-assignee", ".board-facts"].map((sel) => el.querySelector(sel)!.getBoundingClientRect().top));
  assert.ok(progress < owner && owner - 4 <= icons, "Progress, then the assignee row with its icons");
  await card.locator(":scope > .work-item-link").click();
  await page.locator("#work-detail-subtasks").waitFor();
  await page.keyboard.press("Escape");
  await page.getByRole("dialog").waitFor({ state: "detached" });
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

/* Priority sort follows the priority each card shows (set, else inferred), never the bug's raw severity */
async function checkBoardPrioritySort(page: Page, board: ReturnType<Page["locator"]>) {
  const display = page.getByRole("button", { name: /^Work display/ });
  await display.click();
  await page.getByRole("group", { name: "Sort by" }).getByRole("menuitemradio", { name: "Priority", exact: true }).click();
  await page.keyboard.press("Escape");
  const ranks = await board.locator(".bcol").evaluateAll((lanes) => lanes.map((lane) => [...lane.querySelectorAll(".bcard .bpri")].map((el) => {
    const word = el.getAttribute("aria-label")!.match(/(Low|Medium|High|Urgent)/)![1];
    return { Low: 1, Medium: 2, High: 3, Urgent: 4 }[word]!;
  })));
  assert.ok(ranks.every((lane) => lane.every((rank, i) => i === 0 || rank <= lane[i - 1])), `Each lane runs from the highest displayed priority down (${JSON.stringify(ranks)})`);
  const order = await board.locator("#bcol-in-progress .bcard").evaluateAll((cards) => cards.map((card) => card.getAttribute("data-work-id")));
  assert.ok(order.indexOf("T-A84") < order.indexOf("T-A121"), `Inferred High (Minor severity) sorts above set Medium (${order})`);
  const summary = page.locator("#work-results > .board-filterbar");
  assert.match((await summary.innerText()).replace(/\s+/g, " "), /Highest priority first/, "The summary names the sort and its direction");
  assert.doesNotMatch(await summary.innerText(), /\bcards?\b|Stages columns/, "The summary repeats neither the preset count nor the mode the toolbar shows");
  await display.click();
  await page.getByRole("group", { name: "Direction" }).getByRole("menuitemradio", { name: "Lowest first" }).click();
  await page.keyboard.press("Escape");
  assert.match((await summary.innerText()).replace(/\s+/g, " "), /Lowest priority first/);
  await display.click();
  await page.getByRole("group", { name: "Direction" }).getByRole("menuitemradio", { name: "Highest first" }).click();
  await page.keyboard.press("Escape");
  await display.click();
  await page.getByRole("group", { name: "Sort by" }).getByRole("menuitemradio", { name: "Attention first" }).click();
  await page.keyboard.press("Escape");
}

/* One canonical card: a single top row (project chip, one pill, the reference), the title, progress, the assignee, then
   the right-aligned footer icons with the Needs you bell. No stripe, no outlines, no second chip row, in every theme. */
async function checkCardLayout(page: Page, board: ReturnType<Page["locator"]>) {
  await page.setViewportSize({ width: 375, height: 812 });
  const card = board.locator('.bcard[data-work-id="T-A146"]');
  /* Resizing can re-render the page, so measure in one pass once it has settled */
  await page.waitForTimeout(300);
  const [top, title, progress, owner, foot, need, box] = await card.evaluate((el) => [".board-top", ":scope > .work-item-link", ".board-progress", ".board-owner-label a", ".board-facts", ".board-facts .board-need", ":scope"]
    .map((sel) => { const r = (sel === ":scope" ? el : el.querySelector(sel))!.getBoundingClientRect(); return { x: r.x, y: r.y, width: r.width, height: r.height }; }));
  assert.ok(top!.y + top!.height <= title!.y + 1, "The top row sits above the title");
  assert.ok(progress!.y >= title!.y + title!.height - 1 && owner!.y >= progress!.y + progress!.height - 1, "Progress, then the assignee");
  assert.ok(foot!.y + foot!.height / 2 >= owner!.y, "The icons sit on the assignee line or under it");
  assert.ok(Math.abs(need!.x + need!.width - (box!.x + box!.width)) <= 80, "The bell sits at the right of the card");
  const [projectLink, ownerLink] = await card.evaluate((el) => [".board-project a", ".board-owner-label a"].map((sel) => { const r = el.querySelector(sel)!.getBoundingClientRect(); return [r.width, r.height]; }));
  assert.ok(projectLink[1] >= 44 && ownerLink[0] >= 44 && ownerLink[1] >= 44, `Both card links are 44px targets (${projectLink} ${ownerLink})`);
  assert.ok(box!.height <= 300, `The first card stays compact at 375px (${Math.round(box!.height)}px)`);
  const borders = await board.evaluate((el) => [el.querySelector(".bcol")!, el.querySelector(".bcard")!].map((n) => {
    const st = getComputedStyle(n);
    return st.borderTopStyle === "none" || st.borderTopWidth === "0px" || st.borderTopColor === "rgba(0, 0, 0, 0)" ? "none" : `${st.borderTopWidth} ${st.borderTopColor}`;
  }));
  assert.deepEqual(borders, ["none", "none"], "Lanes and cards draw no outline");
  await page.setViewportSize({ width: 1440, height: 900 });
  for (const theme of ALL_THEMES) await checkCardTheme(page, board, theme);
  await page.evaluate(() => document.documentElement.setAttribute("data-theme", "graphite"));
}

/* The assignee row carries the card's icons (no separate footer row); Agent, Person and Owner are icons named by their
   word; the team is a dot named by its team, its name shown on hover. The top row's parts stay apart without CSS. */
async function checkAssigneeRow(page: Page, board: ReturnType<Page["locator"]>) {
  await page.mouse.move(0, 0);
  assert.equal(await board.locator(".board-foot").count(), 0, "No card has a separate footer row");
  const card = board.locator('.bcard[data-work-id="T-A118"]');
  const [ownerMid, iconsMid] = await card.evaluate((el) => [".board-assignee", ".board-facts"].map((sel) => { const r = el.querySelector(sel)!.getBoundingClientRect(); return r.top + r.height / 2; }));
  assert.ok(Math.abs(ownerMid - iconsMid) <= 4, `The icons share the assignee's line at 1440 (${ownerMid} ${iconsMid})`);
  assert.equal((await card.locator(".board-top").textContent())!.replace(/\s+/g, " ").trim(), "Office federation High T-A118", "The top row's parts are spaced in the markup");
  const kinds = await board.locator(".bcard .board-kind").evaluateAll((els) => els.map((el) => [el.getAttribute("aria-label"), el.getAttribute("data-icon-tip"), el.getAttribute("role"), el.querySelectorAll(".ic").length, (el as HTMLElement).innerText.trim()]));
  assert.ok(kinds.length > 5 && kinds.every(([name, tip, role, icons, text]) => ["Agent", "Person", "Owner"].includes(name!) && tip === name && role === "img" && icons === 1 && !text), `Kind is an icon named Agent, Person or Owner (${JSON.stringify(kinds)})`);
  assert.equal(await card.locator(".board-kind").getAttribute("aria-label"), "Agent");
  assert.equal(await board.locator('.bcard[data-work-id="T-A142"] .board-kind').getAttribute("aria-label"), "Owner");
  const words = await board.locator(".bcard .board-card-context").evaluateAll((els) => els.filter((el) => /\b(Agent|Person)\b/.test((el as HTMLElement).innerText)).length);
  assert.equal(words, 0, "No card spells out Agent or Person");
  assert.equal((await board.locator('.bcard[data-work-id="T-A121"] .board-need').innerText()).replace(/\s+/g, " ").trim(), "3 · 4h", "The bell's count and expiry read apart");
  await checkTeamDot(page, card);
}

/* The team is a dot named by its team; hovering the card, or keyboard focus inside it, shows the name in place */
async function checkTeamDot(page: Page, card: ReturnType<Page["locator"]>) {
  const team = card.locator(".board-team-tag"), name = team.locator(".board-team-name");
  assert.equal(await team.getAttribute("aria-label"), "Platform (Rust)", "The team dot is named by its team");
  assert.equal(await team.getAttribute("data-icon-tip"), "Platform (Rust)", "…and its tooltip says it");
  assert.ok((await name.evaluate((el) => el.getBoundingClientRect().width)) <= 1, "At rest the team is only its dot");
  await card.locator(".board-desc").hover({ force: true });
  await page.waitForTimeout(250);
  assert.ok((await name.evaluate((el) => el.getBoundingClientRect().width)) > 20, "Hovering the card shows the team name");
  const height = await card.evaluate((el) => el.getBoundingClientRect().height);
  await page.mouse.move(0, 0);
  await page.waitForTimeout(250);
  assert.ok(Math.abs(height - (await card.evaluate((el) => el.getBoundingClientRect().height))) <= 1, "Showing the team name never changes the card's height");
  await card.locator(".board-owner-label a").focus();
  await page.keyboard.press("Shift+Tab");
  await page.keyboard.press("Tab");
  assert.ok((await name.evaluate((el) => el.getBoundingClientRect().width)) > 20, "Keyboard focus inside the card shows the team name");
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
}

/* In one theme: every card has exactly one chip row and no stripe; the bell and descriptions keep their contrast */
async function checkCardTheme(page: Page, board: ReturnType<Page["locator"]>, theme: string) {
  await page.evaluate((t) => document.documentElement.setAttribute("data-theme", t), theme);
  /* Let the card's colour transition settle before reading colours */
  await page.waitForTimeout(250);
  {
    const cards = await board.locator(".bcard").evaluateAll((els) => els.map((el) => {
      const row = el.querySelectorAll(":scope > .board-top"), parts = [...(row[0]?.children ?? [])].filter((c) => c.getClientRects().length);
      const tops = parts.map((c) => { const r = c.getBoundingClientRect(); return r.top + r.height / 2; });
      const stripe = getComputedStyle(el, "::before");
      return { id: el.getAttribute("data-work-id"), rows: row.length, pills: el.querySelectorAll(".bpill").length, chips: el.querySelectorAll(".board-chips, .bchip, .board-status").length,
        spread: Math.max(...tops) - Math.min(...tops), stripe: stripe.content !== "none" && stripe.borderInlineStartWidth !== "0px" };
    }));
    for (const c of cards) {
      assert.ok(c.rows === 1 && c.pills === 1 && c.chips === 0 && c.spread <= 4, `${c.id} has exactly one chip row in ${theme} (${JSON.stringify(c)})`);
      assert.equal(c.stripe, false, `${c.id} has no attention stripe in ${theme}`);
    }
    /* The bell is the card's attention signal: amber, its icon at 3:1 and its text at 4.5:1 against the card */
    const bellEl = board.locator('.bcard[data-work-id="T-A121"] .board-need');
    const bell = { text: await bellEl.evaluate(textContrast), icon: await bellEl.evaluate(iconContrast) };
    assert.ok(bell.text >= 4.5 && bell.icon >= 3, `The Needs you bell stays visible in ${theme} (${JSON.stringify(bell)})`);
    const descEl = board.locator('.bcard[data-work-id="T-A121"] .board-desc');
    await descEl.evaluate((el) => Object.assign((el.closest(".bcard") as HTMLElement).style, { background: "var(--bd-card-hover)", transition: "none" }));
    const desc = await descEl.evaluate(textContrast);
    await descEl.evaluate((el) => Object.assign((el.closest(".bcard") as HTMLElement).style, { background: "", transition: "" }));
    assert.ok(desc >= 4.5, `Description ink holds 4.5:1 on the hover fill in ${theme} (${desc.toFixed(2)})`);
  }
}

/* Hovering a card, or focusing its title, shows the card tooltip only when the title is clamped (it reveals the rest).
   Footer icons have their own tooltips on hover and keyboard focus. Escape dismisses them. */
async function checkCardTooltip(page: Page, board: ReturnType<Page["locator"]>) {
  const tip = page.locator(".board-tip:not(.board-icon-tip)");
  const title = board.locator('.bcard[data-work-id="T-A121"] > .work-item-link');
  await title.hover();
  await page.waitForTimeout(600);
  assert.equal(await page.locator(".board-tip:not(.board-icon-tip):popover-open").count(), 0, "A title that fits gets no tooltip");
  await page.mouse.move(0, 0);
  await title.evaluate((el) => { el.dataset.was = el.textContent!; el.textContent = "Split codec 2a.3.2b ".repeat(8).trim(); });
  await title.hover();
  await page.locator(".board-tip:not(.board-icon-tip):popover-open").waitFor();
  assert.match((await tip.innerText()).replace(/\s+/g, " ").trim(), /In progress · ETA 2h$/, "A clamped title's tooltip shows it whole, with status and ETA");
  assert.doesNotMatch(await tip.innerText(), /drag/i);
  assert.equal(await board.locator(".bcard[data-tip], .bcard [data-tip], .bcard[title]").count(), 0, "Cards have one tooltip, not the shell's or the browser's as well");
  assert.equal(await tip.getAttribute("aria-hidden"), "true", "The tooltip repeats the card, so it is hidden from assistive technology");
  const [tipBox, cardBox] = await Promise.all([tip.boundingBox(), board.locator('.bcard[data-work-id="T-A121"]').boundingBox()]);
  assert.ok(tipBox!.y + tipBox!.height <= cardBox!.y + 1, "The tooltip sits above the card, clear of its content");
  await page.keyboard.press("Escape");
  await page.locator(".board-tip:not(.board-icon-tip):popover-open").waitFor({ state: "detached" });
  await title.evaluate((el) => { el.textContent = el.dataset.was!; delete el.dataset.was; });
  await page.mouse.move(0, 0);
  /* Icons: hover and keyboard focus show the full text; it matches the accessible name */
  const eta = board.locator('.bcard[data-work-id="T-A121"] .bdate');
  assert.equal(await eta.getAttribute("aria-label"), "ETA 2h", "The ETA is an icon named in full");
  assert.equal((await eta.innerText()).trim(), "", "The ETA has no visible text");
  await eta.hover();
  const iconTip = page.locator(".board-icon-tip:popover-open");
  await iconTip.waitFor();
  assert.equal(await iconTip.textContent(), "ETA 2h", "Hovering the icon shows its tooltip");
  await page.mouse.move(0, 0);
  await iconTip.waitFor({ state: "detached" });
  await eta.focus();
  await iconTip.waitFor();
  assert.ok(await eta.evaluate((el) => getComputedStyle(el).outlineStyle !== "none"), "A focused icon shows a focus ring");
  await page.keyboard.press("Escape");
  await iconTip.waitFor({ state: "detached" });
}

/* Filtered results that leave out archived matches say so beside the count, with the way to reveal them */
async function checkMixedArchivedMatches(page: Page, board: ReturnType<Page["locator"]>) {
  await page.getByRole("button", { name: /^Filter work/ }).click();
  await page.getByRole("group", { name: "Status" }).getByRole("menuitemradio", { name: "Approved", exact: true }).click();
  await page.keyboard.press("Escape").catch(() => {});
  const summary = page.locator("#work-results > .board-filterbar");
  const text = (await summary.innerText()).replace(/\s+/g, " ");
  const [, shown, archived] = (text.match(/^(\d+) shown\W+(\d+) archived match/) ?? assert.fail(`Mixed results summary: ${text}`)).map(Number);
  assert.ok(shown > 0 && archived > 0, `Mixed results say what is shown and what is archived (${text})`);
  assert.equal(await board.locator(".bcard").count(), shown);
  const before = await board.locator(".bcard").evaluateAll((cards) => cards.map((c) => c.getAttribute("data-work-id")));
  await summary.getByRole("button", { name: `Show ${archived} archived ${archived === 1 ? "match" : "matches"}` }).click();
  assert.equal(await board.locator(".bcard").count(), shown + archived, "Revealing adds the archived matches");
  const focused = await page.evaluate(() => document.activeElement?.closest(".bcard")?.getAttribute("data-work-id"));
  assert.ok(focused && !before.includes(focused), `Focus lands on the newly revealed card (${focused}, already shown: ${before})`);
  assert.ok(await page.evaluate(() => document.activeElement?.matches(".bcard > .work-item-link")), "Focus is on the revealed card's title");
  await page.waitForFunction((n) => document.getElementById("work-results-status")?.textContent === `${n} more ${n === 1 ? "card" : "cards"} shown`, archived);
  assert.doesNotMatch((await summary.innerText()).replace(/\s+/g, " "), /archived match/, "Nothing is left hidden");
  await page.locator(".work-archived-chip").click();
  await page.locator("#work-results").getByRole("button", { name: "Clear filters" }).click();
}

/* Answering a card's last request in Inbox and coming back: focus finds the card's title in its new lane; a card that
   left the view hands focus to the active view and the count is announced */
async function checkReturnAfterResolve(page: Page, board: ReturnType<Page["locator"]>) {
  const back = page.getByRole("link", { name: "Back to work board" });
  await board.locator('.bcard[data-work-id="T-A118"] a.board-need').focus();
  await page.keyboard.press("Enter");
  await page.getByRole("button", { name: "Use timed access" }).click();
  await back.click();
  const title = board.locator('.bcard[data-work-id="T-A118"] > .work-item-link');
  await title.waitFor();
  assert.ok(await focusedIs(title), "With its badge gone, focus returns to the card's title");
  assert.equal(await title.evaluate((el) => el.closest(".bcol")!.id), "bcol-to-do", "The card moved lanes");
  await page.waitForFunction(() => /T-A118 is now in To do/.test(document.getElementById("work-results-status")?.textContent ?? ""));
  const shown = await title.evaluate((el) => { const r = el.getBoundingClientRect(), v = el.closest(".board")!.getBoundingClientRect(); return r.top >= v.top && r.bottom <= v.bottom; });
  assert.ok(shown, "The card's title is in view");
  await page.locator('[data-act="work-preset"][data-v="needs-me"]').click();
  /* Wait for the Needs you view to render before focusing, then for the Inbox item to open */
  await page.locator('[data-act="work-preset"][data-v="needs-me"][aria-pressed="true"]').waitFor();
  const handoff = board.locator('.bcard[data-work-id="T-A146"] a.board-need');
  await handoff.waitFor();
  await handoff.focus();
  await page.keyboard.press("Enter");
  await page.waitForURL(/#\/inbox\/in14/);
  await page.getByRole("button", { name: "Take ownership of T-A146", exact: true }).click();
  await back.click();
  const view = page.locator('[data-act="work-preset"][data-v="needs-me"]');
  await page.waitForFunction(() => document.activeElement?.matches('[data-act="work-preset"][data-v="needs-me"]'));
  assert.equal(await board.locator('.bcard[data-work-id="T-A146"]').count(), 0, "The answered card left Needs you");
  await page.waitForFunction(() => /T-A146 is no longer in this view\. \d+ cards? shown\./.test(document.getElementById("work-results-status")?.textContent ?? ""));
  assert.equal(await view.getAttribute("aria-pressed"), "true");
  await page.locator('[data-act="work-preset"][data-v="all"]').click();
}

/* Denying T-A124's access request: the card leaves Needs you, keeps its block, says "Access denied" and who acts next.
   Undo brings the request back. */
async function checkDeniedAccess(page: Page, board: ReturnType<Page["locator"]>) {
  const needs = async () => Number(await page.locator('[data-work-count="needs-me"]').textContent());
  const before = await needs();
  await board.locator('.bcard[data-work-id="T-A124"] a.board-need').click();
  await page.waitForURL(/#\/inbox\/in5/);
  await page.locator('#main [data-act="deny-arm"][data-id="in5"]').click();
  await page.locator('#main [data-act="deny-confirm"][data-id="in5"], #main [data-act="send-reply"][data-id="in5"]').first().click();
  await page.getByRole("link", { name: "Back to work board" }).click();
  const card = board.locator('.bcard[data-work-id="T-A124"]');
  await card.waitFor();
  assert.equal(await card.getAttribute("data-attention"), null, "A denied request leaves Needs you");
  assert.equal(await card.locator(".board-need").count(), 0, "No bell or inert Unblock control");
  assert.equal(await card.evaluate((el) => el.closest(".bcol")!.id), "bcol-blocked", "The block stays visible");
  assert.equal((await card.locator(".board-reason").innerText()).trim(), "Access denied · Waiting on Jules", "It says the outcome and the next actor");
  assert.equal(await needs(), before - 1, "Needs you counts one fewer");
  await page.locator(".toast-undo").last().click();
  await card.locator(".board-need").waitFor();
  assert.equal(await card.locator(".board-reason").count(), 0, "Undo brings the request back");
}

/* Approving T-A81's fix closes it today, so it stays in recent Done history; Undo restores the pending verification */
async function checkVerifyApproved(page: Page, board: ReturnType<Page["locator"]>) {
  await board.locator('.bcard[data-work-id="T-A81"] a.board-need').click();
  await page.waitForURL(/#\/inbox\/in11/);
  await page.getByRole("button", { name: "Approve fix", exact: true }).click();
  await page.getByRole("link", { name: "Back to work board" }).click();
  const card = board.locator('#bcol-done .bcard[data-work-id="T-A81"]');
  await card.waitFor();
  assert.equal(await card.locator(".bdate").getAttribute("aria-label"), "Closed today", "The approval is dated today");
  assert.equal(await card.locator(".bstatus").getAttribute("aria-label"), "Status: Closed");
  assert.equal(await card.locator(".board-need").count(), 0);
  await page.locator(".toast-undo").last().click();
  await card.locator(".board-need").waitFor();
  assert.equal(await card.locator(".bdate").getAttribute("aria-label"), "Waiting on your verification", "Undo restores the pending verification");
}

const focusedIs = (locator: ReturnType<Page["locator"]>) => locator.evaluate((el) => el === document.activeElement);

/* Request menus live in the top layer, stay inside the viewport and hand focus back predictably */
async function checkBoardRequestMenus(page: Page, board: ReturnType<Page["locator"]>) {
  assert.match((await page.locator(".queue-count").textContent())!, /^Across all work: \d+ open · \d+ need you$/, "Headline totals say they cover all work");
  const card = board.locator('.bcard[data-work-id="T-A121"]');
  const trigger = card.locator("button.board-need");
  assert.equal(await trigger.getAttribute("aria-label"), "Needs you: 3 requests, 1 expires in 4 hours", "The bell's name says how many and when the soonest expires");
  assert.equal(await trigger.getAttribute("data-icon-tip"), await trigger.getAttribute("aria-label"), "Its tooltip says the same");
  assert.equal((await trigger.locator(".board-need-when").textContent())!.trim(), "4h", "The soonest expiry shows in short beside the bell");
  assert.equal(await card.locator(".board-need-due, .board-status").count(), 0, "No separate badge row or expiry line in the card body");
  await page.setViewportSize({ width: 1024, height: 520 });
  /* Lanes overflow at 1024: put the card's lane against the board's right edge, then open its menu */
  await board.evaluate((el) => {
    const lane = el.querySelector('.bcard[data-work-id="T-A121"]')!.closest(".bcol")!;
    el.scrollLeft += lane.getBoundingClientRect().right - el.getBoundingClientRect().right + 4;
  });
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
  assert.ok(await page.evaluate(() => Boolean(document.activeElement?.closest('.bcard[data-work-id="T-A121"] .board-facts')) && !document.activeElement?.matches(".board-need")), "Tab moves on to the next control on the card");
  /* Scrolling the badge under its lane's sticky header closes its menu, though the badge is still in the viewport */
  await board.evaluate((el) => { el.scrollTop = 0; });
  await trigger.click();
  await menu.waitFor();
  await board.evaluate((el) => {
    const t = el.querySelector('.bcard[data-work-id="T-A121"] button.board-need')!.getBoundingClientRect();
    /* Where the sticky header's bottom sits once it is pinned to the top of the board */
    const head = el.querySelector('.bcard[data-work-id="T-A121"]')!.closest(".bcol")!.querySelector(".bcol-h")!.getBoundingClientRect();
    el.scrollTop += t.top + t.height / 2 - (el.getBoundingClientRect().top + head.height) + 4;
  });
  await page.waitForFunction(() => !document.querySelector(".board-need-menu:popover-open"));
  assert.ok(await trigger.evaluate((el) => el.getBoundingClientRect().bottom > 0), "The badge is still inside the viewport, under the header");
  assert.equal(await trigger.getAttribute("aria-expanded"), "false");
  await board.evaluate((el) => { el.scrollTop = 0; });
  await page.setViewportSize({ width: 1440, height: 900 });
}

/* Contrast of an icon's colour against its card's background */
function iconContrast(el: Element) {
  const card = el.closest(".bcard")!;
  const parse = (c: string) => {
    const m = c.match(/[\d.]+/g)!.map(Number);
    return c.startsWith("color(srgb") ? [m[0] * 255, m[1] * 255, m[2] * 255] : [m[0], m[1], m[2]];
  };
  const channel = (v: number) => (v / 255 <= 0.03928 ? v / 255 / 12.92 : ((v / 255 + 0.055) / 1.055) ** 2.4);
  const lum = (c: number[]) => 0.2126 * channel(c[0]) + 0.7152 * channel(c[1]) + 0.0722 * channel(c[2]);
  const [a, b] = [lum(parse(getComputedStyle(el).color)), lum(parse(getComputedStyle(card).backgroundColor))];
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
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
  /* The Done lane's ⋯ opens the same choices as a popover anchored to it */
  const more = done.locator("button.bcol-window");
  await more.click();
  const pop = page.getByRole("menu", { name: "Done history" });
  const current = pop.getByRole("menuitemradio", { name: "Last 7 days" });
  await current.waitFor();
  assert.ok(await focusedIs(current), "The Done lane's ⋯ opens Done history with the current choice focused");
  const [popBox, moreBox] = [(await pop.boundingBox())!, (await more.boundingBox())!];
  assert.ok(Math.abs(popBox.y - (moreBox.y + moreBox.height)) <= 12 && Math.abs(popBox.x + popBox.width / 2 - (moreBox.x + moreBox.width / 2)) <= popBox.width, "The choices open next to the ⋯");
  assert.equal(await page.locator("#menu-work-display:not([hidden])").count(), 0, "Display stays closed");
  await page.keyboard.press("Escape");
  assert.ok(await focusedIs(more), "Escape returns focus to the ⋯");
  await more.press("Enter");
  await pop.getByRole("menuitemradio", { name: "Include archived" }).press("Enter");
  assert.ok(await focusedIs(done.locator("button.bcol-window")), "Choosing returns focus to the ⋯");
  assert.equal(await done.locator("button.bcol-window").getAttribute("aria-label"), "Done history: With archived. Change");
  assert.ok(await done.locator(".bcard").count() > before, "Include archived reaches back past the last week");
  await display.click();
  assert.equal(await page.getByRole("group", { name: "Done history" }).getByRole("menuitemradio", { name: "Include archived" }).getAttribute("aria-checked"), "true", "Display shows the same setting");
  await page.getByRole("group", { name: "Done history" }).getByRole("menuitemradio", { name: "Last 7 days" }).click();
  assert.equal(await done.locator(".bcard").count(), before);
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
  /* T-A81 (Fixed, its fix still waiting for approval) is never archived, so it is shown before the reveal */
  const before = await board.locator(".bcard").count();
  assert.equal(await board.locator('.bcard[data-work-id="T-A81"]').count(), 1, "A Fixed item with a pending approval is not archived");
  /* Lanes with no matches collapse to their heading, so the matches get the room */
  const lanes = await board.locator(".bcol").evaluateAll((cols) => cols.map((c) => ({ id: c.id, shut: c.hasAttribute("data-collapsed"), cards: c.querySelectorAll(".bcard").length, w: c.getBoundingClientRect().width })));
  const open = lanes.filter((l) => !l.shut), shut = lanes.filter((l) => l.shut);
  assert.ok(shut.length > 0 && shut.every((l) => l.cards === 0 && l.w < Math.min(...open.map((o) => o.w)) / 1.5), `Zero-match lanes collapse (${JSON.stringify(lanes)})`);
  const summary = (await page.locator("#work-results > .board-filterbar").innerText()).replace(/\s+/g, " ");
  assert.match(summary, /^\d+ (shown|match|matches)\b.*Status: Fixed/, `The summary says the match count and the filter (${summary})`);
  await reveal.click();
  assert.equal(await board.locator(".bcard").count(), before + n, "Show archived matches reveals each of them");
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
  assert.ok(await hit.first().evaluate((el) => el.scrollWidth <= el.clientWidth + 1 && el.querySelector(".board-subhit-title")!.scrollWidth <= el.querySelector(".board-subhit-title")!.clientWidth + 1), "The subtask title wraps instead of clipping");
  const summary = (await page.locator("#work-results > .board-filterbar").innerText()).replace(/\s+/g, " ");
  assert.match(summary, /^1 match\b/, `The summary gives the match count (${summary})`);
  assert.doesNotMatch(summary, /Inventory CSP/, "The summary does not repeat the query under the search field");
  assert.equal(await page.locator("#work-results").getByRole("button", { name: "Clear filters" }).count(), 1, "The summary offers recovery");
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
  /* Scrolled, with the badge still in view (the badge sits just under the card's title) */
  await board.evaluate((el) => { el.scrollTop = 30; });
  await badge.focus();
  const top = await board.evaluate((el) => el.scrollTop);
  assert.ok(top > 0, "The board is scrolled before leaving");
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
  assert.match((await display.innerText()).trim(), /^Display\b/, "At 420px and below Display keeps its word");
  assert.match((await display.getAttribute("aria-label"))!, /^Work display, Stages columns/);
  const valuesHidden = await page.locator(".work-toolbar").evaluate((el) => el.getAttribute("data-values") === "hidden");
  const modeShown = await page.locator(".board-lanes .board-lanes-mode").isVisible();
  assert.equal(modeShown, valuesHidden, "The lane picker's label names the column mode exactly when the toolbar hides it");
  const toggles = await page.getByRole("group", { name: "Work layout" }).getByRole("button").evaluateAll((els) => els.map((el) => { const r = el.getBoundingClientRect(); return [r.width, r.height]; }));
  assert.ok(toggles.every(([w, h]) => w >= 44 && h >= 44), `List and Board are 44 × 44 on phones (${JSON.stringify(toggles)})`);
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
  /* Let the resize settle (layout and the measured compact labels) before counting toolbar rows */
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  await page.waitForTimeout(150);
  const rows = await page.locator(".work-toolbar > *").evaluateAll((els) => els.filter((el) => el.getClientRects().length).map((el) => el.getBoundingClientRect().top).sort((a, b) => a - b)
    .filter((top, i, tops) => i === 0 || top - tops[i - 1] > 20).length);
  /* Two rows at 320px: the views, then layout, search, Filter and Display; nothing is clipped or scrolls sideways */
  assert.equal(rows, 2, `The 320px toolbar has two rows (${rows})`);
  /* New task is the page's primary action: at the end of the title row, above the toolbar, at every width */
  const [titleBox, tabsTop, newBox] = await page.evaluate(() => [document.querySelector("#main h1")!.getBoundingClientRect().toJSON(), document.querySelector(".work-toolbar .primary-tab-row")!.getBoundingClientRect().top, document.getElementById("work-new-task")!.getBoundingClientRect().toJSON()]);
  assert.ok(newBox.top < tabsTop && newBox.top < titleBox.bottom && newBox.left > titleBox.right, "New sits at the end of the title row, above the views");
  assert.ok(newBox.right <= 320, "New stays inside the 320px viewport");
  assert.ok(newBox.width >= 44 && newBox.height >= 44, "New stays a 44px target");
  assert.match((await page.getByRole("button", { name: /^Filter work/ }).innerText()).trim(), /^Filter$/, "At 320px Filter keeps its word");
  assert.equal((await page.locator(".board-lanes .board-lanes-label").innerText()).trim(), "Stages", "At 320px the picker's label names the column mode the toolbar hides");
  assert.equal(await page.locator("#work-results > .board-filterbar").count(), 0, "No summary row just for the mode");
  assert.match((await page.locator(".queue-count").innerText()).trim(), /^Across all work: \d+ open$/, "At 320px the headline keeps the open count, not the Needs you tab's count");
  const overflow = await page.locator(".work-toolbar").evaluate((el) => el.scrollWidth - el.clientWidth);
  assert.ok(overflow <= 1, `The 320px toolbar does not overflow (${overflow}px)`);
  assert.match((await page.getByRole("button", { name: /^Work display/ }).innerText()).trim(), /^Display$/, "At 320px Display keeps its word; the mode is in the picker label");
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
  /* Widening closes the drawer and steps back over its history entry; let that settle before navigating. */
  await page.locator(".chat.list-open").waitFor({ state: "detached" });
  await page.waitForFunction(() => !history.state?.chatOverlay);
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
  await page.locator(".thread").getByRole("button", { name: "Take ownership of T-A146" }).waitFor();
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
  assert.deepEqual(await more.getByRole("menuitem").evaluateAll((els) => els.map((e) => e.getAttribute("href"))), ["#/inbox/in1", "#/inbox/in4"], "Ada's other requests link to their Inbox records");
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
/* ---------------------------------------------------------------- Chat review round 6 */
const navTo = (page: Page, h: string) => page.evaluate((x) => { location.hash = x; }, h);
const activeAttr = (page: Page, name: string) => page.evaluate((n) => document.activeElement?.getAttribute(n) ?? null, name);
/* Touch keeps where a follow-up posts and that the ask stays open; only the shortcut hint goes. */
async function checkChatTouch(page: Page) {
  const context = await page.context().browser()!.newContext({ viewport: { width: 375, height: 667 }, hasTouch: true, isMobile: true, reducedMotion: "reduce" });
  const touch = await context.newPage();
  const errors: string[] = [];
  touch.on("pageerror", (e) => errors.push(e.message));
  touch.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
  try {
    touch.setDefaultTimeout(5000);
    await touch.goto(page.url().split("#")[0] + "#/chat/agency-v1");
    assert.ok(await touch.evaluate(() => matchMedia("(pointer: coarse)").matches), "Touch context reports a coarse pointer");
    assert.equal(await touch.locator("#compose-help").isVisible(), false, "The Ctrl/⌘ hint is hidden on touch");
    await touch.locator('.chat-needs:has(#needs-m3) [data-act="chat-ask-reply"]').click();
    const dest = touch.locator(".chat-needs .chat-ask-dest");
    assert.ok(await dest.isVisible(), "The follow-up destination shows on touch");
    assert.match(await dest.innerText(), /^Posts in #agentic-org-v1 and on the Inbox request\. It stays open until you decide\.$/);
    await touch.locator('.chat-needs [data-act="chat-ask-send"]').click();
    assert.ok(await touch.locator(".chat-needs .chat-ask-dest.is-error").isVisible(), "An empty follow-up's error shows on touch");
    assert.deepEqual(errors, [], "No errors in the touch context");
  } finally {
    await context.close();
  }
}
/* The linked message's author line is clear of the stuck day band, and focused. */
async function linkedPlacement(page: Page, mid: string) {
  await page.locator(`.msg.is-linked[data-mid="${mid}"]`).waitFor();
  await page.waitForTimeout(300);
  return page.evaluate((id) => {
    const s = document.querySelector(".chmain .msgs[data-ch]")!, el = s.querySelector(`:scope > [data-mid="${id}"]`)!;
    const band = [...s.querySelectorAll(":scope > .chat-day")].filter((d) => d.classList.contains("is-stuck")).pop();
    const head = el.querySelector(".hd")!.getBoundingClientRect(), top = s.getBoundingClientRect().top;
    return { focused: document.activeElement === el, headTop: head.top - top, bandBottom: band ? band.getBoundingClientRect().bottom - top : 0, inView: head.top >= top };
  }, mid);
}
async function assertLinked(page: Page, mid: string, label: string) {
  const at = await linkedPlacement(page, mid);
  assert.ok(at.focused, `${label}: focus moves to the linked message`);
  assert.ok(at.inView && at.headTop >= at.bandBottom, `${label}: the day band does not cover the author line (${JSON.stringify(at)})`);
}
/* Every source navigation re-anchors: a repeat visit to the same URL, and a click on the URL already open. */
async function checkChatSourceRepeat(page: Page) {
  await page.setViewportSize({ width: 375, height: 667 });
  for (const visit of ["first", "repeat"]) {
    await navTo(page, "#/inbox/in1");
    await page.getByRole("link", { name: "Open source message" }).click();
    await assertLinked(page, "m2", `${visit} visit`);
    await page.evaluate(() => { document.querySelector(".chmain .msgs[data-ch]")!.scrollTop = 0; (document.activeElement as HTMLElement)?.blur(); });
  }
  await page.evaluate(() => {
    const a = document.createElement("a");
    a.href = location.hash; a.id = "same-url-link"; a.textContent = "same";
    document.querySelector("#main .chat-heading")!.append(a);
  });
  await page.locator("#same-url-link").click();
  await assertLinked(page, "m2", "same-URL click");
  await page.setViewportSize({ width: 1440, height: 900 });
}
/* Home's Spotlight and cards route a consequential ask to Inbox with the same reason as Chat. */
async function checkHomeRouting(page: Page) {
  await navTo(page, "#/chat/agency-v1");
  const chatReason = await page.locator('.chat-needs:has(#needs-m2) .chat-needs-route').innerText();
  await navTo(page, "#/home");
  const spot = page.locator("#hb-spotlight");
  await spot.getByRole("link", { name: "Review in Inbox" }).waitFor();
  assert.equal(await spot.getByRole("link", { name: "Review in Inbox" }).getAttribute("href"), "#/inbox/in1");
  assert.equal(await spot.locator(".ask-route").innerText(), chatReason, "Home states the same short reason as Chat");
  const direct = await page.locator("#main [data-act='handle'], #main [data-act='deny-arm'], #main [data-act='reply']").evaluateAll((els) => els.map((e) => e.getAttribute("data-id")));
  for (const id of ["in1", "in2", "in4", "in16"]) assert.ok(!direct.includes(id), `Home offers no direct decision on consequential ${id}`);
}
/* The thread's "Also send to" choice is kept with its draft and cleared on send. */
async function checkThreadAlsoDraft(page: Page) {
  await navTo(page, "#/chat/agency-v1");
  await page.locator('.msg[data-mid="m2"] .thr').click();
  await page.locator("#thread-compose").fill("Holding the merge until the fixtures land.");
  await page.locator(".thread .chat-also input").check();
  await page.locator(".thread [data-act=thread-close]").click();
  await page.locator(".chat-search").click(); /* an unrelated re-render */
  await page.locator(".chat-search-row [aria-label='Close search']").click();
  await page.locator('.msg[data-mid="m2"] .thr').click();
  assert.equal(await page.locator("#thread-compose").inputValue(), "Holding the merge until the fixtures land.");
  assert.ok(await page.locator(".thread .chat-also input").isChecked(), "The cross-post choice is kept with the draft");
  await page.locator("#thread-compose").press("Control+Enter");
  await page.locator(".chmain .msgs > .msg", { hasText: "Holding the merge until the fixtures land." }).locator(".msg-src").waitFor();
  assert.equal(await page.locator(".thread .chat-also input").isChecked(), false, "Sending clears the cross-post choice");
  await page.locator(".thread [data-act=thread-close]").click();
}
/* Search covers thread replies; a match opens in its thread, marked and focused. */
async function checkChatSearchThread(page: Page) {
  await navTo(page, "#/chat/agency-v1");
  await page.locator(".chat-search").click();
  await page.keyboard.type("as soon as");
  const hitLine = page.locator('.chmain .msg[data-mid="m2"] .chat-thread-hit');
  await hitLine.waitFor();
  assert.match(await hitLine.innerText(), /Kai in the thread: .*I'll merge as soon as it's approved/);
  assert.equal(await page.locator("#chat-search-status").innerText(), "1 thread reply", "A match only in a reply counts as a reply");
  await hitLine.getByRole("button", { name: "Open in thread" }).click();
  const reply = page.locator(".thread .msgs > .msg.is-linked");
  await reply.waitFor();
  assert.match(await reply.innerText(), /I'll merge as soon as it's approved/);
  assert.ok(await reply.evaluate((el) => el === document.activeElement), "Focus moves to the matching reply");
  assert.equal(await page.locator(".thread mark").first().innerText(), "as soon as");
  await page.locator(".thread [data-act=thread-close]").click();
  await page.locator(".chat-search-row [aria-label='Close search']").click();
}
/* Eden's one-tap question shows what the choice weighs above the answers. */
async function checkChoiceContext(page: Page) {
  await navTo(page, "#/chat/dm-eden");
  const card = page.locator(".chat-needs");
  const context = card.locator(".chat-needs-context");
  assert.match(await context.innerText(), /Splitting them makes reporting lines harder to follow[\s\S]*Eden recommends: one tree/);
  assert.ok(await context.evaluate((el) => Boolean(el.compareDocumentPosition(el.parentElement!.querySelector(".actions")!) & Node.DOCUMENT_POSITION_FOLLOWING)), "Context sits above the answers");
  await card.getByRole("button", { name: "One tree", exact: true }).waitFor();
}
/* Agents' conversations: owner access by the title at 375; DM actions name their destination. */
async function checkA2AHeader(page: Page) {
  await navTo(page, "#/chat/a2a-kai-rowan");
  assert.deepEqual(await page.locator(".readfoot .actions .btn").allInnerTexts(), ["DM Kai", "DM Rowan"]);
  assert.equal(await page.locator(".chat-title .chat-readas-tag").isVisible(), false, "Wide: the context line carries access");
  await page.setViewportSize({ width: 375, height: 667 });
  const tag = page.locator(".chat-title .chat-readas-tag");
  assert.ok(await tag.isVisible(), "Narrow: access label beside the title");
  assert.equal(await tag.innerText(), "Reading as the owner");
  const [title, label] = await Promise.all([page.locator("#ch-title").boundingBox(), tag.boundingBox()]);
  assert.ok(title && label && label.y < title.y + title.height && label.y + label.height > title.y, "The label shares the title's row");
  await page.setViewportSize({ width: 1440, height: 900 });
}
/* Choosing any conversation in the drawer, the open one included, closes it and focuses the heading. */
async function checkDrawerChoice(page: Page) {
  await page.setViewportSize({ width: 375, height: 667 });
  await navTo(page, "#/chat/design");
  for (const pick of ['#chat-nav a[aria-current="page"]', '#chat-nav a[href="#/chat/general"]']) {
    await page.locator(".chat-list-btn").click();
    await page.locator(pick).click();
    await page.locator(".chat.list-open").waitFor({ state: "detached" });
    assert.equal(await page.evaluate(() => document.activeElement?.id), "ch-title", `${pick}: focus on the conversation heading`);
    assert.equal(await page.evaluate(() => document.querySelectorAll("[inert]").length), 0, `${pick}: nothing left inert`);
  }
  await page.setViewportSize({ width: 1440, height: 900 });
}
/* Ada's requests at a constrained width: "1 in this DM", then one "2 more requests" control opening the full list. */
async function checkCompactRequests(page: Page) {
  await page.setViewportSize({ width: 1024, height: 768 });
  await navTo(page, "#/chat/dm-ada");
  const compact = page.locator(".chat-context .chat-more-btn");
  assert.equal(await compact.innerText(), "2 more requests");
  assert.equal(await page.locator(".chat").evaluate((el) => el.classList.contains("ctx-overflow")), false, "The context line fits");
  await compact.click();
  const hrefs = await page.locator("#menu-chat-more-ctx").getByRole("menuitem").evaluateAll((els) => els.map((e) => e.getAttribute("href")));
  assert.equal(hrefs.length, 2);
  assert.ok(hrefs.includes("#/inbox/in4"));
  await page.keyboard.press("Escape");
  await page.setViewportSize({ width: 1440, height: 900 });
}
/* Short screens: a reply-count tap lands on the replies with the ask above; Jump to latest when the newest is below. */
async function checkShortScreens(page: Page) {
  await page.setViewportSize({ width: 375, height: 667 });
  await page.reload();
  await navTo(page, "#/chat/agency-v1");
  await page.locator('.msg[data-mid="m2"] .thr').click();
  const landing = await page.evaluate(() => {
    const s = document.querySelector(".thread .msgs")!, sr = s.getBoundingClientRect();
    const first = s.querySelector(":scope > .chat-thread-count + .msg")!.getBoundingClientRect();
    const sum = s.querySelector(":scope > .chat-thread-sum")!.getBoundingClientRect();
    return { firstReplyVisible: first.top >= sr.top && first.top < sr.bottom - 24, askSummaryAbove: sum.top >= sr.top && sum.bottom <= first.top };
  });
  assert.ok(landing.firstReplyVisible && landing.askSummaryAbove, `A narrow thread shows its replies under a compact summary of the ask (${JSON.stringify(landing)})`);
  assert.match(await page.locator(".thread .chat-thread-sum").innerText(), /Needs you · Approve publishing[\s\S]*3 replies/, "The summary names the ask and counts the replies");
  await page.locator(".thread [data-act=chat-thread-root]").click();
  assert.equal(await page.locator(".thread .chat-thread-root").isVisible(), true, "Show full message opens the ask in place");
  await page.locator(".thread [data-act=thread-close]").click();
  await page.setViewportSize({ width: 420, height: 900 });
  await page.goto(page.url().split("#")[0] + "#/chat/agency-v1");
  await page.waitForTimeout(500);
  const jump = page.locator('[data-act="chat-latest-jump"]');
  const newestBelow = () => page.evaluate(() => { const s = document.querySelector(".chmain .msgs[data-ch]")!; return [...s.querySelectorAll(":scope > .msg")].pop()!.getBoundingClientRect().bottom > s.getBoundingClientRect().bottom + 1; });
  assert.equal(await jump.isVisible(), await newestBelow(), "Jump to latest shows exactly when the newest message is below the fold");
  if (await jump.isVisible()) {
    const [stream, button] = await Promise.all([page.locator(".chmain .msgs[data-ch]").boundingBox(), jump.boundingBox()]);
    assert.ok(stream && button && button.y >= stream.y + stream.height - 1, "On phones Jump to latest is its own row, never over the stream");
  }
  await page.evaluate(() => { document.querySelector(".chmain .msgs[data-ch]")!.scrollTop = 0; });
  await jump.waitFor();
  await jump.click();
  assert.equal(await newestBelow(), false, "Jump to latest shows the newest message");
  assert.equal(await activeAttr(page, "data-mid"), "m4", "Focus moves to the newest message");
  assert.equal(await jump.isVisible(), false);
  await page.setViewportSize({ width: 1440, height: 900 });
}
async function checkChatRound6(page: Page) {
  await checkChatTouch(page);
  await checkChatSourceRepeat(page);
  await checkHomeRouting(page);
  await checkThreadAlsoDraft(page);
  await checkChatSearchThread(page);
  await checkChoiceContext(page);
  await checkA2AHeader(page);
  await checkDrawerChoice(page);
  await checkCompactRequests(page);
  await checkShortScreens(page);
}
/* ---------------------------------------------------------------- task context (TASK-205) */
const savedTaskWithContext = (page: Page) => page.evaluate(() => Object.keys(localStorage)
  .filter((k) => k.startsWith("agentic-org.v6.task."))
  .map((k) => JSON.parse(localStorage.getItem(k)!))
  .find((t) => t.context?.length) ?? null);

/* Chat's Turn into task attaches the message (and its thread) by default; earlier messages are opt-in ("Include 3
   earlier messages", here 3 exist). What is sent is stated above the send actions. Ids, not prose. */
async function checkTaskContextFromChat(page: Page) {
  await navTo(page, "#/chat/agency-v1");
  await page.locator('.msg[data-mid="m4"]').hover();
  await page.locator('.msg[data-mid="m4"] .msg-tool[data-act="chat-task"]').click();
  const dlg = page.locator("#work-task-dialog");
  const card = dlg.locator(".stc-ctx-card");
  assert.equal(await card.count(), 1, "Turn into task attaches one context item by default");
  assert.equal((await card.locator(".stc-ctx-title").innerText()).replace(/\s*\(opens in a new tab\)/, ""), "Conversation excerpt · #agentic-org-v1 · 1 message", "The excerpt is the message itself, not the whole chat");
  assert.equal(await card.locator(".stc-ctx-title").getAttribute("href"), "#/chat/agency-v1/m4", "The excerpt deep-links to the message");
  assert.doesNotMatch(await dlg.locator("#stc-prompt").inputValue(), /#\/chat\//, "The prompt carries no raw chat link");
  assert.equal(await dlg.locator(".stc-ctx-offer").count(), 0, "Chat's default excerpt replaces the page offer");
  assert.equal((await dlg.locator(".stc-sends").innerText()).replace(/\s+/g, " "), "Sends with the task: 1 message", "What is sent shows above the send actions");
  await dlg.getByRole("button", { name: "Include 3 earlier messages" }).click();
  assert.match(await card.locator(".stc-ctx-title").innerText(), /· 4 messages/, "Earlier messages are opt-in");
  assert.equal(await activeAttr(page, "data-stc"), "ctx-more", "Focus moves to the stepper");
  assert.equal(await dlg.getByRole("button", { name: "Include one more earlier message" }).getAttribute("aria-disabled"), "true", "No earlier messages exist past the start");
  await dlg.getByRole("button", { name: "Include one fewer earlier message" }).click();
  assert.match(await card.locator(".stc-ctx-title").innerText(), /· 3 messages/, "Fewer earlier messages shrinks the excerpt");
  assert.equal(await activeAttr(page, "data-stc"), "ctx-fewer", "Focus stays on the stepper");
  assert.match(await dlg.locator(".stc-sends").innerText(), /3 messages/);
  await dlg.getByRole("button", { name: "Show 3 messages" }).click();
  assert.equal(await dlg.locator(".stc-ctx-lines li").count(), 3, "The preview expands to the excerpt's messages");
  await dlg.getByRole("button", { name: /^Remove context: Conversation excerpt · #agentic-org-v1/ }).waitFor();
  await page.evaluate(() => (document.getElementById("work-task-dialog") as HTMLDialogElement).close());
  await dlg.waitFor({ state: "detached" });
}

/* Elsewhere the page is offered, never attached; attaching and creating stores it; the task shows Context links. */
async function checkTaskContextFromPage(page: Page) {
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  await page.keyboard.press("n");
  const dlg = page.locator("#work-task-dialog");
  const offer = dlg.getByRole("button", { name: "Add this page as context: Agentic Org v1.0 · Overview" });
  await offer.waitFor();
  assert.equal(await dlg.locator(".stc-ctx-card").count(), 0, "A project page is offered as context, not attached");
  await offer.click();
  assert.equal(await dlg.locator(".stc-ctx-card").count(), 1, "One click attaches the page");
  assert.equal(await activeAttr(page, "class"), "stc-ctx-title", "Focus moves to the attached item");
  await dlg.getByRole("button", { name: "Add context…" }).click();
  await page.keyboard.type("CSP gates");
  await dlg.locator(".stc-ctx-result", { hasText: "T-A124" }).first().click();
  assert.equal(await dlg.locator(".stc-ctx-card").count(), 2, "Add context… attaches a searched work item");
  await dlg.locator("#stc-prompt").fill("Unblock the CSP gateway permission for the release");
  await dlg.getByRole("button", { name: "Create as-is" }).click();
  await dlg.waitFor({ state: "detached" });
  const saved = await savedTaskWithContext(page);
  assert.ok(saved, "The created task stores its context");
  assert.deepEqual(saved.context.map((c: { kind: string }) => c.kind), ["project", "work"]);
  assert.equal(saved.context[0].href, "#/projects/agency-v1");
  await navTo(page, `#/work?item=${saved.ref}`);
  const section = page.locator("#work-panel .work-ctx");
  await section.waitFor();
  assert.deepEqual(await section.locator(".work-ctx-links > a").evaluateAll((els) => els.map((e) => e.getAttribute("href"))), ["#/projects/agency-v1", "#/work?item=T-A124"], "The task detail links its context");
}

/* A chat excerpt stored on a task keeps its ids and opens the source message, as Inbox does. */
async function checkTaskContextSourceLink(page: Page) {
  await navTo(page, "#/chat/platform");
  await page.locator('.msg[data-mid="p2"]').hover();
  await page.locator('.msg[data-mid="p2"] .msg-tool[data-act="chat-task"]').click();
  const dlg = page.locator("#work-task-dialog");
  await dlg.getByRole("button", { name: "Create as-is" }).click();
  await dlg.waitFor({ state: "detached" });
  const saved = await page.evaluate(() => Object.keys(localStorage).filter((k) => k.startsWith("agentic-org.v6.task."))
    .map((k) => JSON.parse(localStorage.getItem(k)!)).find((t) => t.context?.[0]?.source?.ch === "platform"));
  assert.deepEqual(saved.context[0].source, { ch: "platform", msg: "p2" }, "The excerpt stores conversation and message ids");
  assert.doesNotMatch(saved.description + saved.title, /#\/chat\//, "No raw chat link in the task text");
  await navTo(page, `#/work?item=${saved.ref}`);
  await page.locator("#work-panel").getByRole("link", { name: "Open source message" }).waitFor();
  assert.equal(await page.locator("#work-panel").getByRole("link", { name: "Open source message" }).getAttribute("href"), "#/chat/platform/p2");
}
/* Attachments (TASK-206): picker, tray, remove, size limit, attachment-only send, image viewer, Files panel, search. */
const PNG_1PX = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==", "base64");
const mainComposer = (page: Page) => page.locator('#main .chat form[data-form="send"]');
const trayItems = (page: Page) => mainComposer(page).locator(".chat-att-item");
async function waitUploads(page: Page) {
  await page.waitForFunction(() => document.querySelectorAll("#main .chat .chat-att-item").length > 0 && !document.querySelector("#main .chat .chat-att-item.is-uploading"));
}
async function checkAttachTray(page: Page) {
  await page.getByRole("button", { name: /^Attach files, up to 25 MB each$/ }).waitFor();
  await mainComposer(page).locator(".chat-att-input").setInputFiles([
    { name: "board.png", mimeType: "image/png", buffer: PNG_1PX },
    { name: "notes.pdf", mimeType: "application/pdf", buffer: Buffer.from("%PDF-1.4 test") },
    { name: "extra.txt", mimeType: "text/plain", buffer: Buffer.from("extra") },
  ]);
  assert.equal(await trayItems(page).count(), 3, "Picked files show in the tray");
  assert.equal(await trayItems(page).locator("img").count(), 1, "An image shows a thumbnail");
  assert.match(await trayItems(page).nth(1).innerText(), /notes\.pdf[\s\S]*PDF/, "A file shows its name and type");
  await page.locator(".chat-att-item.is-uploading").first().waitFor();
  await waitUploads(page);
  await page.getByRole("button", { name: "Remove extra.txt" }).click();
  assert.equal(await trayItems(page).count(), 2, "Remove takes the file out of the tray");
  assert.equal(await page.evaluate(() => document.activeElement?.closest("form")?.dataset.form), "send", "Focus stays in the composer after Remove");
}
async function checkAttachLimit(page: Page) {
  /* Built in the page: sending 26 MB through the driver is slow. */
  await page.evaluate(() => {
    const input = document.querySelector<HTMLInputElement>('#main .chat form[data-form="send"] .chat-att-input')!, dt = new DataTransfer();
    dt.items.add(new File([new Uint8Array(26 * 1024 * 1024)], "huge.zip", { type: "application/zip" }));
    input.files = dt.files;
    input.dispatchEvent(new Event("change", { bubbles: true }));
  });
  assert.match(await mainComposer(page).locator(".chat-att-error").innerText(), /huge\.zip.*26\.0 MB.*up to 25 MB/, "An oversize file is rejected with the limit");
  assert.equal(await trayItems(page).count(), 2, "The rejected file is not attached");
}
async function checkAttachSend(page: Page) {
  await mainComposer(page).getByRole("button", { name: "Send" }).click();
  const sent = page.locator("#main .chat .chmain .msgs > .msg").last();
  await sent.locator(".chat-att-img img").waitFor();
  await page.waitForFunction(() => !document.querySelector("#main .chat .msg.is-pending"));
  assert.equal(await sent.locator(".chat-att-file .chat-att-name").innerText(), "notes.pdf", "A sent file renders as a card");
  assert.equal(await sent.locator(".tx").count(), 0, "A message can carry only attachments");
  assert.equal(await trayItems(page).count(), 0, "Sending clears the tray");
  await mainComposer(page).getByRole("button", { name: "Send" }).click();
  assert.match(await mainComposer(page).locator(".chat-compose-hint").innerText(), /Write a message or attach a file first/);
}
async function checkAttachViewer(page: Page) {
  const tile = page.locator('.msg[data-mid="m1"] .chat-att-img').first();
  await tile.click();
  const viewer = page.getByRole("dialog", { name: "ia-v2-nav-before.png" });
  await viewer.waitFor();
  assert.ok(await viewer.evaluate((d) => d.contains(document.activeElement)), "Focus moves into the viewer");
  assert.equal(await viewer.getByRole("link", { name: "Download" }).getAttribute("download"), "ia-v2-nav-before.png");
  await page.keyboard.press("ArrowRight");
  await page.getByRole("dialog", { name: "ia-v2-nav-after.png" }).waitFor();
  await page.keyboard.press("Tab");
  await page.keyboard.press("Tab");
  await page.keyboard.press("Tab");
  assert.ok(await page.evaluate(() => Boolean(document.activeElement?.closest("dialog.chat-lightbox"))), "Focus stays in the viewer");
  await page.keyboard.press("Escape");
  assert.equal(await page.locator("dialog.chat-lightbox").count(), 0, "Esc closes the viewer");
  assert.equal(await activeAttr(page, "data-act"), "chat-att-view", "Focus returns to the image");
  assert.equal(await activeAttr(page, "data-id"), "m1");
}
async function checkAttachDropAndPaste(page: Page) {
  await page.evaluate(() => {
    const dt = new DataTransfer();
    dt.items.add(new File(["drop"], "dropped.log", { type: "text/plain" }));
    const zone = document.querySelector("#main .chat .chmain .msgs")!;
    zone.dispatchEvent(new DragEvent("dragenter", { bubbles: true, dataTransfer: dt }));
    zone.dispatchEvent(new DragEvent("dragover", { bubbles: true, cancelable: true, dataTransfer: dt }));
  });
  assert.match(await page.locator("#main .chat .chmain > .chat-drop").innerText(), /Drop to attach/, "Dragging files shows the drop target");
  await page.evaluate(() => {
    const dt = new DataTransfer();
    dt.items.add(new File(["drop"], "dropped.log", { type: "text/plain" }));
    document.querySelector("#main .chat .chmain .msgs")!.dispatchEvent(new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: dt }));
    const paste = new DataTransfer();
    paste.items.add(new File([new Uint8Array([137, 80, 78, 71])], "pasted.png", { type: "image/png" }));
    document.getElementById("compose")!.dispatchEvent(new ClipboardEvent("paste", { bubbles: true, cancelable: true, clipboardData: paste }));
  });
  assert.equal(await page.locator("#main .chat .chat-drop").count(), 0, "Drop clears the target");
  assert.deepEqual(await trayItems(page).locator(".chat-att-name").allInnerTexts(), ["dropped.log", "pasted.png"], "Drop and paste attach");
  await page.getByRole("button", { name: "Remove dropped.log" }).click();
  await page.getByRole("button", { name: "Remove pasted.png" }).click();
}
async function checkAttachFilesAndSearch(page: Page) {
  await page.locator(".chat-files-btn").click();
  const panel = page.locator("#chat-files");
  await panel.waitFor();
  assert.ok((await panel.locator(".chat-file-row").count()) >= 4, "Files lists the conversation's attachments");
  assert.match(await panel.locator(".chat-file-row").last().innerText(), /Ada · Today 09:12/, "Each file says who sent it and when");
  await panel.getByRole("link", { name: "Open source message for ia-v2-nav-before.png", exact: true }).click();
  await page.locator('.msg.is-linked[data-mid="m1"]').waitFor();
  assert.equal(await page.locator("#chat-files").count(), 0, "Following a file closes Files");
  await navTo(page, "#/chat/platform");
  await page.locator(".chat-search").click();
  await page.locator("#chat-search-input").fill("lease-race");
  assert.equal(await page.locator("#chat-search-status").innerText(), "1 post", "Search finds an attachment name");
  assert.equal(await page.locator('.msgs > .msg[data-mid="p1"] .chat-att-name mark').count(), 1, "The matching name is marked");
  await page.locator("#chat-search-input").press("Escape");
}
async function checkAttachThread(page: Page) {
  await navTo(page, "#/chat/agency-v1");
  await page.locator('.msg[data-mid="m2"] .thr').click();
  const form = page.locator('aside.thread form[data-form="thread-send"]');
  await form.locator(".chat-att-input").setInputFiles([{ name: "review.diff", mimeType: "text/x-diff", buffer: Buffer.from("+ok") }]);
  await waitUploads(page);
  await form.locator("#thread-compose").press("Control+Enter");
  await page.locator('aside.thread .msgs > .msg:last-child .chat-att-file .chat-att-name', { hasText: "review.diff" }).waitFor();
  assert.equal(await form.locator(".chat-att-item").count(), 0, "Ctrl+Enter sends a thread reply with only an attachment");
  await page.locator("aside.thread [data-act=thread-close]").click();
}
async function checkAttachReadOnly(page: Page) {
  await navTo(page, "#/chat/a2a-kai-rowan");
  await page.locator('.msg[data-mid="k1"] .chat-att-file').waitFor();
  assert.equal(await page.locator(".chat-attach-btn").count(), 0, "Agents' conversations have no Attach");
}
async function checkChatAttachments(page: Page) {
  const errors: string[] = [];
  page.on("console", (msg) => { if (msg.type() === "error") errors.push(msg.text()); });
  await page.setViewportSize({ width: 1440, height: 900 });
  await navTo(page, "#/chat/agency-v1");
  assert.equal(await page.locator('.msg[data-mid="m1"] .chat-att-img').count(), 2, "Seeded screenshots show");
  await checkAttachTray(page);
  await checkAttachLimit(page);
  await checkAttachSend(page);
  await checkAttachViewer(page);
  await checkAttachDropAndPaste(page);
  await checkAttachFilesAndSearch(page);
  await checkAttachThread(page);
  await checkFilesExactReply(page);
  await checkAttachReadOnly(page);
  assert.deepEqual(errors, [], "No console errors while attaching");
}
/* ---------------------------------------------------------------- Chat round 7 (TASK-195) */
const viewport = (page: Page, width: number, height: number) => page.setViewportSize({ width, height });
/* A low-risk follow-up's chip goes back to the answerable card (same conversation too) and focuses it. */
async function checkFollowUpChip(page: Page) {
  await navTo(page, "#/chat/agency-v1");
  await page.locator('.chat-needs:has(#needs-m3) [data-act="chat-ask-reply"]').click();
  await page.keyboard.type("Is the sandbox wiped after the run?");
  await page.keyboard.press("Control+Enter");
  const chip = page.locator(".chmain .msgs > .msg").last().locator(".chat-needs-chip a");
  assert.equal(await chip.innerText(), "Needs you · Approval on the card above", "The follow-up's chip names the card, not Inbox");
  assert.equal(await chip.getAttribute("href"), "#/chat/agency-v1/m3");
  await chip.click();
  await page.locator('.msg.is-linked[data-mid="m3"]').waitFor();
  assert.equal(await activeAttr(page, "id"), "needs-m3", "Arriving focuses the card");
  assert.equal(await page.locator('.chat-needs:has(#needs-m3) a[href="#/inbox/in5"]').count(), 1, "The card keeps its Inbox record link");
  await page.locator('.msg[data-mid="m3"]').hover();
  await page.locator('.msg[data-mid="m3"] .msg-tool[data-act="thread"]').click();
  await page.locator('.thread [data-act="chat-ask-reply"]').click();
  await page.keyboard.type("And the logs?");
  await page.keyboard.press("Control+Enter");
  const inThread = page.locator(".thread .msgs > .msg").last().locator(".chat-needs-chip button");
  assert.equal(await inThread.innerText(), "Needs you · Approval on the card above");
  await inThread.click();
  assert.ok(await page.evaluate(() => Boolean(document.activeElement?.closest(".thread .chat-needs"))), "In a thread the chip focuses the card at its top");
  await page.locator(".thread [data-act=thread-close]").click();
}
/* Thread replies say "Sending…" like main sends, the channel copy too; focus stays in the thread composer. */
async function checkThreadPending(page: Page) {
  await page.locator('.msg[data-mid="m2"] .thr').click();
  await page.locator(".thread .chat-also input").check();
  await page.locator("#thread-compose").fill("Looks good from here.");
  await page.locator("#thread-compose").press("Control+Enter");
  await page.locator(".thread .msgs > .msg.is-pending .msg-status", { hasText: "Sending…" }).waitFor();
  await page.locator(".chmain .msgs > .msg.is-pending .msg-status", { hasText: "Sending…" }).waitFor();
  await page.waitForFunction(() => !document.querySelector("#main .chat .msg.is-pending"));
  assert.equal(await activeAttr(page, "id"), "thread-compose", "Focus stays in the thread composer");
  await page.locator(".thread [data-act=thread-close]").click();
}
/* The drawer is used below 768 px of chat content; widening past it closes the drawer cleanly. */
async function checkListThreshold(page: Page) {
  const btnShown = () => page.locator(".chat-list-btn").isVisible();
  await viewport(page, 800, 800); /* 800 - the app nav's rail = under 768 px of content */
  await navTo(page, "#/chat/agency-v1");
  await page.waitForFunction(() => Boolean(document.querySelector("#main .chat.list-overlay")));
  assert.ok(await page.evaluate(() => document.getElementById("main")!.clientWidth < 768), "The test width gives under 768 px of content");
  assert.ok(await btnShown(), "Content under 768 px: the Conversations button shows");
  assert.equal(await page.locator(".chlist").isVisible(), false, "and the list is an overlay");
  await viewport(page, 900, 800); /* 900 - the rail = over 768 px of content */
  await page.waitForFunction(() => !document.querySelector("#main .chat.list-overlay"));
  assert.equal(await btnShown(), false, "Content of 768 px or more: the list is inline, no button");
  await viewport(page, 800, 800);
  await page.waitForFunction(() => Boolean(document.querySelector("#main .chat.list-overlay")));
  await page.locator(".chat-list-btn").click();
  await page.locator(".chat.list-open").waitFor();
  assert.ok(await page.evaluate(() => Boolean(history.state?.chatOverlay)), "The drawer adds a history entry");
  await viewport(page, 1440, 900);
  await page.locator(".chat.list-open").waitFor({ state: "detached" });
  await page.waitForFunction(() => !history.state?.chatOverlay);
  assert.equal(await page.evaluate(() => document.querySelectorAll("[inert]").length), 0, "Nothing left inert");
  assert.equal(await page.evaluate(() => document.activeElement?.matches('.chlist a[aria-current="page"]')), true, "Focus on the selected inline row");
  await page.locator("#compose").click();
  await page.keyboard.type("Still typing");
  assert.equal(await page.locator("#compose").inputValue(), "Still typing", "The conversation takes typing again");
  await page.locator("#compose").fill("");
}
/* Opening an older message reads only what came into view; the rest stays unread. */
async function checkReadMarker(page: Page) {
  const designRow = page.locator('#chat-nav a[href="#/chat/design"]');
  await viewport(page, 1440, 500);
  await navTo(page, "#/chat/general");
  assert.match(await designRow.innerText(), /3 unread messages/);
  await navTo(page, "#/chat/design/d1");
  await page.locator('.msg.is-linked[data-mid="d1"]').waitFor();
  assert.equal(await page.locator(".chat-new-rule + .msg").getAttribute("data-mid"), "d3", "New messages starts at the first unread");
  await page.waitForTimeout(300);
  await navTo(page, "#/chat/general");
  assert.ok(await designRow.evaluate((a) => a.classList.contains("is-unread")), "Messages below the fold stay unread");
  await navTo(page, "#/chat/design");
  await page.locator(".chmain .msgs[data-ch=design]").evaluate((s) => { s.scrollTop = s.scrollHeight; });
  await page.waitForTimeout(300);
  await navTo(page, "#/chat/general");
  assert.equal(await designRow.evaluate((a) => a.classList.contains("is-unread")), false, "Reading to the end clears unread");
}
/* At 1024 the opening position cuts into Tomas's ask; its author line stays in view. */
async function checkCutAuthor(page: Page) {
  await viewport(page, 1024, 768);
  await page.reload();
  await navTo(page, "#/chat/agency-v1");
  await page.waitForTimeout(400);
  const at = await page.evaluate(() => {
    const s = document.querySelector(".chmain .msgs[data-ch]")!, band = s.querySelector(".chat-day")!.getBoundingClientRect().height;
    const line = s.getBoundingClientRect().top + band;
    const top = [...s.querySelectorAll(":scope > .msg:not(.cont)")].find((m) => m.getBoundingClientRect().bottom > line + 32)!;
    const hd = top.querySelector(".hd")!.getBoundingClientRect();
    return { mid: top.getAttribute("data-mid"), cut: top.getBoundingClientRect().top < line - 1, hdTop: hd.top - line };
  });
  assert.ok(!at.cut || at.hdTop >= -1, `The top message's author line is visible (${JSON.stringify(at)})`);
  await viewport(page, 1440, 900);
}
/* Mentions come from recipient data, including thread replies, and never from prose. */
async function checkMentions(page: Page) {
  await navTo(page, "#/chat/mentions");
  const text = await page.locator(".chat-view-list").innerText();
  assert.doesNotMatch(text, /I asked the owner/, "Prose about the owner is not a mention");
  const reply = page.locator(".chat-view-item", { hasText: "Approved on my side" });
  assert.match(await reply.innerText(), /in a thread on Tomas's message/);
  await reply.getByRole("button", { name: "Open in thread" }).click();
  const landed = page.locator(".thread .msgs > .msg.is-linked", { hasText: "Approved on my side" });
  await landed.waitFor();
  assert.ok(await landed.evaluate((el) => el === document.activeElement), "Focus lands on the mentioning reply");
  await page.locator(".thread [data-act=thread-close]").click();
}
/* A context line that would be cut shows a plain summary; its links move to Details, out of the tab order. */
async function checkHeaderSummary(page: Page) {
  await viewport(page, 700, 800);
  await navTo(page, "#/chat/agency-v1");
  await page.locator(".chat.ctx-overflow").waitFor();
  assert.equal(await page.locator(".chat-ctx-summary").innerText(), "Shipping the first release · 8 members");
  assert.equal(await page.locator(".chat-ctx-full").evaluate((el) => getComputedStyle(el).visibility), "hidden", "The clipped links leave the tab order");
  await page.locator(".chat-details-btn").click();
  await page.locator('.chat-details a[href="#/projects/agency-v1"]').waitFor();
  await page.locator(".chat-details-btn").click();
  await viewport(page, 1440, 900);
  await page.waitForFunction(() => !document.querySelector("#main .chat.ctx-overflow"));
  assert.ok(await page.locator(".chat-ctx-full .chat-members-btn").isVisible(), "Wide: the full line with its links");
}
/* Agents' conversations name "Message Rowan about this" on desktop; the DM opens with its composer focused. */
async function checkA2AMessageAction(page: Page) {
  await navTo(page, "#/chat/a2a-kai-rowan");
  await page.locator('.msg[data-mid="k2"]').hover();
  const action = page.locator('.msg[data-mid="k2"] .msg-tool-text');
  assert.equal(await action.innerText(), "Message Rowan about this");
  await action.click();
  await page.waitForURL((u) => u.hash === "#/chat/dm-rowan");
  await page.waitForFunction(() => document.activeElement?.id === "compose");
}
/* The cross-post choice is a 44 px target at phone width. */
async function checkAlsoTarget(page: Page) {
  await viewport(page, 375, 667);
  await navTo(page, "#/chat/agency-v1");
  await page.locator('.msg[data-mid="m2"] .thr').click();
  const box = await page.locator(".thread .chat-also").boundingBox();
  assert.ok(box && box.height >= 44, `Also send to is 44 px tall (${box?.height})`);
  await page.locator(".thread [data-act=thread-close]").click();
  await viewport(page, 1440, 900);
}
/* The side panel is about a third of the chat area at 1440, resizable from its edge by keyboard, and remembered. */
async function checkPanelWidth(page: Page) {
  const width = () => page.locator("aside.thread").evaluate((el) => Math.round(el.getBoundingClientRect().width));
  await viewport(page, 1440, 900);
  await navTo(page, "#/chat/agency-v1");
  await page.locator('.msg[data-mid="m2"] .thr').click();
  const start = await width();
  assert.ok(start >= 400 && start <= 480, `The thread panel is 400-480 px at 1440 (${start})`);
  const sep = page.locator('aside.thread [role="separator"]');
  assert.equal(await sep.getAttribute("aria-valuenow"), String(start));
  await sep.focus();
  await page.keyboard.press("ArrowLeft");
  await page.keyboard.press("ArrowLeft");
  assert.equal(await width(), start + 32, "Arrow keys widen it from its left edge");
  assert.equal(await sep.getAttribute("aria-valuenow"), String(start + 32));
  await page.reload();
  await page.locator('.msg[data-mid="m2"] .thr').click();
  assert.equal(await width(), start + 32, "The width is remembered on this device");
  await page.locator(".thread [data-act=thread-close]").click();
  await viewport(page, 1024, 768);
  await page.locator('.msg[data-mid="m2"] .thr').click();
  assert.equal(await page.locator('aside.thread[role="dialog"]').count(), 1, "Below 1280 it is a modal");
  assert.equal(await page.locator('aside.thread [role="separator"]').count(), 0, "with no resize edge");
  await page.locator(".thread [data-act=thread-close]").click();
  await page.evaluate(() => localStorage.removeItem("agentic-org.v6.chat.panelWidth"));
  await viewport(page, 1440, 900);
}
async function checkChatRound7(page: Page) {
  await checkFollowUpChip(page);
  await checkThreadPending(page);
  await checkListThreshold(page);
  await checkReadMarker(page);
  await checkCutAuthor(page);
  await checkMentions(page);
  await checkHeaderSummary(page);
  await checkA2AMessageAction(page);
  await checkAlsoTarget(page);
  await checkPanelWidth(page);
}
/* ---------------------------------------------------------------- agents' conversations at scale */
const a2aRows = (page: Page) => page.locator('#chat-nav a[href^="#/chat/a2a-"]').evaluateAll((els) => els.map((e) => e.getAttribute("href")!.slice(7)));
async function checkA2ARecent(page: Page) {
  await viewport(page, 1440, 900);
  await navTo(page, "#/chat/general");
  assert.deepEqual(await a2aRows(page), ["a2a-ada-tomas", "a2a-tomas-sage", "a2a-emery-eden", "a2a-jules-avery"], "The last day's, newest first");
  assert.equal(await page.locator("#chat-a2a-head .chat-a2a-n").innerText(), "4", "The heading counts the rows");
  const all = page.locator("#chat-a2a-all");
  assert.equal(await all.innerText(), "All dates");
  await all.click();
  assert.equal((await a2aRows(page)).length, 8, "All dates lists every agents' conversation");
  assert.equal(await page.locator("#chat-a2a-all").getAttribute("aria-pressed"), "true");
  assert.equal(await page.locator("#chat-a2a-head .chat-a2a-n").innerText(), "8");
  assert.equal(await activeAttr(page, "id"), "chat-a2a-all", "Focus stays on the control");
  await page.locator("#chat-a2a-day").focus();
  await page.keyboard.press("Enter");
  assert.equal((await a2aRows(page)).length, 4, "and the keyboard returns to the last 24 hours");
}
/* Picked names, the picker and every matching conversation lie inside the list's visible area. */
const a2aInView = (page: Page) => page.evaluate(() => {
  const nav = document.getElementById("chat-nav")!.getBoundingClientRect();
  const els = [...document.querySelectorAll('#chat-a2a-filter, #chat-nav a[href^="#/chat/a2a-"]')];
  return els.length > 1 && els.every((e) => { const r = e.getBoundingClientRect(); return r.top >= nav.top - 1 && r.bottom <= nav.bottom + 1; });
});
async function checkA2AFilter(page: Page) {
  assert.equal(await page.locator("#chat-a2a-day").getAttribute("aria-pressed"), "true", "The default date scope is named");
  await page.locator("#chat-a2a-filter-btn").click();
  const field = page.locator("#chat-a2a-q");
  assert.equal(await activeAttr(page, "id"), "chat-a2a-q", "Opening the filter focuses the agent search");
  assert.equal(await field.getAttribute("role"), "combobox");
  assert.equal(await page.locator("#chat-a2a-options [role=option]").count(), 0, "No wall of agents: options list only while searching");
  await page.keyboard.type("ro");
  assert.equal(await field.getAttribute("aria-expanded"), "true");
  assert.deepEqual(await page.locator("#chat-a2a-options [role=option]").allInnerTexts(), ["Rowan"], "Search narrows the agents");
  await page.keyboard.press("Enter");
  assert.deepEqual(await a2aRows(page), ["a2a-kai-rowan", "a2a-rowan-sage", "a2a-ada-rowan"], "One agent: every conversation it is in");
  assert.equal(await page.locator("#chat-a2a-all").getAttribute("aria-pressed"), "true", "Picking an agent shows all dates, visibly");
  assert.deepEqual(await page.locator(".chat-a2a-token").allInnerTexts(), ["Rowan"], "The picked agent shows by name");
  assert.equal(await activeAttr(page, "id"), "chat-a2a-q", "Focus stays in the picker");
  assert.ok(await a2aInView(page), "At 1440×900 the picked name and its conversations are in view without scrolling");
  await page.keyboard.press("ArrowDown");
  const partners = await page.locator("#chat-a2a-options [role=option]").allInnerTexts();
  assert.deepEqual(partners, ["Ada", "Kai", "Sage"], "With one agent picked, only its partners are offered");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  assert.deepEqual(await a2aRows(page), ["a2a-kai-rowan"], "Two agents: that pair");
  assert.equal(await page.locator("#chat-a2a-q").count(), 0, "Two is the most");
  assert.equal(await activeAttr(page, "id"), "chat-a2a-rm-kai", "Focus moves to the last name's Remove");
  assert.ok(await a2aInView(page), "The pair's conversation stays in view");
  await page.keyboard.press("Enter");
  assert.deepEqual(await page.locator(".chat-a2a-token").allInnerTexts(), ["Rowan"], "Remove takes one name out");
  assert.equal(await activeAttr(page, "id"), "chat-a2a-q");
  await page.keyboard.press("Backspace");
  assert.equal(await page.locator(".chat-a2a-token").count(), 0, "Backspace in the empty field removes the last name");
  assert.equal((await a2aRows(page)).length, 4);
  await page.keyboard.press("Escape");
  assert.equal(await page.locator("#chat-a2a-filter").count(), 0, "Esc closes the picker");
  assert.equal(await activeAttr(page, "id"), "chat-a2a-filter-btn");
}
async function checkA2AFind(page: Page) {
  const why = (href: string) => page.locator(`#chat-nav a[href="${href}"] .chat-row-why`).innerText();
  await page.locator("#chat-find").fill("T-A121");
  assert.match(await why("#/chat/a2a-ada-tomas"), /mentions T-A121/, "A task id finds agents' conversations, saying why");
  await page.locator("#chat-find").fill("look & feel");
  assert.match(await why("#/chat/design"), /in Look & feel refresh/, "A project name finds its conversations");
  await page.locator("#chat-find").fill("soak");
  assert.match(await why("#/chat/a2a-kai-quinn"), /soak/, "Message words find agents' conversations");
  await page.locator("#chat-find").fill("");
}
async function checkA2ADrawerTargets(page: Page) {
  await viewport(page, 375, 667);
  await page.locator(".chat-list-btn").click();
  await page.locator("#chat-a2a-filter-btn").click();
  await page.keyboard.type("a");
  const heights = await page.locator(".chlist :is(#chat-a2a-q, #chat-a2a-options [role=option], .chat-a2a-filter-btn, .chat-seg-btn)").evaluateAll((els) => els.map((e) => e.getBoundingClientRect().height));
  assert.ok(heights.length > 2 && heights.every((h) => h >= 44), `Agent filter targets are 44 px in the drawer (${heights})`);
  await page.keyboard.press("Escape");
  await page.locator("#chat-a2a-filter-btn").click();
  await page.keyboard.press("Escape");
  await page.locator(".chat.list-open").waitFor({ state: "detached" });
  await page.waitForFunction(() => !history.state?.chatOverlay);
  await viewport(page, 1440, 900);
}
async function checkAgentProfileA2A(page: Page) {
  await navTo(page, "#/members/rowan");
  const rows = page.locator(".c-a2a .c-a2a-row");
  await rows.first().waitFor();
  assert.deepEqual(await rows.evaluateAll((els) => els.map((e) => e.getAttribute("href"))), ["#/chat/a2a-kai-rowan", "#/chat/a2a-rowan-sage", "#/chat/a2a-ada-rowan"]);
  await rows.first().click();
  await page.waitForURL((u) => u.hash === "#/chat/a2a-kai-rowan");
}
async function checkA2AScale(page: Page) {
  await checkA2ARecent(page);
  await checkA2AFilter(page);
  await checkA2AFind(page);
  await checkA2ADrawerTargets(page);
  await checkAgentProfileA2A(page);
}
/* ---------------------------------------------------------------- composer: @ mentions and / commands */
const assistOptions = (page: Page, field: string) => page.locator(`#${field}-assist [role="option"] .chat-assist-label`).allInnerTexts();
async function checkMentionAutocomplete(page: Page) {
  await navTo(page, "#/chat/agency-v1");
  const field = page.locator("#compose");
  await field.click();
  await page.keyboard.type("@");
  assert.equal((await assistOptions(page, "compose"))[0], "@here", "@here is offered in a channel");
  assert.equal(await field.getAttribute("aria-activedescendant"), "compose-assist-0");
  await page.keyboard.press("ArrowDown");
  assert.equal(await field.getAttribute("aria-activedescendant"), "compose-assist-1", "Arrow keys move the highlight");
  assert.equal(await page.locator("#compose-assist-1").getAttribute("aria-selected"), "true");
  await page.keyboard.press("Escape");
  assert.equal(await page.locator("#compose-assist").count(), 0, "Esc closes the list");
  assert.equal(await field.getAttribute("aria-expanded"), "false");
  await field.fill("");
  await page.keyboard.type("Thanks @ka");
  assert.equal((await assistOptions(page, "compose"))[0], "Kai", "Filtered by display name");
  assert.match(await page.locator("#compose-assist-0 .chat-assist-hint").innerText(), /in this conversation/, "This conversation's members first");
  await page.keyboard.press("Enter");
  assert.equal(await field.inputValue(), "Thanks @kai ", "Enter completes to the handle");
  await page.keyboard.type("for the fix");
  await page.keyboard.press("Control+Enter");
  await page.locator('.chmain .msgs > .msg:last-child .tx a[href="#/members/kai"]').waitFor();
  await page.locator('.msg[data-mid="m2"] .thr').click();
  await page.locator("#thread-compose").click();
  await page.keyboard.type("cc @ad");
  await page.keyboard.press("Enter");
  assert.equal(await page.locator("#thread-compose").inputValue(), "cc @ada ", "Enter completes in the thread composer too");
  await page.keyboard.type("and @ka");
  await page.keyboard.press("Tab");
  assert.equal(await page.locator("#thread-compose").inputValue(), "cc @ada and @ka", "Tab never inserts");
  assert.equal(await page.locator("#thread-compose-assist").count(), 0, "Tab closes the list");
  assert.notEqual(await activeAttr(page, "id"), "thread-compose", "and moves focus on as usual");
  await page.locator("#thread-compose").fill("");
  await page.locator(".thread [data-act=thread-close]").click();
}
async function checkSlashCommands(page: Page) {
  const field = page.locator("#compose");
  await field.click();
  await page.keyboard.type("/");
  assert.deepEqual(await assistOptions(page, "compose"), ["/task", "/ask", "/assign", "/files"]);
  assert.ok((await page.locator("#compose-assist-0 .chat-assist-hint").innerText()).length > 10, "Each command says what it does");
  await page.keyboard.type("fi");
  await page.keyboard.press("Enter");
  await page.locator("#chat-files").waitFor();
  await page.locator('#chat-files [data-act="chat-files"]').click();
  await field.click();
  await page.keyboard.type("/ta");
  await page.keyboard.press("Enter");
  const dlg = page.locator("#work-task-dialog");
  await dlg.waitFor();
  assert.match(await dlg.locator(".stc-ctx-title").innerText(), /Conversation excerpt · #agentic-org-v1/, "/task attaches the conversation excerpt");
  await page.keyboard.press("Escape");
  await dlg.waitFor({ state: "detached" });
  await field.fill("");
  await field.click();
  await page.keyboard.type("/as");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  assert.equal(await field.inputValue(), "/assign @");
  assert.ok((await assistOptions(page, "compose")).every((n) => !["Maya Chen", "@here"].includes(n)), "Assign offers agents only");
  await page.keyboard.type("aver");
  await page.keyboard.press("Enter");
  await page.keyboard.type("T-A84");
  await page.keyboard.press("Control+Enter");
  const confirm = page.locator('form[data-form="send"] .chat-assign-ask');
  await confirm.waitFor();
  assert.match(await confirm.innerText(), /Assign T-A84 .+ to Avery\?/, "/assign names the change before making it");
  assert.equal(await field.inputValue(), "/assign @avery T-A84", "and nothing has changed yet");
  assert.equal(await activeAttr(page, "data-act"), "chat-assign-confirm", "Focus moves to Assign");
  await page.keyboard.press("Enter");
  await page.locator(".toast", { hasText: "Assigned T-A84 to Avery." }).waitFor();
  assert.equal(await field.inputValue(), "");
  assert.equal(await activeAttr(page, "id"), "compose");
  await page.keyboard.type("/ask @row");
  await page.keyboard.press("Enter");
  await page.keyboard.type("Can the flag go in v1.1?");
  await page.keyboard.press("Control+Enter");
  assert.match(await page.locator(".chmain .msgs > .msg:last-child .chat-ask-line").innerText(), /Question for Rowan/, "/ask posts a structured question");
  await page.keyboard.type("/nope");
  await page.keyboard.press("Control+Enter");
  assert.match(await page.locator('form[data-form="send"] .chat-compose-hint').innerText(), /\/nope isn't a command/);
  await field.fill("");
}
async function checkComposerAssists(page: Page) {
  await viewport(page, 1440, 900);
  await checkMentionAutocomplete(page);
  await checkSlashCommands(page);
}
/* ---------------------------------------------------------------- Chat round 8 (TASK-195) */
const lastMessageText = (page: Page) => page.locator("#main .chat .chmain .msgs > .msg").last().locator(".tx").innerText();
/* Only a complete "/word" token is a command: file names and paths are prose and send as written. */
async function checkSlashProse(page: Page) {
  await viewport(page, 1440, 900);
  await navTo(page, "#/chat/general");
  const field = page.locator("#compose");
  for (const text of ["/files.txt is attached", "/etc/hosts is missing"]) {
    await field.fill(text);
    await field.press("Control+Enter");
    assert.match(await lastMessageText(page), new RegExp(text.replace(/[./]/g, "\\$&")), `"${text}" sends as a message`);
    assert.equal(await field.inputValue(), "");
    assert.equal(await page.locator("#chat-files").count(), 0, "and opens nothing");
  }
}
/* "/task what to do" carries the words into the task prompt, keeps the excerpt, and leaves the composer only once
   the task exists; cancelling keeps them. */
async function checkSlashTaskText(page: Page) {
  const field = page.locator("#compose"), dlg = page.locator("#work-task-dialog");
  await field.fill("/task Finish the rollout");
  await field.press("Control+Enter");
  await dlg.waitFor();
  assert.equal(await dlg.locator("#stc-prompt").inputValue(), "Finish the rollout", "The words become the task prompt");
  assert.match(await dlg.locator(".stc-ctx-title").innerText(), /Conversation excerpt · #general/, "with the excerpt attached");
  await page.evaluate(() => (document.getElementById("work-task-dialog") as HTMLDialogElement).close());
  await dlg.waitFor({ state: "detached" });
  assert.equal(await field.inputValue(), "/task Finish the rollout", "Cancelling keeps the text");
  await field.press("Control+Enter");
  await dlg.getByRole("button", { name: "Create as-is" }).click();
  await dlg.waitFor({ state: "detached" });
  assert.equal(await page.locator("#compose").inputValue(), "", "Creating the task clears it");
}
/* An unknown command never loses the text and offers to send it as written. */
async function checkUnknownCommand(page: Page) {
  const field = page.locator("#compose");
  await field.fill("/shrug at the build");
  await field.press("Control+Enter");
  const hint = page.locator('form[data-form="send"] .chat-compose-hint');
  assert.match(await hint.innerText(), /\/shrug isn't a command/);
  assert.equal(await field.inputValue(), "/shrug at the build", "The text stays");
  await hint.getByRole("button", { name: "Send as message" }).click();
  assert.match(await lastMessageText(page), /^\/shrug at the build/, "Send as message posts it as written");
  assert.equal(await field.inputValue(), "");
}
/* Choosing a command from the list writes it in front of words already in the field, never over them. */
async function checkCommandKeepsText(page: Page) {
  const field = page.locator("#compose");
  await field.fill(" the rollout");
  await field.press("Home");
  await page.keyboard.type("/ta");
  await page.keyboard.press("Enter");
  assert.equal(await field.inputValue(), "/task the rollout", "The words stay after the command");
  assert.equal(await page.locator("#work-task-dialog").count(), 0, "and nothing runs until sent");
  await field.fill("");
}
/* Files leads to the exact reply a file was sent in: the thread opens on it, marked and focused. */
async function checkFilesExactReply(page: Page) {
  await viewport(page, 1440, 900);
  await navTo(page, "#/chat/agency-v1");
  await page.locator(".chat-files-btn").click();
  const link = page.locator("#chat-files").getByRole("link", { name: "Open source message for review.diff", exact: true });
  const href = await link.getAttribute("href");
  assert.match(href!, /^#\/chat\/agency-v1\/m2-r\d+$/, "A reply's file links to that reply");
  await link.click();
  await page.waitForURL((u) => u.hash === href);
  const mid = href!.split("/").pop()!;
  const reply = page.locator(`aside.thread .msgs > .msg[data-mid="${mid}"]`);
  await reply.waitFor();
  /* The mark can be brief (a re-render redraws the thread), so wait, every frame, for the landing itself: the reply
     taking focus, which stays. */
  await page.waitForFunction((m) => document.activeElement?.matches(`aside.thread .msgs > .msg[data-mid="${m}"]`) ?? false, mid, { polling: "raf" });
  assert.match(await reply.innerText(), /review\.diff/);
  assert.equal(await activeAttr(page, "data-mid"), href!.split("/").pop(), "Focus lands on the reply");
  assert.match(await activeAttr(page, "aria-label") ?? "", /^Linked reply from You/);
  await page.locator("aside.thread [data-act=thread-close]").click();
  assert.equal(await page.locator("aside.thread").count(), 0, "Close closes it; the link is not taken again on re-render");
}
/* An attachment-only message is never an empty context entry: names and links reach the excerpt and Work. */
async function checkTaskContextAttachments(page: Page) {
  await navTo(page, "#/chat/design");
  await mainComposer(page).locator(".chat-att-input").setInputFiles([{ name: "palette.pdf", mimeType: "application/pdf", buffer: Buffer.from("%PDF-1.4 p") }]);
  await waitUploads(page);
  await mainComposer(page).getByRole("button", { name: "Send" }).click();
  await page.waitForFunction(() => !document.querySelector("#main .chat .msg.is-pending"));
  const mid = await page.locator("#main .chat .chmain .msgs > .msg").last().getAttribute("data-mid");
  await page.locator("#compose").fill("/task Review the palette");
  await page.locator("#compose").press("Control+Enter");
  const dlg = page.locator("#work-task-dialog");
  assert.match(await dlg.locator(".stc-ctx-preview").innerText(), /You: Attached: palette\.pdf/, "The preview names the file");
  assert.match(await dlg.locator(".stc-ctx-toggle").innerText(), /^Show \d+ messages? · 1 file$/, "The collapsed excerpt counts its files");
  assert.deepEqual(await dlg.locator(".stc-sends .stc-sends-file").allInnerTexts(), ["palette.pdf"], "The file is named above the send actions");
  await dlg.getByRole("button", { name: /^Show \d+ messages? · 1 file$/ }).click();
  const file = dlg.locator(".stc-ctx-lines li").last().locator(".stc-ctx-files a");
  assert.equal(await dlg.locator(".stc-ctx-lines li").last().locator(".ctx-file-name").innerText(), "palette.pdf");
  assert.equal(await file.innerText(), "Open source message");
  assert.equal(await file.getAttribute("href"), `#/chat/design/${mid}`, "and links its message");
  await dlg.getByRole("button", { name: "Create as-is" }).click();
  await dlg.waitFor({ state: "detached" });
  const saved = await page.evaluate(() => Object.keys(localStorage).filter((k) => k.startsWith("agentic-org.v6.task."))
    .map((k) => JSON.parse(localStorage.getItem(k)!)).find((t) => t.context?.[0]?.source?.ch === "design"));
  await navTo(page, `#/work?item=${saved.ref}`);
  const panel = page.locator("#work-panel .work-ctx");
  await panel.waitFor();
  assert.match(await panel.locator(".work-ctx-preview").innerText(), /Attached: palette\.pdf/);
  await panel.locator(".work-ctx-msgs summary").click();
  assert.equal(await panel.locator(".work-ctx-files a").last().getAttribute("href"), `#/chat/design/${mid}`, "Work's detail links the file's message");
}
/* The viewer's Download keeps its name where its word is hidden. */
async function checkLightboxName(page: Page) {
  await viewport(page, 375, 667);
  await navTo(page, "#/chat/agency-v1");
  await page.locator('.msg[data-mid="m1"] .chat-att-img').first().click();
  const viewer = page.locator("dialog.chat-lightbox");
  await viewer.waitFor();
  await viewer.getByRole("link", { name: "Download ia-v2-nav-before.png" }).waitFor();
  await page.keyboard.press("Escape");
  await viewport(page, 1440, 900);
}
/* Crossing 1280 px with a thread open syncs its mode at once: dialog, inert background, focus and resize edge. */
async function checkPanelModeSync(page: Page) {
  await viewport(page, 1440, 900);
  await navTo(page, "#/chat/agency-v1");
  await page.locator('.msg[data-mid="m2"] .thr').click();
  const sep = page.locator('aside.thread [role="separator"]');
  await sep.waitFor();
  const grip = await sep.evaluate((el) => { const st = getComputedStyle(el, "::before"); return { bg: st.backgroundColor, dots: st.backgroundImage, h: st.height }; });
  assert.ok(grip.bg !== "rgba(0, 0, 0, 0)" && /radial-gradient/.test(grip.dots) && grip.h === "28px", `The dotted resize grip is always visible (${JSON.stringify(grip)})`);
  assert.equal(await sep.getAttribute("aria-label"), "Resize thread", "The edge names what it resizes");
  await sep.focus();
  assert.equal(await sep.locator(".chat-panel-resize-tip").evaluate((el) => getComputedStyle(el).opacity), "1", "Resize thread shows on keyboard focus");
  await page.locator("#compose").focus();
  await viewport(page, 1024, 768);
  await page.waitForFunction(() => document.querySelector("aside.thread")?.getAttribute("aria-modal") === "true");
  assert.equal(await sep.count(), 0, "Overlay: no resize edge");
  assert.ok(await page.evaluate(() => (document.querySelector(".chat .chmain") as HTMLElement).inert), "The conversation is inert behind the dialog");
  assert.ok(await page.evaluate(() => Boolean(document.activeElement?.closest("aside.thread"))), "Focus moves into the dialog");
  await viewport(page, 1440, 900);
  await page.waitForFunction(() => !document.querySelector("aside.thread")?.hasAttribute("aria-modal"));
  await sep.waitFor();
  assert.ok(await page.evaluate(() => !(document.querySelector(".chat .chmain") as HTMLElement).inert), "Inline: nothing is inert");
  assert.equal(await page.locator("aside.thread").getAttribute("role"), null);
  await page.locator("aside.thread [data-act=thread-close]").click();
}
/* Inline approvals state the scope and risk they rest on, with their context, above the buttons; the routing reason
   reads at 4.5:1. */
async function checkApprovalFacts(page: Page) {
  await navTo(page, "#/chat/agency-v1");
  const card = page.locator('.msg[data-mid="m3"] .chat-needs');
  const facts = await card.locator(".chat-needs-facts li").allInnerTexts();
  assert.deepEqual(facts, ["Scope: Local test sandbox only · ends after the security run", "Risk: Low risk · Zero production or customer data"]);
  const why = card.locator("details.chat-needs-more");
  assert.equal(await why.getAttribute("open"), null, "The longer reasoning, which repeats the facts, is folded");
  assert.equal(await why.locator("summary").innerText(), "Jules recommends: allow for testing only.", "behind the recommendation, which stays in view");
  assert.match(await why.evaluate((e) => e.textContent!), /No live customer or company data/, "and still there to read");
  const [f, a] = await Promise.all([card.locator(".chat-needs-facts").boundingBox(), card.locator(".actions").boundingBox()]);
  assert.ok(f && a && f.y + f.height <= a.y, "The facts sit above the buttons");
  const ratio = await page.locator('.msg[data-mid="m2"] .chat-needs-route').evaluate(textContrast);
  assert.ok(ratio >= 4.5, `The routing reason reads at 4.5:1 (${ratio.toFixed(2)})`);
}
async function checkRouteContrastThemes(page: Page) {
  for (const theme of ["ocean", "classic", "report", "report-dark", "graphite", "graphite-light", "cozy"]) {
    const u = new URL(page.url());
    u.searchParams.set("theme", theme);
    u.hash = "#/chat/agency-v1";
    await page.goto(u.href);
    const route = page.locator('.msg[data-mid="m2"] .chat-needs-route');
    await route.waitFor();
    const ratio = await route.evaluate(textContrast);
    assert.ok(ratio >= 4.5, `Routing reason contrast in ${theme}: ${ratio.toFixed(2)}`);
  }
  const u = new URL(page.url());
  u.searchParams.set("theme", "classic");
  await page.goto(u.href);
  await page.locator("#main h1").first().waitFor();
}
/* The list's tools stay put while the conversations scroll; names win over presence words in the 240 px rail. */
async function checkListRail(page: Page) {
  await viewport(page, 1440, 700);
  await navTo(page, "#/chat/general");
  const findTop = () => page.locator("#chat-find").evaluate((e) => Math.round(e.getBoundingClientRect().top));
  const before = await findTop();
  const scrolled = await page.locator("#chat-nav").evaluate((n) => { n.scrollTop = n.scrollHeight; return n.scrollTop; });
  assert.ok(scrolled > 0, "The conversations scroll");
  assert.equal(await findTop(), before, "Find and New message stay above them");
  assert.ok(await page.locator("#chat-new").isVisible());
  await navTo(page, openDmHash("noor"));
  const row = page.locator('#chat-nav a[href="#/chat/dm-noor"]');
  await row.waitFor();
  const m = await row.evaluate((a) => {
    const name = a.querySelector(".chat-row-name")!, word = a.querySelector(".chat-row-word");
    return { nameH: name.getBoundingClientRect().height, line: parseFloat(getComputedStyle(name).lineHeight) || 20, word: word?.textContent };
  });
  assert.ok(m.word && m.nameH <= m.line * 1.3, `Noor's name keeps one line beside "${m.word}" (${m.nameH}px)`);
  await viewport(page, 1440, 900);
}
const openDmHash = (who: string) => `#/chat/dm-${who}`;
/* At 800 px a DM's header keeps the actionable status, in the same words as Details. */
async function checkCompactStatus(page: Page) {
  await viewport(page, 800, 800);
  await navTo(page, "#/chat/dm-ada");
  const summary = page.locator(".chat-ctx-summary");
  await summary.waitFor();
  assert.equal(await summary.innerText(), "1 in this DM · 2 more requests", "The summary leads with the request here");
  assert.ok(await summary.evaluate((e) => e.scrollWidth <= e.clientWidth + 1), "and is not cut off");
  await page.locator(".chat-details-btn").click();
  assert.match(await page.locator("#chat-details").innerText(), /Engineering Manager/, "Role moves to Details");
  assert.match(await page.locator("#chat-details .chat-status").innerText(), /^1 in this DM · 2 more requests$/, "Details agrees with the summary");
  assert.equal(await summary.isVisible(), false, "With Details open, the status shows once");
  await page.locator(".chat-details-btn").click();
  await viewport(page, 1440, 900);
}
/* Opening at the end never leaves an attachment as a sliver under the sticky bands. */
async function checkNoSliver(page: Page) {
  await viewport(page, 1440, 900);
  await page.goto(page.url().split("#")[0] + "#/chat/agency-v1");
  await page.locator('.msg[data-mid="m1"]').waitFor();
  await page.waitForTimeout(600);
  const shown = await page.evaluate(() => {
    const s = document.querySelector(".chmain .msgs[data-ch]")!, band = s.querySelector(":scope > .chat-day")!.getBoundingClientRect().bottom;
    const cut = s.querySelector(":scope > .msg.is-cut .hd");
    const top = cut ? cut.getBoundingClientRect().bottom : band;
    const r = s.querySelector('[data-mid="m1"] .chat-atts')!.getBoundingClientRect();
    return Math.max(0, r.bottom - Math.max(top, r.top));
  });
  assert.ok(shown === 0 || shown > 56, `Thumbnails are hidden or clearly shown, never a sliver (${shown}px)`);
}
/* At 375 px both jumps share one row and the composer placeholder is one line. */
async function checkPhoneJumpRow(page: Page) {
  await viewport(page, 375, 667);
  await navTo(page, "#/chat/agency-v1");
  const field = page.locator("#compose");
  await field.waitFor();
  assert.equal(await field.getAttribute("placeholder"), "Message", "A one-line placeholder");
  assert.equal(await field.getAttribute("aria-label") ?? await page.locator('label[for="compose"]').innerText(), "Message #agentic-org-v1", "The label keeps the full name");
  const both = await page.evaluate(async () => {
    const s = document.querySelector(".chmain .msgs[data-ch]") as HTMLElement;
    const btns = () => [...document.querySelectorAll('[data-act="chat-asks-jump"], [data-act="chat-latest-jump"]')] as HTMLElement[];
    for (let y = 0; y <= s.scrollHeight; y += 24) {
      s.scrollTop = y;
      s.dispatchEvent(new Event("scroll"));
      await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
      if (btns().every((b) => !b.hidden)) return btns().map((b) => b.getBoundingClientRect().top);
    }
    return null;
  });
  assert.ok(both && Math.abs(both[0] - both[1]) < 1, `Both jumps share one row (${both})`);
  await page.screenshot({ path: resolve(import.meta.dir, "../../.playwright", "poc-chat-375-jump-row.png") });
  await viewport(page, 1440, 900);
}
async function checkChatRound8(page: Page) {
  await checkSlashProse(page);
  await checkSlashTaskText(page);
  await checkUnknownCommand(page);
  await checkCommandKeepsText(page);
  await checkLightboxName(page);
  await checkPanelModeSync(page);
  await checkApprovalFacts(page);
  await checkListRail(page);
  await checkCompactStatus(page);
  await checkNoSliver(page);
  await checkPhoneJumpRow(page);
  await checkTaskContextAttachments(page);
  await checkRouteContrastThemes(page);
}
/* ---------------------------------------------------------------- Chat round 9 (TASK-195) */
const composeHint = (page: Page) => page.locator('form[data-form="send"] .chat-compose-hint');
/* Indented text is prose; /assign changes nothing until Assign is chosen. */
async function checkCommandBoundaries(page: Page) {
  await viewport(page, 1440, 900);
  await navTo(page, "#/chat/general");
  const field = page.locator("#compose");
  await field.fill(" /assign @rowan T-A121");
  await field.press("Control+Enter");
  assert.match(await lastMessageText(page), /\/assign @Rowan T-A121/, "Indented text sends as a message");
  assert.equal(await page.locator(".chat-assign-ask").count(), 0, "and is never a command");
  assert.equal(await field.inputValue(), "");
  await field.fill("/assign @rowan T-A121");
  await field.press("Control+Enter");
  const ask = page.locator(".chat-assign-ask");
  assert.match(await ask.innerText(), /Assign T-A121 .+ from Kai to Rowan\?/, "The confirmation names from whom and to whom");
  await ask.getByRole("button", { name: "Cancel" }).click();
  assert.equal(await page.locator(".chat-assign-ask").count(), 0);
  assert.equal(await field.inputValue(), "/assign @rowan T-A121", "Cancel keeps the command");
  assert.equal(await activeAttr(page, "id"), "compose");
  await field.press("Control+Enter");
  await page.keyboard.press("Escape");
  assert.equal(await page.locator(".chat-assign-ask").count(), 0, "Escape withdraws it too");
  await field.fill("/assign @kai T-A121");
  await field.press("Control+Enter");
  assert.match(await composeHint(page).innerText(), /T-A121 is already assigned to Kai/, "Nothing changed before an explicit Assign");
  await field.fill("/assign @rowan T-A121");
  await field.press("Control+Enter");
  await field.press("End");
  await page.keyboard.type("4");
  assert.equal(await page.locator(".chat-assign-ask").count(), 0, "Editing the command withdraws the confirmation");
  await field.fill("");
}
/* A mention list whose caret has left its token never writes over the words the caret moved to. */
async function checkMentionCaret(page: Page) {
  const field = page.locator("#compose");
  await field.fill("");
  await field.click();
  await page.keyboard.type("ship it today @ma");
  await page.locator("#compose-assist").waitFor();
  await field.evaluate((f: HTMLTextAreaElement) => f.setSelectionRange(5, 5));
  await page.keyboard.press("Enter");
  assert.equal(await field.inputValue(), "ship it today @ma", "Selecting with the caret outside the token changes nothing");
  assert.equal(await page.locator("#compose-assist").count(), 0, "and closes the list");
  await field.press("End");
  await page.keyboard.type("y");
  await page.locator("#compose-assist").waitFor();
  await page.keyboard.press("Home");
  await page.locator("#compose-assist").waitFor({ state: "detached" });
  assert.equal(await field.getAttribute("aria-expanded"), "false", "Moving the caret out of the token closes the list");
  await field.fill("ping @mayx now");
  await field.evaluate((f: HTMLTextAreaElement) => { f.setSelectionRange(9, 9); f.dispatchEvent(new Event("input", { bubbles: true })); });
  await page.locator("#compose-assist").waitFor();
  await page.keyboard.press("Enter");
  assert.match(await field.inputValue(), /^ping @\w+ now$/, "Completing inside a token replaces only that token");
  await field.fill("");
}
/* Armed denial replaces the whole action row; Approve is gone until it is disarmed. */
async function checkDenyArmed(page: Page) {
  await viewport(page, 1440, 900);
  await navTo(page, "#/chat/agency-v1");
  const card = page.locator('.msg[data-mid="m3"] .chat-needs');
  await card.locator('[data-act="deny-arm"]').click();
  const group = card.getByRole("group", { name: "Confirm denial" });
  await group.waitFor();
  assert.equal(await card.locator('[data-act="handle"]').count(), 0, "Allow test access is not on screen while denial is armed");
  assert.deepEqual(await group.getByRole("button").allInnerTexts(), ["Confirm denial", "Keep request open"]);
  assert.equal(await activeAttr(page, "id"), "deny-msg-in5", "Focus starts in the optional reason, inside the group");
  await group.getByRole("button", { name: "Keep request open" }).click();
  assert.equal(await activeAttr(page, "data-act"), "deny-arm", "Keep request open returns to Deny");
  await card.getByRole("button", { name: "Allow test access" }).waitFor();
}
/* Ada's DM leads with the request answerable here, linked to its card, then the ones in Inbox. */
async function checkAdaHeader(page: Page) {
  await viewport(page, 1440, 900);
  await navTo(page, "#/chat/dm-ada");
  const here = page.locator(".chat-context .chat-here-link");
  assert.equal(await here.innerText(), "1 in this DM");
  assert.equal(await here.getAttribute("href"), "#/chat/dm-ada/x3", "linked to the hand-off card");
  assert.equal(await page.locator(".chat-context .chat-more-btn").innerText(), "2 more requests");
  assert.ok(await page.locator(".chat-context .chat-status").evaluate((s) => s.textContent!.indexOf("this DM") < s.textContent!.indexOf("more")), "Here comes first");
}
/* Back closes the image viewer first and returns focus to its thumbnail, in the stream and in a thread. */
async function viewerBack(page: Page, scope: string, mid: string) {
  await page.locator(`${scope} .msg[data-mid="${mid}"] .chat-att-img`).first().click();
  await page.locator("dialog.chat-lightbox").waitFor();
  const hash = await page.evaluate(() => location.hash);
  await page.goBack();
  await page.locator("dialog.chat-lightbox").waitFor({ state: "detached" });
  assert.equal(await page.evaluate(() => location.hash), hash, `Back closes the viewer (${scope}) and stays in the conversation`);
  assert.equal(await activeAttr(page, "data-act"), "chat-att-view", "Focus returns to the thumbnail");
  assert.equal(await activeAttr(page, "data-id"), mid);
}
async function checkViewerBack(page: Page) {
  await viewport(page, 1440, 900);
  await navTo(page, "#/chat/agency-v1");
  await page.locator('.msg[data-mid="m1"] .chat-att-img').first().waitFor();
  await viewerBack(page, ".chmain", "m1");
  await page.locator('.msg[data-mid="m2"] .thr').click();
  const form = page.locator('aside.thread form[data-form="thread-send"]');
  await form.locator(".chat-att-input").setInputFiles([{ name: "shot.png", mimeType: "image/png", buffer: PNG_1PX }]);
  await waitUploads(page);
  await form.locator("#thread-compose").press("Control+Enter");
  const tile = page.locator("aside.thread .msgs > .msg:last-child .chat-att-img");
  await tile.waitFor();
  await viewerBack(page, "aside.thread", (await page.locator("aside.thread .msgs > .msg:last-child").getAttribute("data-mid"))!);
  assert.equal(await page.locator("aside.thread").count(), 1, "The thread stays open");
  await page.goBack();
  await page.locator("aside.thread").waitFor({ state: "detached" });
  assert.equal(await page.evaluate(() => location.hash), "#/chat/agency-v1", "The next Back closes the thread");
}
/* Find a conversation: a task reference finds channels and DMs too. */
async function checkFindTaskRef(page: Page) {
  await viewport(page, 1440, 900);
  await navTo(page, "#/chat/general");
  await page.locator("#chat-find").fill("T-A121");
  const why = (href: string) => page.locator(`#chat-nav a[href="${href}"] .chat-row-why`).innerText();
  assert.match(await why("#/chat/agency-v1"), /^, ?mentions T-A121$|mentions T-A121/, "A channel that discusses T-A121 is found");
  assert.match(await why("#/chat/a2a-ada-tomas"), /mentions T-A121/);
  await page.locator("#chat-find").fill("");
}
/* An older open pair leads its section, labelled as outside the window, with its last-active time; a closed picker
   keeps the picked names and Clear. */
async function checkA2APicker(page: Page) {
  await viewport(page, 1440, 900);
  await navTo(page, "#/chat/a2a-kai-rowan");
  const labels = await page.locator(".chat-a2a-window").allInnerTexts();
  assert.deepEqual(labels, ["Older than 24h"], "The older pair leads the section, labelled as outside the window");
  assert.equal(await page.locator("#chat-a2a-day").getAttribute("aria-pressed"), "true", "while the scope says Last 24 hours");
  const current = page.locator(".chat-a2a-window").nth(0).locator("xpath=following-sibling::ul[1]//a");
  assert.equal(await current.getAttribute("href"), "#/chat/a2a-kai-rowan");
  assert.equal(await current.locator(".chat-row-when").innerText(), "Yesterday", "It says when the pair last spoke");
  await page.locator("#chat-a2a-filter-btn").click();
  await page.keyboard.type("ro");
  await page.keyboard.press("Enter");
  await page.keyboard.press("Escape");
  assert.equal(await page.locator("#chat-a2a-filter").count(), 0, "The picker closes");
  const active = page.locator("#chat-a2a-head .chat-a2a-who");
  assert.match(await active.innerText(), /Rowan/, "The picked agent stays named in the heading");
  await active.getByRole("button", { name: "Clear the agent filter" }).click();
  assert.equal(await page.locator("#chat-a2a-head .chat-a2a-who").count(), 0);
  assert.equal(await page.locator("#chat-a2a-day").getAttribute("aria-pressed"), "true", "Clearing restores the date scope");
  assert.equal(await activeAttr(page, "id"), "chat-a2a-filter-btn", "Clear returns focus to Filter");
}
/* 320–375: jumps are as wide as their labels, share one row at 320 ("Latest") and never clip; Jules's scope, risk and action are in the first
   view at 375; the thread's resize edge is a 24 px target with a slim grip. */
async function bothJumps(page: Page) {
  return page.evaluate(async () => {
    const s = document.querySelector(".chmain .msgs[data-ch]") as HTMLElement;
    const btns = () => [...document.querySelectorAll('[data-act="chat-asks-jump"], [data-act="chat-latest-jump"]')] as HTMLElement[];
    for (let y = 0; y <= s.scrollHeight; y += 24) {
      s.scrollTop = y;
      s.dispatchEvent(new Event("scroll"));
      await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
      if (btns().every((b) => !b.hidden)) {
        return btns().map((b) => { const r = b.getBoundingClientRect(), t = b.querySelector("span")!; return { top: r.top, left: r.left, right: r.right, clipped: t.scrollWidth > t.clientWidth + 1 }; });
      }
    }
    return null;
  });
}
async function checkNarrowRound9(page: Page) {
  await viewport(page, 320, 700);
  await navTo(page, "#/chat/agency-v1");
  await page.locator("#compose").waitFor();
  const at320 = await bothJumps(page);
  assert.ok(at320, "Both jumps show at 320");
  assert.ok(at320!.every((b) => !b.clipped && b.left >= 0 && b.right <= 320), `No label is clipped at 320 (${JSON.stringify(at320)})`);
  assert.ok(Math.abs(at320![0].top - at320![1].top) < 1 && at320![0].right <= at320![1].left, `Both jumps fit one row at 320 (${JSON.stringify(at320)})`);
  const head = await page.locator("#main .chat > .chhead").evaluate((h) => [...h.querySelectorAll(".chat-list-btn, .chat-actions > .btn")]
    .filter((b) => (b as HTMLElement).offsetParent).map((b) => { const r = b.getBoundingClientRect(); return [Math.round(r.left), Math.round(r.right)]; }));
  assert.ok(head.every(([l, r], i) => l >= 0 && r <= 320 && (i === 0 || l >= head[i - 1][1] - 1)), `Header controls never overlap at 320 (${JSON.stringify(head)})`);
  /* Scope, risk, Allow and Deny share the first view at 375 and 320: the card reclaims the avatar gutter. */
  for (const [w, h] of [[375, 667], [320, 700]]) {
    await viewport(page, w, h);
    await reloadAt(page, "#/chat/agency-v1");
    const card = page.locator('.msg[data-mid="m3"] .chat-needs');
    await card.waitFor();
    await page.waitForTimeout(500);
    const fold = await page.locator(".chmain .msgs[data-ch]").evaluate((s) => s.getBoundingClientRect().bottom);
    for (const sel of [".chat-needs-facts", '[data-act="handle"]', '[data-act="deny-arm"]']) {
      const box = await card.locator(sel).boundingBox();
      assert.ok(box && box.y >= 0 && box.y + box.height <= fold + 1 && box.x >= 0 && box.x + box.width <= w, `${sel} is whole in the first view at ${w} (${JSON.stringify(box)} vs ${fold})`);
    }
    const allow = await card.locator('[data-act="handle"]').evaluate((b) => b.scrollWidth <= b.clientWidth + 1);
    assert.ok(allow, `"Allow test access" is not clipped at ${w}`);
  }
  await viewport(page, 1440, 900);
  await navTo(page, "#/chat/agency-v1");
  await page.locator('.msg[data-mid="m2"] .thr').click();
  const sep = page.locator('aside.thread [role="separator"]');
  await sep.waitFor();
  const hit = await sep.evaluate((el) => ({ w: el.getBoundingClientRect().width, grip: getComputedStyle(el, "::before").width }));
  assert.deepEqual(hit, { w: 24, grip: "14px" }, "A 24 px hit area with a small dotted grip");
  await page.locator("aside.thread [data-act=thread-close]").click();
}
async function checkChatRound9(page: Page) {
  await checkCommandBoundaries(page);
  await checkMentionCaret(page);
  await checkDenyArmed(page);
  await checkAdaHeader(page);
  await checkViewerBack(page);
  await checkFindTaskRef(page);
  await checkA2APicker(page);
  await checkNarrowRound9(page);
}
/* Work reload: a task made from Chat keeps its context snapshot, with exact message and file references; a source
   that can no longer open (a message from an earlier session, a conversation that is gone) says so; a tampered,
   unsafe entry is dropped while the rest load. */
const savedTasks = (page: Page) => page.evaluate(() => Object.keys(localStorage).filter((k) => k.startsWith("agentic-org.v6.task."))
  .map((key) => ({ key, t: JSON.parse(localStorage.getItem(key)!) })));
async function reloadAt(page: Page, hash: string) {
  await navTo(page, hash);
  await page.reload();
  await page.locator("#main h1").first().waitFor();
}
async function checkWorkContextReload(page: Page) {
  await viewport(page, 1440, 900);
  await navTo(page, "#/chat/agency-v1");
  await page.locator("#compose").fill("/task Ship the release notes");
  await page.locator("#compose").press("Control+Enter");
  const dlg = page.locator("#work-task-dialog");
  /* Earlier messages are opt-in; including them brings m1's images. */
  await dlg.getByRole("button", { name: /^Include \d+ earlier messages?$/ }).click();
  await dlg.getByRole("button", { name: "Create as-is" }).click();
  await dlg.waitFor({ state: "detached" });
  const all = await savedTasks(page);
  const made = all.find((x) => x.t.title === "Ship the release notes")!.t;
  const design = all.find((x) => x.t.context?.[0]?.source?.ch === "design")!;
  await page.evaluate((key) => {
    const t = JSON.parse(localStorage.getItem(key)!);
    t.context.push({ kind: "conversation", title: "Unsafe", href: "javascript:alert(1)", source: { ch: "general" } },
      { kind: "conversation", title: "Gone channel", href: "#/chat/nope", preview: "", source: { ch: "nope", msg: "z9" }, messages: [] });
    localStorage.setItem(key, JSON.stringify(t));
  }, design.key);
  await reloadAt(page, `#/work?item=${made.ref}`);
  const panel = page.locator("#work-panel .work-ctx");
  await panel.waitFor();
  assert.match(await panel.innerText(), /Conversation excerpt · #agentic-org-v1/, "The excerpt survives a reload");
  assert.equal(await panel.getByRole("link", { name: "Open source message" }).getAttribute("href"), "#/chat/agency-v1/m4", "with its exact message");
  await panel.locator(".work-ctx-msgs summary").click();
  const files = await panel.locator(".work-ctx-files a").evaluateAll((els) => els.map((e) => e.getAttribute("href")));
  assert.ok(files.length > 0 && files.every((h) => h === "#/chat/agency-v1/m1"), `and each file links its exact message (${files})`);
  assert.equal(await panel.locator(".work-ctx-gone").count(), 0, "Sources that open say nothing extra");
  await reloadAt(page, `#/work?item=${design.t.ref}`);
  const items = page.locator("#work-panel .work-ctx .work-ctx-item");
  await items.first().waitFor();
  assert.equal(await items.count(), 2, "The unsafe entry is dropped; the rest load");
  assert.match(await items.nth(0).locator(".work-ctx-links").innerText(), /Source unavailable/, "A message from an earlier session says it can't open");
  await items.nth(0).locator("summary").click();
  assert.match(await items.nth(0).locator(".work-ctx-files").innerText(), /palette\.pdf\s+Message unavailable/, "Its file keeps its name and says why it has no link");
  assert.equal(await items.nth(1).locator("a").count(), 0, "A conversation that is gone is not a link");
  assert.match(await items.nth(1).innerText(), /Gone channel\s+Source unavailable/);
  assert.equal(await page.locator('a[href^="javascript:"]').count(), 0);
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
  await page.reload();
  await checkChatRound6(page);
  await checkPrimaryContrast(page);
  await checkTaskContextFromChat(page);
  await checkTaskContextSourceLink(page);
  await page.reload();
  await checkChatAttachments(page);
  await page.reload();
  await checkChatRound7(page);
  await page.reload();
  await checkA2AScale(page);
  await page.reload();
  await checkComposerAssists(page);
  await page.reload();
  await checkChatRound8(page);
  await page.reload();
  await checkChatRound9(page);
  await checkWorkContextReload(page);
}

const ROUTE_CHECKS: Record<string, (page: Page, href: string, theme: string) => Promise<void>> = {
  inbox: (page) => checkInbox(page),
  work: (page, href) => checkWork(page, href),
  activity: (page, _href, theme) => checkActivity(page, theme),
  "projects/office-federation/activity": (page) => checkProjectActivity(page),
  "projects/agency-v1/insights": (page) => checkProjectInsights(page),
  chat: (page) => checkChat(page),
  "projects/agency-v1": (page) => checkTaskContextFromPage(page),
  home: (page, href) => checkPersonPanel(page, href),
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

const CHAT_SWEEP = ["general", "agency-v1", "platform", "design", "dm-ada", "dm-eden", "dm-maya", "dm-noor", "dm-rowan", "a2a-kai-rowan", "a2a-tomas-sage", "mentions", "threads"];
/* Each conversation loads fresh at 1024 px, with motion on, where the app nav animates as the page settles. */
async function sweepChatAtLaptopWidth(page: Page, baseUrl: URL) {
  await page.setViewportSize({ width: 1024, height: 768 });
  await page.emulateMedia({ reducedMotion: "no-preference" });
  for (const c of CHAT_SWEEP) {
    const u = new URL(baseUrl);
    u.searchParams.set("sweep", c);
    u.hash = "#/chat/" + c;
    await page.goto(u.href);
    await page.locator("#main h1").first().waitFor({ state: "visible" });
    await page.waitForTimeout(400);
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
    /* Window error events that Playwright does not surface as page errors (a ResizeObserver loop, for one) still
       raise the Bun dev overlay, so they are collected too. */
    await page.exposeFunction("__smokeWindowError", (m: string) => errors.push("window error: " + m));
    await page.addInitScript(() => addEventListener("error", (e) => (window as unknown as { __smokeWindowError: (m: string) => void }).__smokeWindowError(String(e.message))));
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
    await sweepChatAtLaptopWidth(page, baseUrl);
    assert.deepEqual(failed, [], "No HTTP >=400 responses");
    assert.deepEqual(errors, [], "No page or console errors");
    console.log(`Passed sweep: ${SWEEP_ROUTES.length} routes x ${ALL_THEMES.length} themes (0 errors)`);
  } finally {
    await page.close();
  }
}

const args = process.argv.slice(2);
const runAll = args.includes("--all");
/* --dev serves the page as the owner's dev server does (development mode, stylesheets unbundled) */
const server = servePrototype(0, args.includes("--dev"));

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
