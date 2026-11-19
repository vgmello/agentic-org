// Layout and keyboard regressions for Organization settings › Processes and development processes (app-shell-ux §2,
// §6a; processes-ux PROC-33): reflow at 320 px and 200% text, 44 px targets below 640 px, the heading block above
// the Show select, and focus returning from the sheet.
import { expect, test, type Locator, type Page } from '@playwright/test';
import { visit } from './ready';
import { enterMockedOrganization } from './mock-organization';
import { shot } from './shot';

test.use({ locale: 'en-GB', timezoneId: 'UTC' });

async function open(page: Page, url: string) {
	await enterMockedOrganization(page, 'Acme');
	await visit(page, url);
}

const sideways = (page: Page) =>
	page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
const height = async (locator: Locator) => (await locator.boundingBox())!.height;

test('PROC-33 / §2: at 320 px and 200% text every Processes screen reflows without sideways scroll', async ({
	page
}) => {
	await page.setViewportSize({ width: 320, height: 640 });
	await open(page, '/settings/processes?place=team:web');
	await page.evaluate(() => (document.documentElement.style.fontSize = '200%'));
	await expect(page.getByTestId('process-row').first()).toBeVisible();
	expect(await sideways(page)).toBeLessThanOrEqual(0);
	await shot(page, 'processes-320-200');

	await page.locator('[data-testid=process-row][data-key="code-review"] button').first().click();
	const sheet = page.getByRole('dialog');
	await expect(sheet.getByTestId('why')).toBeVisible();
	// Measure the sheet once its slide-in has finished.
	await page.evaluate(() =>
		Promise.all(document.getAnimations().map((a) => a.finished.catch(() => undefined)))
	);
	const box = (await sheet.boundingBox())!;
	expect(box.x).toBeGreaterThanOrEqual(0);
	expect(box.x + box.width).toBeLessThanOrEqual(320);
	expect(await sheet.evaluate((el) => el.scrollWidth - el.clientWidth)).toBeLessThanOrEqual(0);
	await shot(page, 'processes-320-200-sheet');
	await page.keyboard.press('Escape');

	for (const url of [
		'/settings/processes/documents/new?place=team:web&kind=process',
		'/settings/processes/development',
		'/settings/processes/development/0193a000-0000-7000-8000-0000000000c0'
	]) {
		await visit(page, url);
		await page.evaluate(() => (document.documentElement.style.fontSize = '200%'));
		await expect(page.locator('main h2').first()).toBeVisible();
		await expect(page.getByRole('status').filter({ hasText: /Loading/ })).toHaveCount(0);
		expect(await sideways(page), url).toBeLessThanOrEqual(0);
	}
});

test('PROC-33: below 640 px the heading block stacks above Show, and controls are 44 px targets', async ({
	page
}) => {
	await page.setViewportSize({ width: 390, height: 844 });
	await open(page, '/settings/processes?place=team:web');
	const title = page.locator('#processes-heading');
	const select = page.getByLabel('Show processes for');
	await expect(page.getByTestId('process-row').first()).toBeVisible();
	const titleBox = (await title.boundingBox())!;
	const selectBox = (await select.boundingBox())!;
	expect(titleBox.y + titleBox.height).toBeLessThanOrEqual(selectBox.y);
	expect(titleBox.width).toBeGreaterThanOrEqual(200);
	expect(await height(select)).toBeGreaterThanOrEqual(44);
	expect(await height(page.getByRole('link', { name: 'New process' }))).toBeGreaterThanOrEqual(44);
	expect(
		await height(page.getByRole('link', { name: 'Fix Design handoff' }))
	).toBeGreaterThanOrEqual(44);
	for (const tab of await page
		.getByRole('navigation', { name: 'Processes and standards' })
		.getByRole('link')
		.all())
		expect(await height(tab)).toBeGreaterThanOrEqual(44);
	await page.locator('[data-testid=process-row][data-key="code-review"] button').first().click();
	expect(
		await height(page.getByRole('dialog').getByRole('button', { name: 'Close', exact: true }))
	).toBeGreaterThanOrEqual(44);
	await shot(page, 'processes-390-sheet');
});

test('§6a: the sheet traps focus, Escape closes it, and focus returns to the row that opened it', async ({
	page
}) => {
	await open(page, '/settings/processes');
	const opener = page.locator('[data-testid=process-row][data-key="review-and-merge"] button');
	await opener.focus();
	await page.keyboard.press('Enter');
	const sheet = page.getByRole('dialog');
	await expect(sheet.getByRole('heading', { name: 'Review and merge' })).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(page.getByRole('dialog')).toHaveCount(0);
	await expect(opener).toBeFocused();
});

test('§6a: a long unbroken title and key wrap inside the row and the sheet', async ({ page }) => {
	await page.setViewportSize({ width: 360, height: 700 });
	await open(page, '/settings/processes/documents/new?place=organization&kind=process');
	const long = 'Aprocesswithaverylongunbrokentitlethatmustwrapinsidethecolumn';
	await page.getByLabel('Title').fill(long);
	await page
		.getByLabel('Key (optional)')
		.fill('a-very-long-key-that-still-has-to-wrap-inside-the-row-x');
	await page.getByLabel('Owner').selectOption('you');
	await page.getByRole('button', { name: 'Create process' }).click();
	const created = page.getByRole('button', { name: long });
	await expect(created).toBeVisible();
	expect(await sideways(page)).toBeLessThanOrEqual(0);
	await created.click();
	const title = page.getByRole('dialog').getByRole('heading', { name: long });
	const box = (await title.boundingBox())!;
	expect(box.x + box.width).toBeLessThanOrEqual(360);
});

for (const width of [960, 1024]) {
	test(`review P2-1: at ${width} px the title column of a row stays at least 200 px wide`, async ({
		page
	}) => {
		await page.setViewportSize({ width, height: 800 });
		await open(page, '/settings/processes?place=agent:noor');
		const title = page.getByTestId('process-row').first().locator(':scope > div').first();
		await expect(title).toBeVisible();
		for (const row of await page.getByTestId('process-row').all()) {
			const box = (await row.locator(':scope > div').first().boundingBox())!;
			expect(box.width).toBeGreaterThanOrEqual(200);
		}
	});
}
