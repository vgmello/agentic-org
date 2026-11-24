import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';
import { enterMockedOrganization } from './mock-organization';

// The ExecutiveSidebar main navigation in group order (Focus, Workspace, Organization).
const SECTIONS = [
	['Home', '/home', 1, null],
	['Inbox', '/inbox', 1, 'Inbox'],
	['Work', '/work', 1, 'Work'],
	['Chat', '/chat', 2, 'Channels'],
	['Activity', '/activity', 1, 'Activity'],
	['Projects', '/projects', 2, 'Projects'],
	['Teams', '/teams', 2, 'Teams'],
	['Org chart', '/teams/org-chart', 2, 'Org chart'],
	['Members', '/teams/members', 2, 'Members'],
	['Processes', '/settings/processes', 2, 'Processes'],
	['Offices', '/settings/offices', 2, 'Offices'],
	['Settings', '/settings', 2, 'General']
] as const;
const JOURNEY_SECTIONS = ['Chat', 'Teams', 'Projects', 'Work', 'Settings'];
const slug = (path: string) => path.slice(1).replaceAll('/', '-');
/** The primary destinations in ExecutiveSidebar, excluding pinned Favorite projects. */
const destinations = (page: Page) =>
	page.getByRole('navigation', { name: 'Main' }).locator('ul:not(#exec-nav-favs) > li > a');

test.beforeEach(async ({ page }) => {
	await enterMockedOrganization(page);
	await expect(page).toHaveURL(/\/teams\/members$/);
});

test('the sidebar lists every section, with its icon', { tag: '@journey' }, async ({ page }) => {
	await expect(destinations(page)).toHaveCount(SECTIONS.length);
	for (const [index, [name]] of SECTIONS.entries()) {
		const expected = name === 'Inbox' ? /^Inbox/ : name;
		await expect(destinations(page).nth(index)).toHaveAccessibleName(expected);
	}
	await expect(destinations(page).locator('svg')).toHaveCount(SECTIONS.length);
});

for (const [name, path, level, heading] of SECTIONS) {
	test(
		`${name} opens its page and is marked current`,
		{ tag: JOURNEY_SECTIONS.includes(name) ? '@journey' : [] },
		async ({ page }) => {
			const link = destinations(page).and(
				page.getByRole('link', {
					name: name === 'Inbox' ? /^Inbox/ : name,
					exact: name !== 'Inbox'
				})
			);
			await link.click();
			await expect(page).toHaveURL(new RegExp(`${path}$`));
			await expect(link).toHaveAttribute('aria-current', 'page');
			const main = page.getByRole('main');
			if (heading === null) {
				await expect(main.getByRole('heading', { level })).toBeVisible();
			} else {
				await expect(
					main.getByRole('heading', { level, name: new RegExp(`^${heading}`) }).first()
				).toBeVisible();
			}
			await shot(page, `nav-${slug(path)}`);
		}
	);
}

test('an unknown section is a 404, not a blank page', async ({ page }) => {
	await visit(page, '/nope');
	await expect(page.getByText('Something went wrong')).toBeVisible();
});
