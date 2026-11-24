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

    if (route === "work") {
      const display = page.getByRole("button", { name: "Work display" });
      const group = page.getByRole("group", { name: "Group by", exact: true });
      const subgroup = page.getByRole("group", { name: "Subgroup by", exact: true });
      const groups = page.locator("#work-results .work-group");
      await display.press("Enter");
      await group.getByRole("menuitemradio", { name: "Project" }).press("Enter");
      await groups.first().waitFor();
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
    }
    if (route === "activity") {
      await page.getByRole("heading", { name: "Portfolio Assembly" }).waitFor();
      const gates = page.getByRole("button", { name: "Release gates" });
      const panel = page.getByRole("complementary", { name: "Release operations" });
      const activity = page.locator(".activity-main");
      const fullWidth = (await activity.boundingBox())!.width;
      await gates.press("Enter");
      await panel.waitFor();
      assert.equal(await gates.getAttribute("aria-expanded"), "true");
      const bounds = (await panel.boundingBox())!;
      const header = (await page.locator(".activity-header").boundingBox())!;
      const body = (await page.locator(".activity-body").boundingBox())!;
      assert.ok(Math.abs(bounds.y - header.y - header.height) <= 1, "Panel starts below the Activity header");
      assert.ok(Math.abs(bounds.x + bounds.width - body.x - body.width) <= 1, "Panel docks to the right edge");
      assert.ok(Math.abs(bounds.height - body.height) <= 1, "Panel fits the available height");
      assert.ok((await activity.boundingBox())!.width < fullWidth, "Activity stays visible next to the panel");
      await page.keyboard.press("Escape");
      await panel.waitFor({ state: "hidden" });
      assert.equal((await activity.boundingBox())!.width, fullWidth);
      assert.ok(await gates.evaluate((button) => button === document.activeElement), "Focus returns to Release gates");
    }

    await page.evaluate(() => document.fonts.ready.then(() => undefined));
    const screenshot = resolve(import.meta.dir, "../.playwright", `poc-${route.replace(/[^a-z0-9]+/gi, "-")}-${theme}.png`);
    await page.screenshot({ path: screenshot });
    assert.deepEqual(errors, [], "No browser errors");
    console.log(`Passed ${route} (${theme})\nScreenshot ${screenshot}`);
  } finally {
    await browser.close();
  }
} finally {
  await server.stop(true);
}
