import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function openOffices(page: Page) {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await page.getByRole('navigation', { name: 'Main', exact: true }).getByRole('link', { name: 'Offices', exact: true }).click();
	await expect(page.getByRole('heading', { name: 'Offices', exact: true })).toBeVisible();
}

for (const width of [960, 1440]) {
	test(`BUG-52: the full navigation remains at ${width}px`, async ({ page }) => {
		await openOffices(page);
		await page.setViewportSize({ width, height: 900 });
		const nav = page.getByRole('navigation', { name: 'Main', exact: true });
		expect((await nav.locator('..').boundingBox())?.width).toBe(288);
		await expect(nav.getByRole('link', { name: 'Offices', exact: true })).toHaveAttribute('aria-current', 'page');
		await expect(page.getByRole('button', { name: 'Menu', exact: true })).toHaveCount(0);
		await shot(page, `shell-full-${width}`);
	});
}

for (const width of [959, 720]) {
	test(`BUG-52: the icon rail keeps navigation names and tooltips at ${width}px`, async ({ page }) => {
		await openOffices(page);
		await page.setViewportSize({ width, height: width === 720 ? 450 : 900 });
		const nav = page.getByRole('navigation', { name: 'Main', exact: true });
		await shot(page, `shell-rail-${width}`);
		expect((await nav.locator('..').boundingBox())?.width).toBe(64);
		await expect(nav.getByRole('link', { name: 'Offices', exact: true })).toHaveAttribute('aria-current', 'page');
		for (const name of ['Chat', 'Agents', 'Org chart', 'Projects', 'Work', 'Sessions', 'Activity', 'Processes', 'Offices', 'Settings']) {
			await expect(nav.getByRole('link', { name, exact: true })).toBeVisible();
		}
		await expect(page.getByRole('link', { name: /^Switch organization/ }).locator('svg')).toBeVisible();
		const office = nav.getByRole('link', { name: 'Offices', exact: true });
		await office.hover();
		await expect(page.getByRole('tooltip')).toHaveText('Offices');
		await shot(page, `shell-rail-hover-${width}`);
		await office.focus();
		await page.keyboard.press('Tab');
		await expect(nav.getByRole('link', { name: 'Settings', exact: true })).toBeFocused();
		await expect(page.getByRole('tooltip')).toHaveText('Settings');
		await shot(page, `shell-rail-focus-${width}`);
	});
}

for (const width of [639, 600, 320]) {
	test(`BUG-52: the mobile Menu traps and restores navigation focus at ${width}px`, async ({ page }) => {
		await openOffices(page);
		await page.setViewportSize({ width, height: 900 });
		const menu = page.getByRole('button', { name: 'Menu', exact: true });
		await shot(page, `shell-mobile-closed-${width}`);
		await expect(menu).toBeVisible();
		await expect(menu).toHaveAttribute('aria-expanded', 'false');
		await expect(menu).toHaveAttribute('aria-controls', /\S+/);
		expect(await page.getByRole('main').evaluate((el) => {
			const css = getComputedStyle(el);
			return el.clientWidth - parseFloat(css.paddingLeft) - parseFloat(css.paddingRight);
		})).toBeGreaterThanOrEqual(200);
		expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
		expect((await page.locator('[data-shell-header]').boundingBox())?.height).toBe(56);
		await menu.click();
		const dialog = page.getByRole('dialog', { name: 'Main navigation', exact: true });
		await expect(dialog).toBeVisible();
		expect((await dialog.boundingBox())?.width).toBe(Math.min(288, width - 56));
		await expect(menu).toHaveAttribute('aria-expanded', 'true');
		await expect(dialog).toHaveAttribute('id', (await menu.getAttribute('aria-controls'))!);
		await expect(dialog.getByRole('link', { name: 'Offices', exact: true })).toBeFocused();
		await shot(page, `shell-mobile-open-${width}`);
		const focusables = await dialog.locator('a[href], button:not([disabled])').count();
		for (let i = 0; i < focusables + 2; i++) {
			await page.keyboard.press('Tab');
			expect(await dialog.evaluate((el) => el.contains(document.activeElement))).toBe(true);
		}
		await page.keyboard.press('Escape');
		await expect(dialog).not.toBeVisible();
		await expect(menu).toBeFocused();
		await menu.click();
		await dialog.getByRole('link', { name: 'Settings', exact: true }).click();
		await expect(page).toHaveURL(/\/settings$/);
		await expect(dialog).not.toBeVisible();
		await expect(page.getByRole('heading', { level: 1, name: 'Settings', exact: true })).toBeFocused();
		await expect(page.getByRole('main')).toHaveCount(1);
		await expect(menu).toHaveAttribute('aria-expanded', 'false');
		await shot(page, `shell-mobile-destination-${width}`);
	});
}

test('BUG-52: the scrim dismisses Menu and widening the window releases the focus trap', async ({ page }) => {
	await openOffices(page);
	await page.setViewportSize({ width: 600, height: 900 });
	const menu = page.getByRole('button', { name: 'Menu', exact: true });
	await menu.click();
	const dialog = page.getByRole('dialog', { name: 'Main navigation', exact: true });
	await expect(dialog).toBeVisible();
	await page.locator('[data-slot="sheet-overlay"]').click({ position: { x: 590, y: 120 } });
	await expect(dialog).not.toBeVisible();
	await expect(menu).toBeFocused();
	await menu.click();
	await expect(dialog).toBeVisible();
	await page.setViewportSize({ width: 720, height: 900 });
	await expect(dialog).not.toBeVisible();
	await expect(page.getByRole('navigation', { name: 'Main', exact: true }).getByRole('link', { name: 'Offices', exact: true })).toBeFocused();
	await expect(menu).toHaveCount(0);
	await shot(page, 'shell-menu-resized-to-rail');
});
