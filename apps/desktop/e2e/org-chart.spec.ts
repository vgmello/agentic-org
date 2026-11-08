import { expect, test } from '@playwright/test';
import { shot } from './shot';

test.beforeEach(async ({ page }) => {
	await page.goto('/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Org chart' }).click();
	await expect(page).toHaveURL(/\/org-chart$/);
});

const nodes = (page: import('@playwright/test').Page) => page.getByTestId('org-node');

test('shows everyone in the sample org, owner first', async ({ page }) => {
	await expect(nodes(page)).toHaveCount(11);
	await expect(nodes(page).first()).toContainText('Owner');
	await expect(page.getByText('10 agents')).toBeVisible();
	await shot(page, 'org-chart');
});

test('collapsing a manager hides their whole team, and expand all brings it back', async ({ page }) => {
	await page.locator('[data-node=sam]').getByRole('button', { name: /Collapse Sam Okafor/ }).click();
	await expect(nodes(page)).toHaveCount(8);
	await expect(page.getByText('Noor Haddad')).toHaveCount(0);
	await shot(page, 'org-chart-collapsed');
	await page.getByRole('button', { name: 'Expand all' }).click();
	await expect(nodes(page)).toHaveCount(11);
});

test('collapse all leaves the owner and the chief of staff; the size toggle switches modes', async ({ page }) => {
	await page.getByRole('button', { name: 'Collapse all' }).click();
	await expect(nodes(page)).toHaveCount(2);
	await page.getByRole('button', { name: 'Expand all' }).click();
	await page.getByRole('button', { name: 'Actual size' }).click();
	await expect(page.getByRole('button', { name: 'Fit to width' })).toBeVisible();
});
