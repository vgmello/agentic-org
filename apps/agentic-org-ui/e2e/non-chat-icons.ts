import { expect, type Locator, type Page } from '@playwright/test';
import { visit } from './ready';
import { viewAction } from './org-chart-menu';
import icons from './fixtures/non-chat-icons-v2.2.0.json' with { type: 'json' };

export async function create(page: Page, width: number, scale = 1) {
	await page.setViewportSize({ width, height: 900 });
	await page.addInitScript((scale) => {
		const apply = () => document.documentElement.style.setProperty('font-size', `${scale * 100}%`, 'important');
		addEventListener('DOMContentLoaded', apply, { once: true });
		if (document.documentElement) apply();
	}, scale);
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization', exact: true }).click();
	await expect(page).toHaveURL(/\/agents$/);
	// URL changes before the destination is ready; avoid interrupting the create handoff (BUG-93).
	await expect(page.getByRole('main', { name: 'Agents', exact: true })).toBeVisible();
	await expect(page.getByRole('table', { name: 'Agents', exact: true })).toBeVisible();
}
export async function iconIs(icon: Locator, key: keyof typeof icons) {
	const expected = icons[key];
	await expect(icon).toHaveCount(1);
	for (const [name, value] of Object.entries(expected.attributes)) await expect(icon).toHaveAttribute(name, value);
	await expect(icon).toHaveAttribute('focusable', 'false');
	expect(await icon.locator('path').evaluateAll((paths) => paths.map((path) => Object.fromEntries([...path.attributes].map((a) => [a.name, a.value]))))).toEqual(expected.paths);
	const font = await icon.evaluate(() => parseFloat(getComputedStyle(document.documentElement).fontSize));
	await expect(icon).toHaveCSS('width', `${font}px`);
	await expect(icon).toHaveCSS('height', `${font}px`);
}
export async function removeIs(button: Locator, name: string) {
	await expect(button).toHaveAccessibleName(name);
	await expect(button).toHaveAttribute('title', name);
	await expect(button).toHaveText('');
	await iconIs(button.locator('svg'), 'mini:x-mark');
	const box = (await button.boundingBox())!;
	expect(box.width, `${name}: displayed target width`).toBeGreaterThanOrEqual(24);
	expect(box.height, `${name}: displayed target height`).toBeGreaterThanOrEqual(24);
}
export async function focusTarget(page: Page, button: Locator) {
	// A pointer click on Actual size leaves pointer modality; enter keyboard mode first.
	await page.keyboard.press('Tab');
	await button.evaluate((element) => element.scrollIntoView({ block: 'center', inline: 'center' }));
	await button.focus();
	await expect(button).toBeFocused();
	await expect.poll(() => button.evaluate((element) => {
		const r = element.getBoundingClientRect(), x = r.x + r.width / 2, y = r.y + r.height / 2;
		const hit = document.elementFromPoint(x, y);
		return x >= 0 && x < innerWidth && y >= 0 && y < innerHeight && !!hit && element.contains(hit);
	})).toBe(true);
}
export const team = (page: Page, id: string) => page.locator(`[data-testid=team-card][data-team=${id}]`);

/** The phone defaults to List; choose Chart through the same retained View menu. */
export async function showChart(page: Page) {
	if (page.viewportSize()!.width < 640) await viewAction(page, 'Chart');
	await expect(page.getByTestId('teams-chart')).toBeVisible();
}
