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
			['Home', 'home'],
			['Inbox', 'inbox'],
			['Projects', 'projects'],
			['Work', 'work'],
			['Chat', 'chat'],
			['Activity', 'activity'],
			['Teams', 'teams'],
			['Organization settings', 'settings']
		];
		for (const [name, path] of sections) {
			await page
				.getByRole('navigation', { name: 'Main', exact: true })
				.getByRole('link', { name, exact: true })
				.click();
			await expect(page).toHaveURL(new RegExp(`/${path}$`));
			const main = page.getByRole('main');
			await expect(main).toHaveCount(1);
			await expect(main.getByRole('heading', { level: 1, name, exact: true })).toBeVisible();
			await expect(main.getByRole('main')).toHaveCount(0);
			await shot(page, `shell-main-${path}`);
		}
	}
);
