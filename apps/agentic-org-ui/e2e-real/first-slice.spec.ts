import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
import { stopInstance } from './teardown';

const shots = '../../.playwright/a3-b6-screens';
const base = () => process.env.B6_BASE!;
const login = 'alex';
const password = 'correct horse battery';

async function axe(page: Page) {
	const result = await new AxeBuilder({ page }).analyze();
	expect(
		result.violations.map(
			(v) =>
				`${v.id}: ${v.help} @ ${v.nodes.map((n) => `${n.target.join(' ')} (${n.failureSummary})`).join('; ')}`
		)
	).toEqual([]);
}

test.describe.configure({ mode: 'serial' });

test('SIGN-22/1/9/27: setup link → register → signed in → SIGN-9 → sign out (every tab) → sign in', async ({
	page,
	context
}) => {
	await page.goto(process.env.B6_LINK!);
	await expect(
		page.getByRole('heading', { name: /Create your account on HQ at 127\.0\.0\.1:/ })
	).toBeVisible();
	// The secret leaves the address bar before any request (SIGN-22).
	expect(page.url()).not.toContain('#');
	await axe(page);
	await page.screenshot({ path: `${shots}/01-register.png` });
	await page.getByLabel('Username').fill(login);
	await page.getByLabel('Password (at least 12 characters)').fill(password);
	await page.getByLabel('Password again').fill(password);
	await page.getByRole('button', { name: /Create account|Register/ }).click();
	await expect(page).toHaveURL(/\/welcome$/);
	await expect(
		page.getByRole('heading', { name: 'No organizations yet. Create one to get started.' })
	).toBeVisible();
	await expect(page.getByRole('link', { name: 'Create organization' })).toBeVisible();
	await axe(page);
	await page.screenshot({ path: `${shots}/02-welcome-admin.png` });

	// The link works once (SIGN-38): link inspection detects the used link immediately on load.
	const reused = await context.newPage();
	await reused.goto(process.env.B6_LINK!);
	await expect(
		reused.getByRole('heading', { name: 'This link has expired or was already used.' })
	).toBeVisible();
	await reused.goto(`${base()}/welcome`);
	await expect(reused.getByRole('heading', { name: /No organizations yet/ })).toBeVisible();

	// SIGN-28/29 menu, SIGN-20: sign out sends every tab of this HQ to the sign-in page.
	await page.getByRole('button', { name: login }).click();
	await expect(page.getByText('Administrator')).toBeVisible();
	await page.screenshot({ path: `${shots}/03-account-menu.png` });
	await page.getByRole('button', { name: 'Sign out' }).first().click();
	await expect(page).toHaveURL(/\/sign-in/);
	await expect(page.getByText('You signed out.')).toBeVisible();
	await expect(reused).toHaveURL(/\/sign-in/);
	await expect(reused.getByText('You signed out.')).toBeVisible();
	await reused.close();
	await axe(page);
	await page.screenshot({ path: `${shots}/04-signed-out.png` });

	// SIGN-1: a signed-out visit to a page goes to sign-in with that page as the return target.
	await page.goto(`${base()}/welcome`);
	await expect(page).toHaveURL(/\/sign-in\?.*next=%2Fwelcome/);

	// SIGN-3: empty fields send nothing and say so under the field.
	let requests = 0;
	page.on('request', (r) => {
		if (r.url().includes('/api/v1/session/sign-in')) requests++;
	});
	await page.getByRole('button', { name: 'Sign in' }).click();
	await expect(page.getByText('Enter your username.')).toBeVisible();
	await expect(page.getByLabel('Username')).toBeFocused();
	expect(requests).toBe(0);

	// SIGN-4: wrong password: same words, password cleared, username kept, focus on Password.
	await page.getByLabel('Username').fill(login);
	await page.getByLabel('Password').fill('not the password');
	await page.getByRole('button', { name: 'Sign in' }).click();
	await expect(page.getByText("That username and password don't match. Try again.")).toBeVisible();
	await expect(page.getByLabel('Password')).toHaveValue('');
	await expect(page.getByLabel('Username')).toHaveValue(login);
	await expect(page.getByLabel('Password')).toBeFocused();
	await page.screenshot({ path: `${shots}/05-mismatch.png` });

	// SIGN-27 focus order: Username, Password, Show, Sign in.
	await page.getByLabel('Username').focus();
	await page.keyboard.press('Tab');
	await expect(page.getByLabel('Password')).toBeFocused();
	await page.keyboard.press('Tab');
	await expect(page.getByRole('button', { name: /Show/ })).toBeFocused();
	await page.keyboard.press('Tab');
	await expect(page.getByRole('button', { name: 'Sign in' })).toBeFocused();

	// SIGN-8 + signed in again: the same-HQ return target opens.
	await page.getByLabel('Password').fill(password);
	await page.getByRole('button', { name: 'Sign in' }).click();
	await expect(page).toHaveURL(/\/welcome$/);
	await expect(page.getByRole('heading', { name: /No organizations yet/ })).toBeVisible();
	// The session survives a reload (stored at the host, ADR-0013).
	await page.reload();
	await expect(page.getByRole('heading', { name: /No organizations yet/ })).toBeVisible();
});

test('SIGN-8: a return target off this HQ is ignored', async ({ page }) => {
	await page.goto(`${base()}/sign-in?next=${encodeURIComponent('https://example.com/steal')}`);
	await page.getByLabel('Username').fill(login);
	await page.getByLabel('Password').fill(password);
	await page.getByRole('button', { name: 'Sign in' }).click();
	await expect(page).toHaveURL(`${base()}/welcome`);
});

test('SIGN-6: repeated wrong passwords lock sign-in with a retry time', async ({ page }) => {
	await page.goto(`${base()}/sign-in`);
	await page.getByLabel('Username').fill(login);
	for (let i = 0; i < 8; i++) {
		const button = page.getByRole('button', { name: 'Sign in' });
		if (await button.isDisabled()) break;
		await page.getByLabel('Password').fill(`wrong password ${i}`);
		await button.click();
		await expect(page.getByRole('alert')).toBeVisible();
		if (await page.getByText(/Too many sign-in attempts/).isVisible()) break;
	}
	await expect(
		page.getByText(
			/Too many sign-in attempts\. (Try again in \d+ minutes?|Wait a few minutes, then try again)\./
		)
	).toBeVisible();
	await expect(page.getByRole('button', { name: 'Sign in' })).toBeDisabled();
	await page.screenshot({ path: `${shots}/06-rate-limited.png` });
});

test('SIGN-7: HQ stopped after the page loaded', async ({ page }) => {
	await page.goto(`${base()}/sign-in`);
	await expect(page.getByLabel('Username')).toBeVisible();
	stopInstance(process.env.B6_ROOT!);
	await page.getByLabel('Username').fill('sam');
	await page.getByLabel('Password').fill('another long password');
	await page.getByRole('button', { name: 'Sign in' }).click();
	await expect(page.getByText(/Can't reach HQ/)).toBeVisible();
	await expect(page.getByRole('button', { name: 'Try again' })).toBeVisible();
	await expect(page.getByLabel('Username')).toHaveValue('sam');
	await page.screenshot({ path: `${shots}/07-unreachable.png` });
});
