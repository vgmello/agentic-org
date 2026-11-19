import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';
import { enterMockedOrganization } from './mock-organization';

// The main navigation in app-shell-ux §0's order. Inbox shows because the mocked office serves its count.
const SECTIONS = [
	['Home', '/home'],
	['Inbox', '/inbox'],
	['Projects', '/projects'],
	['Work', '/work'],
	['Chat', '/chat'],
	['Activity', '/activity'],
	['Teams', '/teams'],
	['Organization settings', '/settings']
] as const;
const JOURNEY_SECTIONS = ['Chat', 'Teams', 'Projects', 'Work', 'Organization settings'];
const slug = (path: string) => path.slice(1);
/** The destinations themselves, not the Projects and Teams places listed under them. */
const destinations = (page: Page) =>
	page.getByRole('navigation', { name: 'Main' }).locator(':scope > ul > li > a');

test.beforeEach(async ({ page }) => {
	await enterMockedOrganization(page);
	await expect(page).toHaveURL(/\/teams\/members$/);
});

test('the sidebar lists every section, with its icon', { tag: '@journey' }, async ({ page }) => {
	await expect(destinations(page)).toHaveCount(SECTIONS.length);
	for (const [index, [name]] of SECTIONS.entries())
		await expect(destinations(page).nth(index)).toHaveAccessibleName(name);
	await expect(destinations(page).locator('svg')).toHaveCount(SECTIONS.length);
});

for (const [name, path] of SECTIONS) {
	test(
		`${name} opens its page and is marked current`,
		{ tag: JOURNEY_SECTIONS.includes(name) ? '@journey' : [] },
		async ({ page }) => {
			const link = destinations(page).and(page.getByRole('link', { name, exact: true }));
			await link.click();
			await expect(page).toHaveURL(new RegExp(`${path}$`));
			await expect(link).toHaveAttribute('aria-current', 'page');
			await expect(page.getByRole('heading', { level: 1, name, exact: true })).toBeVisible();
			await shot(page, `nav-${slug(path)}`);
		}
	);
}

test('an unknown section is a 404, not a blank page', async ({ page }) => {
	await visit(page, '/nope');
	await expect(page.getByText('Something went wrong')).toBeVisible();
});
