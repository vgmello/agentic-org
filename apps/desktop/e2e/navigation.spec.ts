import { expect, test } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

const SECTIONS = ['Agents', 'Org chart', 'Projects', 'Work', 'Sessions', 'Activity', 'Offices', 'Settings'];
const slug = (name: string) => name.toLowerCase().replace(' ', '-');

test.beforeEach(async ({ page }) => {
	await visit(page, '/create');
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
		await expect(page).toHaveURL(new RegExp(`/${slug(name)}$`));
		await expect(link).toHaveAttribute('aria-current', 'page');
		await expect(page.getByRole('heading', { level: 1, name })).toBeVisible();
		await shot(page, `nav-${slug(name)}`);
	});
}

test('an unknown section is a 404, not a blank page', async ({ page }) => {
	await visit(page, '/nope');
	await expect(page.getByText('Something went wrong')).toBeVisible();
});
