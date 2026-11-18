import { expect, type Page } from '@playwright/test';
import { visit } from './ready';

export interface WebOrganization {
	readonly id: string;
	readonly name: string;
	readonly created_at: string;
	readonly recovery: 'pending' | 'delivered' | 'acknowledged';
}

/**
 * The mocked web-session fixture (e2e mode only; the Vite test composition serves it, see
 * fakes/session-server.ts): a fresh case whose GET /api/v1/session is the signed-in
 * administrator and whose GET /api/v1/organizations lists `organizations` ([] as on a real
 * instance until A4). Signs in through the real sign-in page and lands on /welcome.
 */
export async function signInAsAdmin(page: Page, organizations: readonly WebOrganization[] = []) {
	const context = page.context();
	const created = await context.request.post('/__p6-auth/new');
	expect(created.ok()).toBe(true);
	const { id } = await created.json();
	await context.addCookies([
		{ name: 'p6_auth_case', value: id, url: new URL('/', created.url()).href }
	]);
	const listed = await context.request.post('/__p6-auth/control', {
		data: { id, action: 'organizations', value: organizations }
	});
	expect(listed.ok()).toBe(true);
	await visit(page, '/sign-in');
	await page.getByLabel('Username').fill('admin');
	await page.getByLabel('Password', { exact: true }).fill('private-password');
	await page.getByRole('button', { name: 'Sign in', exact: true }).click();
	await expect(page).toHaveURL(/\/welcome$/);
}
