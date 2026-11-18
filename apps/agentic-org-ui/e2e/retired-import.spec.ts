import { expect, test } from '@playwright/test';
import { shot } from './shot';
import { enterMockedOrganization } from './mock-organization';

test('IMP-retired: hiring remains while existing-team import and native window controls are absent', { tag: '@journey' }, async ({ page }) => {
	await enterMockedOrganization(page, 'Fresh organization');
	await expect(page).toHaveURL(/\/agents$/);
	await expect(page.getByRole('button', { name: 'Add agent', exact: true })).toBeVisible();
	await expect(page.getByRole('button', { name: /bring in/i })).toHaveCount(0);
	await expect(page.locator('[data-tauri-drag-region]')).toHaveCount(0);
	await page.getByRole('navigation', { name: 'Main', exact: true }).getByRole('link', { name: 'Org chart', exact: true }).click();
	await expect(page.getByRole('button', { name: /bring in/i })).toHaveCount(0);
	await expect(page.getByRole('heading', { name: 'Org chart', exact: true })).toBeVisible();
	await shot(page, 'IMP-retired-org-chart');
});
