import { expect, test } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

const areas = ['agents', 'org-chart', 'projects', 'work', 'chat', 'processes', 'offices', 'sessions', 'activity', 'settings'];

test('SRV-18: every unserved production area is unavailable, without sample data or native controls', async ({ page }) => {
	const apiRequests: string[] = [];
	page.on('request', (request) => { if (new URL(request.url()).pathname.startsWith('/api/')) apiRequests.push(request.url()); });
	for (const path of ['/', '/welcome', '/create', ...areas.map((area) => `/${area}`), '/projects/a-project']) {
		await visit(page, path);
		await expect(page.getByTestId('area-unavailable')).toBeVisible();
		await expect(page.getByRole('main')).toHaveCount(1);
		await expect(page.locator('main main')).toHaveCount(0);
		await expect(page.getByRole('status')).toHaveText('This area is not available yet.');
		await expect(page.getByText('Sample data', { exact: true })).toHaveCount(0);
		await expect(page.locator('[data-tauri-drag-region]')).toHaveCount(0);
		await expect(page.getByRole('button', { name: /create organization|bring in|login items|turn on/i })).toHaveCount(0);
	}
	expect(apiRequests).toEqual([]);
	await shot(page, 'SRV-18-unavailable');
});

test('SRV-18: browser query parameters, remembered sample data and native globals cannot enable fakes', async ({ page }) => {
	await page.addInitScript(() => {
		Object.assign(window, { __TAURI_INTERNALS__: {}, __mockOfficeStatus: 'injected' });
		localStorage.setItem('agentic-org.mock.connections', JSON.stringify([{ organization: { id: 'forged', name: 'Forged sample' } }]));
	});
	await visit(page, '/org-chart?org=empty&mock=healthy&copilot=on');
	await expect(page.getByRole('heading', { name: 'Org chart is unavailable' })).toBeVisible();
	await expect(page.getByText('Forged sample')).toHaveCount(0);
	await expect(page.getByRole('button', { name: /add|create|bring in/i })).toHaveCount(0);
	await page.getByRole('link', { name: 'Chat', exact: true }).click();
	await expect(page.getByRole('heading', { name: 'Chat is unavailable' })).toBeVisible();
	await expect(page.getByRole('link', { name: 'Chat', exact: true })).toHaveAttribute('aria-current', 'page');
	await shot(page, 'SRV-18-unavailable-navigation');
});


test('SRV-18: the production unavailable gate preserves unknown-route errors', async ({ page }) => {
	await visit(page, '/no-such-area');
	await expect(page.getByRole('heading', { name: 'Something went wrong' })).toBeVisible();
	await expect(page.getByText('404', { exact: true })).toBeVisible();
	await expect(page.getByRole('main')).toHaveCount(1);
	await expect(page.locator('main main')).toHaveCount(0);
	await expect(page.getByTestId('area-unavailable')).toHaveCount(0);
	await page.getByRole('link', { name: 'Back to the start' }).click();
	await expect(page.getByTestId('area-unavailable')).toBeVisible();
});
