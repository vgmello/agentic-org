import { expect, test } from '@playwright/test';
import { quiet } from './collector';
import { shot } from '../e2e/shot';

const policy = "default-src 'none'; script-src 'report-sample' 'self' {script_hashes}; style-src 'self' 'unsafe-inline'; img-src 'self'; font-src 'self'; connect-src 'self'; manifest-src 'self'; worker-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; object-src 'none'; report-uri /__csp-report";
const routes = ['/', '/welcome', '/create', '/agents', '/org-chart', '/projects', '/projects/atlas', '/work', '/chat', '/processes', '/offices', '/sessions', '/activity', '/settings'];

test('B1-05 Part B: identical CSP permits every route, a dialog, a sheet and a toast', async ({ page }) => {
	await page.goto('/create');
	await expect(page.locator('html')).toHaveAttribute('data-hydrated', 'true');
	await page.getByLabel('Organization name').fill('CSP Smoke');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	for (const path of routes) await test.step(`fresh route ${path}`, async () => {
		const response = await page.goto(path);
		expect(response?.status()).toBe(200);
		const header = response!.headers()['content-security-policy'] ?? '';
		const hashes = header.match(/'sha256-[A-Za-z0-9+/]{43}='/g) ?? [];
		expect(hashes.length).toBeGreaterThan(0);
		expect(header.replace(hashes.join(' '), '{script_hashes}')).toBe(policy);
		await expect(page.locator('html')).toHaveAttribute('data-hydrated', 'true');
		await expect(page).toHaveURL(new RegExp(`${path === '/' ? '/agents' : path}$`));
	});

	await page.goto('/offices');
	const office = page.getByTestId('office-row').first().getByRole('button');
	await office.press('Enter');
	await expect(page.getByTestId('office-details')).toBeVisible();
	await expect(page.getByTestId('office-details')).toContainText('Online');
	await shot(page, 'csp-office-sheet', { context: page.getByTestId('office-details') });
	await page.keyboard.press('Escape');
	await expect(office).toBeFocused();
	await page.goto('/projects/atlas');
	await page.getByRole('tab', { name: 'Bugs' }).click();
	await page.getByRole('button', { name: 'Report a bug' }).press('Enter');
	const dialog = page.getByRole('dialog', { name: 'Report a bug' });
	await expect(dialog).toBeVisible();
	await dialog.getByLabel('Title').fill('CSP smoke');
	await dialog.getByLabel('What happened').fill('A test report under the production policy.');
	await dialog.getByRole('radio', { name: /^Minor/ }).check();
	await shot(page, 'csp-report-dialog', { context: dialog });
	await dialog.getByRole('button', { name: 'Report bug' }).click();
	await expect(dialog).toBeHidden();
	await expect(page.getByText('Bug B-1 reported.')).toBeVisible();
	await shot(page, 'csp-reported-toast');
	await quiet(); // WebKit can drop reports sent while a context is closing.
});
