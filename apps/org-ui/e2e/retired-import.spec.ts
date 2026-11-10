import { expect, test } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

test('IMP-retired: hiring remains while existing-team import and native window controls are absent', async ({ page }) => {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Fresh organization');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await expect(page.getByRole('button', { name: 'Add agent', exact: true })).toBeVisible();
	await expect(page.getByRole('button', { name: /bring in/i })).toHaveCount(0);
	await expect(page.locator('[data-tauri-drag-region]')).toHaveCount(0);
	await page.getByRole('link', { name: 'Org chart', exact: true }).click();
	await expect(page.getByRole('button', { name: /bring in/i })).toHaveCount(0);
	await expect(page.getByRole('heading', { name: 'Org chart', exact: true })).toBeVisible();
	await shot(page, 'IMP-retired-org-chart');
});
