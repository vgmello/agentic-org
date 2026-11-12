import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function openOffices(page: Page) {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Heading focus');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, '/offices');
}
async function sectionLink(page: Page, width: number, name: string) {
	if (width < 640) {
		await page.getByRole('button', { name: 'Menu', exact: true }).press('Enter');
		return page.getByRole('dialog', { name: 'Main navigation' }).getByRole('link', { name, exact: true });
	}
	return page.getByRole('navigation', { name: 'Main', exact: true }).getByRole('link', { name, exact: true });
}

for (const width of [320, 640, 720, 960, 1280]) {
	for (const textScale of [1, 2]) {
		test(`BUG-74: section selection lands on the destination heading at ${width}px and ${textScale * 100}% text`, { tag: '@webkit' }, async ({ page }) => {
			await page.setViewportSize({ width, height: 900 });
			await openOffices(page);
			await page.evaluate((scale) => { document.documentElement.style.fontSize = `${scale * 100}%`; }, textScale);
			await (await sectionLink(page, width, 'Agents')).press('Enter');
			await expect(page).toHaveURL(/\/agents$/);
			const heading = page.locator('#shell-page-title');
			await expect(heading).toHaveText('Agents');
			await expect(heading).toBeFocused();
			await shot(page, `bug74-heading-landing-${width}-text${textScale * 100}`, { context: page.locator('[data-shell-header]') });
		});
	}
}

for (const width of [320, 720, 1280]) {
	test(`BUG-74: reselecting the current section focuses its heading without history at ${width}px`, { tag: '@webkit' }, async ({ page }) => {
		await page.setViewportSize({ width, height: 900 });
		await openOffices(page);
		const before = await page.evaluate(() => history.length);
		const link = await sectionLink(page, width, 'Offices');
		await link.focus();
		await expect(link).toBeFocused();
		await link.press('Enter');
		await expect(page.locator('#shell-page-title')).toBeFocused();
		expect(await page.evaluate(() => history.length)).toBe(before);
		await expect(page).toHaveURL(/\/offices$/);
		await shot(page, `bug74-reselect-${width}`);
	});
}

for (const width of [720, 1280]) {
	test(`BUG-74: a pointer selection focuses the new title at ${width}px`, { tag: '@webkit' }, async ({ page }) => {
		await page.setViewportSize({ width, height: 900 });
		await openOffices(page);
		await (await sectionLink(page, width, 'Agents')).click();
		await expect(page).toHaveURL(/\/agents$/);
		await expect(page.locator('#shell-page-title')).toBeFocused();
		await shot(page, `bug74-pointer-${width}`);
	});
}

test('BUG-74: desktop navigation and Back preserve the retained Chat thread draft', { tag: ['@journey', '@webkit'] }, async ({ page }) => {
	await page.setViewportSize({ width: 1280, height: 900 });
	await openOffices(page);
	await visit(page, '/chat?c=c-release-v1');
	await page.getByTestId('replies-link').first().click();
	const composer = page.getByTestId('thread-panel').getByTestId('composer');
	await expect(composer).toBeVisible();
	const threadUrl = page.url();
	await composer.fill('Keep this desktop thread draft');
	await (await sectionLink(page, 1280, 'Agents')).press('Enter');
	await expect(page.locator('#shell-page-title')).toHaveText('Agents');
	await expect(page.locator('#shell-page-title')).toBeFocused();
	await page.goBack();
	await expect(page).toHaveURL(threadUrl);
	await expect(composer).toHaveValue('Keep this desktop thread draft');
	await expect(page.locator('#shell-page-title')).not.toBeFocused();
	await shot(page, 'bug74-desktop-chat-back-thread-draft');
});

test('BUG-74: Switch organization keeps its non-shell navigation', { tag: ['@journey', '@webkit'] }, async ({ page }) => {
	await page.setViewportSize({ width: 1280, height: 900 });
	await openOffices(page);
	await page.getByRole('link', { name: /^Switch organization/ }).click();
	await expect(page).toHaveURL(/\/welcome$/);
	await expect(page.locator('#shell-page-title')).toHaveCount(0);
	await expect(page.getByRole('heading', { name: 'Welcome to Agentic Org', exact: true })).toBeVisible();
	await shot(page, 'bug74-switch-organization');
});
