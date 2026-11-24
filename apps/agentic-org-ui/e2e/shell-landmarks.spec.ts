import { expect, test } from '@playwright/test';
import { enterMockedOrganization } from './mock-organization';
import { visit } from './ready';
import { shot } from './shot';

test(
	'BUG-19: retained sections have one main landmark while navigating the shell',
	{ tag: '@journey' },
	async ({ page }) => {
		await visit(page, '/create');
		await expect(page.getByRole('main')).toHaveCount(1);
		await enterMockedOrganization(page);
		await expect(page).toHaveURL(/\/teams\/members$/);
		const sections = [
			['Home', 'home', 1, null],
			['Inbox', 'inbox', 1, 'Inbox'],
			['Projects', 'projects', 2, 'Projects'],
			['Work', 'work', 1, 'Work'],
			['Chat', 'chat', 2, 'Needs you'],
			['Activity', 'activity', 1, 'Activity'],
			['Teams', 'teams', 2, 'Teams'],
			['Settings', 'settings', 2, 'General']
		] as const;
		for (const [name, path, level, heading] of sections) {
			await page
				.getByRole('navigation', { name: 'Main', exact: true })
				.getByRole('link', { name: name === 'Inbox' ? /^Inbox/ : name, exact: name !== 'Inbox' })
				.click();
			await expect(page).toHaveURL(new RegExp(`/${path}$`));
			const main = page.getByRole('main');
			await expect(main).toHaveCount(1);
			if (heading === null) {
				await expect(main.getByRole('heading', { level })).toBeVisible();
			} else {
				await expect(
					main.getByRole('heading', { level, name: new RegExp(`^${heading}`) }).first()
				).toBeVisible();
			}
			await expect(main.getByRole('main')).toHaveCount(0);
			await shot(page, `shell-main-${path}`);
		}
	}
);
