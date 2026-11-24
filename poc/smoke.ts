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
      await simulation.hover();
      await simulation.locator(".activity-tooltip").waitFor();
      await simulation.press("Enter");
      await panel.getByRole("heading", { name: "Simulation", exact: true }).waitFor();
      await panel.getByRole("button", { name: "Reset simulation" }).hover();
      await panel.getByRole("button", { name: "Reset simulation" }).locator(".activity-tooltip").waitFor();
      await panel.getByRole("button", { name: "Reset simulation" }).click();
      await panel.getByRole("button", { name: "8×" }).click();
      await panel.getByRole("button", { name: "Add sample task" }).click();
      await panel.getByText("joined the queue").waitFor();
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
