/* Person side panel checks (TASK-238), run from smoke.ts --all on the Home route. */
import assert from "node:assert/strict";
import type { Page } from "../agentic-org-ui/node_modules/playwright";

const THEMES = ["ocean", "classic", "report", "report-dark", "graphite", "graphite-light", "cozy"];
const panel = (page: Page) => page.locator("#person-panel");
const navTo = (page: Page, hash: string) => page.evaluate((h) => { location.hash = h; }, hash);
const activePerson = (page: Page) => page.evaluate(() => document.activeElement?.getAttribute("data-person") ?? null);
const isModal = (page: Page) => panel(page).evaluate((el) => el.matches(":modal"));

async function openFrom(page: Page, selector: string) {
  const trigger = page.locator(selector).first();
  await trigger.waitFor();
  await trigger.click();
  await page.locator("#person-panel[open]").waitFor();
  return trigger;
}

async function showBoard(page: Page) {
  await navTo(page, "#/work");
  await page.getByRole("group", { name: "Work layout" }).getByRole("button", { name: "Board" }).click();
  await page.getByRole("region", { name: "Work board" }).waitFor();
}

/* A board name opens the panel beside the page with name, role, kind, status and team; Esc returns focus */
async function checkFromBoard(page: Page) {
  await showBoard(page);
  await openFrom(page, '.bcard a[data-person="kai"]');
  const p = panel(page);
  assert.equal(await page.locator("#person-panel-title").innerText(), "Kai");
  assert.equal(await p.getAttribute("aria-labelledby"), "person-panel-title");
  assert.equal(await p.getAttribute("role") ?? "dialog", "dialog", "A <dialog> is exposed as a dialog");
  assert.match(await p.locator(".pp-role").innerText(), /Rust Developer\s*Agent/, "Role and kind word");
  assert.match(await p.locator(".pp-status").innerText(), /With you\s*Working on Codec split 2a\.3\.2b/, "One status line");
  assert.equal(await p.locator(".pp-team").innerText(), "Platform (Rust)", "Team");
  assert.equal(await isModal(page), false, "Non-modal beside the page on desktop");
  assert.equal(await page.evaluate(() => document.activeElement?.id), "person-panel-title", "Focus moves into the panel");
  const box = await p.boundingBox();
  assert.ok(box && box.width >= 380 && box.width <= 420, `Panel is ~400px wide (${box?.width})`);
  assert.equal(await p.getByText(/model|runtime|\$\d/i).count(), 0, "No runtime, model or cost");
  await page.keyboard.press("Escape");
  await p.waitFor({ state: "hidden" });
  assert.equal(await activePerson(page), "kai", "Focus returns to the name that opened it");
  assert.equal(new URL(page.url()).hash, "#/work", "The page stays where it was");
}

/* Agents list their agent-to-agent conversations, each linked; current work is capped at three and linked */
async function checkAgentLists(page: Page) {
  await openFrom(page, '.bcard a[data-person="rowan"]');
  const p = panel(page);
  const a2a = await p.locator(".pp-a2a").evaluateAll((els) => els.map((e) => e.getAttribute("href")));
  assert.deepEqual(a2a, ["#/members/rowan/conversations/a2a-kai-rowan", "#/members/rowan/conversations/a2a-rowan-sage", "#/members/rowan/conversations/a2a-ada-rowan"], "Rowan's A2A conversations, newest first");
  const work = p.locator('section[aria-labelledby="pp-work-h"] a.pp-row');
  assert.ok(await work.count() >= 1 && await work.count() <= 3, "Up to three open items");
  assert.match((await work.first().getAttribute("href"))!, /^#\/work\?item=T-/);
  await p.locator(".pp-a2a").first().click();
  /* Owner (2026-11-27): agent-to-agent conversations live on the agent's page, not in Chat */
  await page.waitForURL((u) => u.hash === "#/members/rowan/conversations/a2a-kai-rowan");
  await p.waitFor({ state: "hidden" });
}

/* Message opens the existing DM */
async function checkMessage(page: Page) {
  await showBoard(page);
  await openFrom(page, '.bcard a[data-person="kai"]');
  await panel(page).getByRole("button", { name: "Message" }).click();
  await page.waitForURL((u) => u.hash === "#/chat/dm-kai");
  await page.locator("#ch-title", { hasText: /^Kai$/ }).waitFor();
  await panel(page).waitFor({ state: "hidden" });
}

/* Inbox From and a Chat author open the same panel; a person shows no A2A list */
async function checkInboxAndChat(page: Page) {
  await navTo(page, "#/inbox/in1");
  const from = await openFrom(page, '.triage-from a[data-person]');
  const who = await from.getAttribute("data-person");
  assert.equal(await panel(page).getAttribute("data-person"), who, "Inbox From opens its sender");
  await panel(page).getByRole("button", { name: "Close" }).click();
  await panel(page).waitFor({ state: "hidden" });
  assert.equal(await activePerson(page), who, "Close returns focus too");
  await navTo(page, "#/chat/agency-v1");
  const author = await openFrom(page, ".msg .hd a[data-person]");
  assert.equal(await panel(page).getAttribute("data-person"), await author.getAttribute("data-person"), "Chat author opens its member");
  await page.keyboard.press("Escape");
  await navTo(page, "#/teams/members");
  await openFrom(page, '.rows a[data-person="maya"]');
  const p = panel(page);
  assert.match(await p.locator(".pp-role").innerText(), /Operations\s*Person/);
  assert.match(await p.locator(".pp-status").innerText(), /Offline/);
  assert.equal(await p.locator(".pp-a2a, #pp-a2a-h").count(), 0, "People have no A2A list");
  await page.keyboard.press("Escape");
}

/* Narrow screens: modal, backdrop click closes; phones: full screen */
async function checkNarrow(page: Page) {
  await page.setViewportSize({ width: 800, height: 900 });
  await navTo(page, "#/teams/members");
  await openFrom(page, '.rows a[data-person="ada"]');
  assert.equal(await isModal(page), true, "Modal on narrow screens");
  await page.mouse.click(60, 450);
  await panel(page).waitFor({ state: "hidden" });
  await page.setViewportSize({ width: 375, height: 740 });
  await openFrom(page, '.rows a[data-person="ada"]');
  const box = await panel(page).boundingBox();
  assert.equal(Math.round(box!.width), 375, "Full screen on phones");
  await page.keyboard.press("Escape");
  await page.setViewportSize({ width: 1440, height: 900 });
}

function contrastOf(el: Element) {
  const rgb = (c: string) => (c.match(/[\d.]+/g) || []).slice(0, 3).map(Number);
  const lum = (c: number[]) => {
    const [r, g, b] = c.map((v) => { const s = v / 255; return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4; });
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  };
  const bg = lum(rgb(getComputedStyle(document.getElementById("person-panel")!).backgroundColor));
  const fg = lum(rgb(getComputedStyle(el).color));
  return (Math.max(bg, fg) + 0.05) / (Math.min(bg, fg) + 0.05);
}

/* Every theme: the panel has an opaque surface and readable text */
async function checkThemes(page: Page, baseUrl: string) {
  for (const theme of THEMES) {
    const u = new URL(baseUrl);
    u.searchParams.set("theme", theme);
    u.hash = "/teams/members";
    await page.goto(u.href);
    await openFrom(page, '.rows a[data-person="kai"]');
    const alpha = await panel(page).evaluate((el) => getComputedStyle(el).backgroundColor);
    assert.ok(!/rgba\(.*, 0\)$|transparent/.test(alpha), `${theme}: the panel has a surface (${alpha})`);
    for (const sel of ["#person-panel-title", ".pp-role", ".pp-h", ".pp-row-meta"]) {
      const ratio = await panel(page).locator(sel).first().evaluate(contrastOf);
      assert.ok(ratio >= 4.5, `${theme}: ${sel} contrast ${ratio.toFixed(2)} >= 4.5`);
    }
    await page.keyboard.press("Escape");
  }
}

export async function checkPersonPanel(page: Page, href: string) {
  await checkFromBoard(page);
  await checkAgentLists(page);
  await checkMessage(page);
  await checkInboxAndChat(page);
  await checkNarrow(page);
  await checkThemes(page, href);
}
