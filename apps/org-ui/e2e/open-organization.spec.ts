import { expect, test } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

const rows = (page: import('@playwright/test').Page) => page.getByTestId('organization-row');

test('the welcome screen lists the organizations this Mac already has, and opening one enters it', { tag: '@journey' }, async ({ page }) => {
	await visit(page, '/welcome');
	await expect(page.getByText('Looking for organizations on this Mac')).toBeVisible();
	await expect(rows(page)).toHaveCount(2);
	await expect(rows(page).first()).toContainText('Northwind Labs');
	await expect(rows(page).first()).toContainText('HQ · This Mac');
	await expect(rows(page).first()).toContainText('created Sep 14, 2026');
	await shot(page, 'welcome-organizations');

	await page.getByRole('button', { name: 'Open Globex Research' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await expect(page.getByText('Globex Research', { exact: true })).toBeVisible();

	// Remembered: the next launch goes straight in.
	await visit(page, '/');
	await expect(page).toHaveURL(/\/agents$/);
	await expect(page.getByText('Globex Research', { exact: true })).toBeVisible();
});

test('Switch organization returns to the list, marks the current one, and opens another', { tag: '@journey' }, async ({ page }) => {
	await visit(page, '/welcome');
	await page.getByRole('button', { name: 'Open Northwind Labs' }).click();
	await expect(page).toHaveURL(/\/agents$/);

	await page.getByRole('link', { name: /Switch organization/ }).click();
	await expect(page).toHaveURL(/\/welcome$/);
	await expect(rows(page)).toHaveCount(2);
	await expect(page.locator('[data-organization=sample-northwind]')).toContainText('Open now');
	await expect(page.locator('[data-organization=sample-globex]')).not.toContainText('Open now');
	await shot(page, 'welcome-switch');

	await page.getByRole('button', { name: 'Open Globex Research' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await expect(page.getByText('Globex Research', { exact: true })).toBeVisible();
});

test('an organization created here is listed afterwards', async ({ page }) => {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Fresh Start');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);

	await page.getByRole('link', { name: /Switch organization/ }).click();
	await expect(rows(page)).toHaveCount(3);
	await expect(page.locator('[data-testid=organization-row]', { hasText: 'Fresh Start' })).toContainText('Open now');
});

test('when the office cannot start, the screen explains why and still offers Create', async ({ page }) => {
	await visit(page, '/welcome?mock=unreachable');
	await expect(page.getByText("The office isn't answering")).toBeVisible();
	await expect(rows(page)).toHaveCount(0);
	await expect(page.getByRole('button', { name: 'Try again' })).toBeVisible();
	await expect(page.getByRole('link', { name: /Create organization/ })).toBeVisible();
	await shot(page, 'welcome-unreachable');
});

test('an organization that vanished is dropped from the list when opening it fails', async ({ page }) => {
	await visit(page, '/welcome');
	await expect(rows(page)).toHaveCount(2);
	// The Mac loses an organization after the list was shown: its stored list now has only Northwind.
	await page.evaluate(() => {
		const hq = (id: string) => ({ id: `office-${id}`, organization_id: id, name: 'HQ', role: 'hq', host: 'This Mac', status: 'active' });
		const northwind = { id: 'sample-northwind', name: 'Northwind Labs', created_at: '2026-09-14T09:30:00.000Z', offices: [hq('sample-northwind')] };
		localStorage.setItem('agentic-org.mock.organizations', JSON.stringify([northwind]));
	});
	await page.getByRole('button', { name: 'Open Globex Research' }).click();
	await expect(page.getByText("Couldn't open it")).toBeVisible();
	await expect(rows(page)).toHaveCount(1);
	await expect(rows(page).first()).toContainText('Northwind Labs');
	await expect(page).toHaveURL(/\/welcome$/);
});

test('an organization whose office is unavailable is listed, marked, and cannot be opened', async ({ page }) => {
	await visit(page, '/welcome');
	await page.evaluate(() => {
		const office = (id: string, status: string) => ({ id: `office-${id}`, organization_id: id, name: 'HQ', role: 'hq', host: 'This Mac', status });
		const org = (id: string, name: string, status: string) => ({ id, name, created_at: '2026-09-14T09:30:00.000Z', offices: [office(id, status)] });
		localStorage.setItem(
			'agentic-org.mock.organizations',
			JSON.stringify([org('up', 'Up and Running', 'active'), org('down', 'Locked Away', 'unavailable')])
		);
	});
	await page.reload();
	await expect(rows(page)).toHaveCount(2);
	const locked = page.locator('[data-organization=down]');
	await expect(locked).toContainText('Unavailable');
	await expect(locked.getByRole('button', { name: 'Open Locked Away' })).toBeDisabled();
	await expect(page.locator('[data-organization=up]').getByRole('button', { name: 'Open Up and Running' })).toBeEnabled();
	await shot(page, 'welcome-unavailable');
});

test('an organization whose office goes down after the list was shown is marked unavailable when opening it fails', async ({ page }) => {
	await visit(page, '/welcome');
	await expect(rows(page)).toHaveCount(2);
	await page.evaluate(() => {
		const office = (id: string, status: string) => ({ id: `office-${id}`, organization_id: id, name: 'HQ', role: 'hq', host: 'This Mac', status });
		const org = (id: string, name: string, status: string) => ({ id, name, created_at: '2026-09-14T09:30:00.000Z', offices: [office(id, status)] });
		// The page already listed both as available; now Globex's office goes down.
		localStorage.setItem(
			'agentic-org.mock.organizations',
			JSON.stringify([org('sample-northwind', 'Northwind Labs', 'active'), org('sample-globex', 'Globex Research', 'unavailable')])
		);
	});
	await page.getByRole('button', { name: 'Open Globex Research' }).click();
	await expect(page.getByText("Couldn't open it")).toBeVisible();
	await expect(page.locator('[data-organization=sample-globex]')).toContainText('Unavailable');
	await expect(page.getByRole('button', { name: 'Open Globex Research' })).toBeDisabled();
	await expect(page).toHaveURL(/\/welcome$/);
});
