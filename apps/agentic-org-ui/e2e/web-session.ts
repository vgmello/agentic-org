import { expect, type Page } from '@playwright/test';
import { visit } from './ready';

export interface WebOrganization {
	readonly id: string;
	readonly name: string;
	readonly created_at: string;
	readonly recovery: 'pending' | 'delivered' | 'acknowledged';
	/** The rest of the organization view (web-api-v1 section 3); filled in when left out. */
	readonly slug?: string;
	readonly employee_id?: string;
	readonly offices?: readonly Record<string, unknown>[];
}
/** The whole organization view of a fixture organization: an HQ office on hq-mac, online. */
export const organizationView = (organization: WebOrganization) => ({
	slug: organization.id,
	employee_id: 'person-1',
	offices: [
		{
			id: `${organization.id}-hq`,
			organization_id: organization.id,
			name: 'HQ',
			role: 'hq',
			host: 'hq-mac',
			status: 'active'
		}
	],
	...organization
});

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
		data: { id, action: 'organizations', value: organizations.map(organizationView) }
	});
	expect(listed.ok()).toBe(true);
	await visit(page, '/sign-in');
	await page.getByLabel('Username').fill('admin');
	await page.getByLabel('Password', { exact: true }).fill('private-password');
	await page.getByRole('button', { name: 'Sign in', exact: true }).click();
	await expect(page).toHaveURL(/\/welcome$/);
}
