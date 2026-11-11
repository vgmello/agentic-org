import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function openOffice(page: Page) {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Topbar reference');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, '/offices');
	await expect(page.getByTestId('office-row')).toHaveCount(1);
}

for (const width of [720, 960]) {
	test(`BUG-70 chrome: the heading follows prototype spacing at ${width}px`, async ({ page }) => {
		await page.setViewportSize({ width, height: 900 });
		await openOffice(page);
		const header = page.locator('[data-shell-header]');
		const heading = header.getByRole('heading', { name: 'Offices', exact: true });
		await expect(heading).toBeVisible();
		await shot(page, `bug70-chrome-before-${width}`);
		const box = await header.boundingBox(), title = await heading.boundingBox();
		expect(box?.height).toBe(64);
		expect(title!.x - box!.x).toBe(24);
		expect(await header.locator('input, a[href]').count()).toBe(0);
	});
}

test('BUG-70 topbar: the current mobile Menu and page heading remain accessible', async ({ page }) => {
	await page.setViewportSize({ width: 320, height: 600 });
	await openOffice(page);
	const header = page.locator('[data-shell-header]');
	await expect(header.getByRole('button', { name: 'Menu', exact: true })).toBeVisible();
	await expect(header.getByRole('heading', { name: 'Offices', exact: true })).toBeVisible();
	const bounds = await header.boundingBox();
	expect(bounds?.height).toBe(56);
	expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(320);
	await header.getByRole('button', { name: 'Menu', exact: true }).press('Enter');
	await expect(page.getByRole('dialog', { name: 'Main navigation' })).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(header.getByRole('button', { name: 'Menu', exact: true })).toBeFocused();
	await shot(page, 'bug70-mobile-before-header');
});

for (const width of [320, 720]) {
	test(`BUG-70 chrome: route navigation retains the heading id and focus at ${width}px`, async ({ page }) => {
		await page.setViewportSize({ width, height: 600 });
		await openOffice(page);
		if (width < 640) {
			await page.getByRole('button', { name: 'Menu', exact: true }).press('Enter');
			await page.getByRole('dialog', { name: 'Main navigation' }).getByRole('link', { name: 'Agents', exact: true }).press('Enter');
		} else await page.getByRole('link', { name: 'Agents', exact: true }).press('Enter');
		await expect(page).toHaveURL(/\/agents$/);
		const heading = page.getByRole('heading', { name: 'Agents', exact: true });
		await expect(heading).toHaveAttribute('id', 'shell-page-title');
		await shot(page, `bug70-heading-focus-before-${width}`);
		await expect(heading).toBeFocused({ timeout: 3000 });
	});
}
