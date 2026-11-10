import { expect, test } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

test('BUG-19: retained sections have one main landmark while navigating the shell', async ({ page }) => {
	await visit(page, '/create');
	await expect(page.getByRole('main')).toHaveCount(1);
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	const sections = [
		['Agents', 'agents'], ['Org chart', 'org-chart'], ['Projects', 'projects'], ['Work', 'work'],
		['Chat', 'chat'], ['Sessions', 'sessions'], ['Activity', 'activity'],
		['Processes', 'processes'], ['Offices', 'offices'], ['Settings', 'settings']
	];
	for (const [name, path] of sections) {
		await page.getByRole('navigation', { name: 'Main', exact: true }).getByRole('link', { name, exact: true }).click();
		await expect(page).toHaveURL(new RegExp(`/${path}$`));
		const main = page.getByRole('main');
		await expect(main).toHaveCount(1);
		await expect(main.getByRole('heading', { level: 1, name, exact: true })).toBeVisible();
		await expect(main.getByRole('main')).toHaveCount(0);
		await shot(page, `shell-main-${path}`);
	}
});
