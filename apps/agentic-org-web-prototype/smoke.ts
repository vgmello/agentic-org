import assert from "node:assert/strict";
import { resolve } from "node:path";
import { chromium } from "../apps/agentic-org-ui/node_modules/playwright";
import { servePrototype } from "./server";

const [pageRoute = "work", theme = "graphite"] = process.argv.slice(2);
const route = pageRoute.replace(/^#?\/?/, "");
const server = servePrototype(0, false);

try {
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage({
      viewport: { width: 1440, height: 900 },
      reducedMotion: "reduce",
    });
    const errors: string[] = [];
    page.on("pageerror", (error) => errors.push(error.message));
    page.setDefaultTimeout(5000);
    const url = new URL(server.url);
    url.searchParams.set("theme", theme);
    url.hash = "/" + route;

    const response = await page.goto(url.href);
    assert.ok(response?.ok(), "Prototype preview loads");
    await page.locator("#main h1").first().waitFor({ state: "visible" });
    assert.equal(await page.locator("html").getAttribute("data-theme"), theme);

    if (route === "inbox") {
      const categories = page.getByRole("group", { name: "Inbox queue controls" });
      const category = (name: RegExp) => categories.getByRole("button", { name });
      const search = page.getByRole("searchbox", { name: "Search inbox" });
      await search.press("m");
      await search.press("a");
      assert.equal(await search.inputValue(), "ma");
      await search.press("Escape");
      assert.ok(await search.evaluate((input) => input !== document.activeElement));
      assert.equal(await search.inputValue(), "ma", "Escape leaves the draft search intact");
      await search.clear();
      await search.press("Escape");

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

    if (route === "work") {
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
      await page.goto(url.href);
      await groups.first().getByRole("heading", { name: /^Blocked\b/ }).waitFor();
      const layout = page.getByRole("group", { name: "Work layout" });
      await layout.getByRole("button", { name: "Board" }).press("Enter");
      const board = page.getByRole("region", { name: "Work board" });
      const columns = board.locator(".bcol");
      const cards = board.locator(".bcard");
      const cardIds = () => cards.evaluateAll((items) => items.map((item) => item.getAttribute("data-work-id")).sort());
      assert.equal(await columns.count(), 6);
      const detailedIds = await cardIds();
      const attention = board.locator(".bcard[data-attention]").first();
      await attention.locator(".board-card-footer .pill").waitFor();
      assert.equal(await attention.locator(":scope > *").count(), 3);
      const trailingGutter = await board.evaluate((el) => {
        el.scrollLeft = el.scrollWidth;
        const lastColumn = el.querySelector(".bcol:last-child")!;
        return el.getBoundingClientRect().left + el.clientWidth - lastColumn.getBoundingClientRect().right;
      });
      assert.ok(trailingGutter >= 15, "The final board column scrolls clear of the right edge");
      await page.getByRole("button", { name: "4-Stage Unified" }).press("Enter");
      assert.equal(await columns.count(), 4);
      assert.deepEqual(await cardIds(), detailedIds, "Unified stages keep every card");
      await layout.getByRole("button", { name: "List" }).press("Enter");
    }
    if (route === "activity") {
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

    if (route === "projects/office-federation/activity") {
      const assembly = page.getByRole("tab", { name: "Assembly line" });
      const feed = page.getByRole("tab", { name: "Activity feed" });
      const readiness = page.getByRole("progressbar", { name: "Project readiness" });
      const release = page.getByRole("region", { name: "Release summary" });
      const releaseReadiness = release.getByRole("progressbar", { name: "Release readiness" });
      const assemblyCycle = page.locator(".assembly-metrics [data-assembly-cycle]");
      const panel = page.getByRole("complementary", { name: "Release gates" });
      const tools = page.getByRole("group", { name: "Activity sidebar tools" });
      const simulation = tools.getByRole("button", { name: "Simulation" });
      const gates = tools.getByRole("button", { name: "Release gates" });
      await readiness.waitFor();
      const projectHeader = page.locator(".project-header");
      assert.equal(await projectHeader.getByRole("progressbar").count(), 0);
      assert.doesNotMatch((await projectHeader.textContent())!, /\bDue\b|\d+ of \d+ done/);
      assert.equal(await assembly.getAttribute("aria-selected"), "true");
      const viewControls = projectHeader.locator(".project-activity-controls");
      const viewSettings = viewControls.getByRole("group", { name: "Activity view settings" });
      await viewControls.getByRole("tab", { name: "Assembly line", selected: true }).waitFor();
      await viewSettings.getByRole("button", { name: "Assembly line display" }).waitFor();
      const orderedRelease = page.locator(".project-header + .activity-workspace > #project-release-summary:first-child");
      await orderedRelease.waitFor();
      assert.equal(await release.count(), 1, "The release summary moved out of the Assembly line table");
      assert.equal(await readiness.getAttribute("max"), "4");
      assert.equal(await releaseReadiness.getAttribute("max"), "4");
      assert.equal(await releaseReadiness.getAttribute("value"), await readiness.getAttribute("value"));
      assert.deepEqual(await release.getByRole("link").allTextContents().then((links) => links.map((label) => label.trim())), ["File access ruling · Pending"]);
      assert.equal(await page.locator(".assembly-milestone").count(), 1);
      assert.equal(await page.locator('[data-assembly-work="T-A121"]').count(), 0);
      assert.equal(await gates.getAttribute("aria-expanded"), "true");
      const milestones = panel.getByRole("region", { name: "Milestones" });
      assert.match((await milestones.textContent())!, /Office sync test.*\d+\/4 tasks · \d+\/2 features/s);
      const gateStatus = panel.getByRole("region", { name: "Gate status" });
      assert.deepEqual(await gateStatus.getByRole("link").allTextContents(), ["File access ruling"]);
      assert.equal(await panel.getByRole("region", { name: /^Needs a human/ }).getByRole("link", { name: "Custody lock fix" }).getAttribute("href"), "#/inbox/in2");
      const throughput = panel.getByRole("region", { name: "Throughput" });
      const lineEvents = panel.getByRole("region", { name: "Line events" });
      assert.equal(await throughput.getByRole("listitem").count(), 4);
      await lineEvents.getByText("File access ruling requested").waitFor();

      await simulation.press("Enter");
      const playback = page.getByRole("complementary", { name: "Simulation" });
      await playback.getByRole("button", { name: "Reset simulation" }).click();
      assert.equal(await assemblyCycle.textContent(), "—");
      await playback.getByRole("button", { name: "8×" }).click();
      await playback.getByRole("button", { name: "Add sample task" }).click();
      await playback.getByText("joined the queue").waitFor();
      assert.equal(await readiness.getAttribute("max"), "5");
      assert.equal(await releaseReadiness.getAttribute("max"), "5");
      await playback.getByRole("button", { name: "Play simulation" }).click();
      await page.waitForFunction(() => document.querySelector('[data-assembly-work="T-S1"]:not([data-stage="queue"])'));
      assert.doesNotMatch((await playback.locator(".assembly-event-log").textContent())!, /T-A130|T-A84/);
      await gates.click();
      await lineEvents.getByText("T-S1 passed review and merged").waitFor();
      assert.doesNotMatch((await lineEvents.textContent())!, /T-A130|T-A84/);
      assert.match((await throughput.locator(".project-throughput-metrics dd").first().textContent())!, /\d h$/);
      assert.equal(await assemblyCycle.textContent(), await throughput.locator(".project-throughput-metrics dd").first().textContent());
      assert.equal(await releaseReadiness.getAttribute("value"), await readiness.getAttribute("value"));
      assert.match((await throughput.getByRole("listitem").last().textContent())!, /[2-9] tasks merged/);
      await simulation.press("Enter");
      await playback.getByRole("button", { name: "Reset simulation" }).click();
      await page.keyboard.press("Escape");
      assert.ok(await simulation.evaluate((button) => button === document.activeElement));
      await gates.click();

      await page.getByRole("button", { name: "Review T-A118" }).click();
      assert.equal(await feed.getAttribute("aria-selected"), "true");
      await orderedRelease.waitFor();
      const decision = page.locator("#activity-triage-in2");
      assert.ok(await decision.evaluate((element) => element === document.activeElement));
      assert.equal(await page.locator("#activity-triage-in1").count(), 0);
      await viewSettings.getByRole("button", { name: "Filter activity" }).press("Enter");
      const filterMenu = page.getByRole("menu", { name: "Filter activity" });
      assert.equal(await filterMenu.getByRole("group", { name: "Project", exact: true }).count(), 0);
      await filterMenu.getByRole("menuitemradio", { name: "Decisions", exact: true }).press("Enter");
      await feed.press("ArrowLeft");
      await readiness.waitFor();
      await projectHeader.getByRole("link", { name: "Overview", exact: true }).click();
      await page.waitForURL(address => address.hash === "#/projects/office-federation");
      await page.locator(".project-header + #project-release-summary + .grid2").waitFor();
      assert.equal(await releaseReadiness.getAttribute("max"), "4");
      await release.getByRole("link", { name: /File access ruling/ }).waitFor();
      await projectHeader.getByRole("link", { name: "Activity", exact: true }).click();
      await readiness.waitFor();
    }

    if (route === "projects/agency-v1/insights") {
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

    await page.evaluate(() => document.fonts.ready.then(() => undefined));
    const screenshot = resolve(import.meta.dir, "../.playwright", `poc-${route.replace(/[^a-z0-9]+/gi, "-")}-${theme}.png`);
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
    await browser.close();
  }
} finally {
  await server.stop(true);
}
