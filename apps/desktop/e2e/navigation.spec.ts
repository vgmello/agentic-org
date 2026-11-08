import { expect, test } from '@playwright/test';
import { shot } from './shot';

const SECTIONS = ['Agents', 'Projects', 'Tasks', 'Sessions', 'Activity', 'Offices', 'Settings'];

test.beforeEach(async ({ page }) => {
	await page.goto('/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
});

test('the sidebar lists every section, with its icon', async ({ page }) => {
	const links = page
		.getByRole('navigation', { name: 'Main' })
		.getByRole('link')
		.filter({ hasNotText: 'Switch organization' });
	await expect(links).toHaveText(SECTIONS);
	await expect(links.locator('svg')).toHaveCount(SECTIONS.length);
});

for (const name of SECTIONS) {
	test(`${name} opens its page and is marked current`, async ({ page }) => {
		const link = page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name });
		await link.click();
		await expect(page).toHaveURL(new RegExp(`/${name.toLowerCase()}$`));
		await expect(link).toHaveAttribute('aria-current', 'page');
		await expect(page.getByRole('heading', { level: 1, name })).toBeVisible();
		await shot(page, `nav-${name.toLowerCase()}`);
	});
}

test('an unknown section is a 404, not a blank page', async ({ page }) => {
	await page.goto('/nope');
	await expect(page.getByText('Something went wrong')).toBeVisible();
});
