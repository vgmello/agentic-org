import { expect, test, type Page } from '@playwright/test';
import { enterMockedOrganization } from './mock-organization';
import { visit } from './ready';
import { organizationView, signInAsAdmin } from './web-session';

async function newAuthCase(
	page: Page,
	options: {
		authEnabled?: boolean;
		organizations?: ReturnType<typeof organizationView>[];
		cliApprovalLinks?: string[];
		links?: Record<string, unknown>;
		currentPassword?: string;
	} = {}
) {
	const context = page.context();
	const created = await context.request.post('/__p6-auth/new');
	expect(created.ok()).toBe(true);
	const { id } = await created.json();
	await context.addCookies([
		{ name: 'p6_auth_case', value: id, url: new URL('/', created.url()).href }
	]);
	const control = async (action: string, value: unknown) => {
		const res = await context.request.post('/__p6-auth/control', { data: { id, action, value } });
		expect(res.ok()).toBe(true);
	};
	if (options.authEnabled !== undefined) await control('authEnabled', options.authEnabled);
	if (options.organizations) await control('organizations', options.organizations);
	if (options.cliApprovalLinks) await control('cliApprovalLinks', options.cliApprovalLinks);
	if (options.links) await control('links', options.links);
	if (options.currentPassword) await control('currentPassword', options.currentPassword);
	const stats = async () => {
		const res = await context.request.get(`/__p6-auth/control?id=${id}`);
		expect(res.ok()).toBe(true);
		return res.json() as Promise<{
			cliApprovalDecisions: { link: string; approve: boolean }[];
			passwordChanges: { current_password: string; new_password: string }[];
			registered: { mode: 'create' | 'attach'; login: string; link: string }[];
			passwordResets: { login: string; link: string }[];
		}>;
	};
	return { id, control, stats };
}

test('Access §4a.2: /set-password inspects token, validates mismatch, sets password, and redirects to /sign-in with login prefilled', async ({
	page
}) => {
	const token = 'reset-link-secret-01';
	const server = await newAuthCase(page, {
		authEnabled: true,
		links: { [token]: { kind: 'set_password', login: 'jordan' } }
	});

	await visit(page, `/set-password#${token}`);
	await expect(
		page.getByRole('heading', { level: 1, name: 'Set a new password for jordan' })
	).toBeVisible();
	// Secret token is scrubbed from the address bar after inspection.
	expect(page.url()).not.toContain(token);

	await page.getByLabel('New password (at least 12 characters)').fill('correct horse battery');
	await page.getByLabel('Password again').fill('different horse battery');
	await page.getByRole('button', { name: 'Set password' }).click();
	await expect(page.getByText("The passwords don't match. Enter them again.")).toBeVisible();

	await page.getByLabel('New password (at least 12 characters)').fill('new strong passphrase');
	await page.getByLabel('Password again').fill('new strong passphrase');
	await page.getByRole('button', { name: 'Set password' }).click();

	await expect(page).toHaveURL(/\/sign-in\?note=password-set&login=jordan$/);
	await expect(page.getByText('Password set. Sign in with the new password.')).toBeVisible();
	await expect(page.getByLabel('Username')).toHaveValue('jordan');

	const stats = await server.stats();
	expect(stats.passwordResets).toEqual([{ login: 'jordan', link: token }]);

	// Reusing the spent link shows expired/used copy.
	await visit(page, `/set-password#${token}`);
	await expect(page.getByText('This link has expired or was already used.')).toBeVisible();
});

test('Access §4a.2: /approve supports inline sign-in, approval, decline, and expired links', async ({
	page
}) => {
	const approveLink = 'cli-approve-secret-01';
	const declineLink = 'cli-decline-secret-02';
	const server = await newAuthCase(page, {
		authEnabled: true,
		cliApprovalLinks: [approveLink, declineLink]
	});

	await visit(page, `/approve#${approveLink}`);
	await expect(
		page.getByRole('heading', { level: 1, name: 'Sign in to approve terminal access' })
	).toBeVisible();
	expect(page.url()).not.toContain(approveLink);

	// Inline sign-in when unauthenticated.
	await page.getByLabel('Username').fill('admin');
	await page.getByLabel('Password', { exact: true }).fill('private-password');
	await page.getByRole('button', { name: 'Sign in to continue' }).click();

	await expect(
		page.getByRole('heading', { level: 1, name: 'Approve terminal sign-in' })
	).toBeVisible();
	await expect(page.getByText('Approve only if you started it, just now.')).toBeVisible();
	await page.getByRole('button', { name: 'Approve terminal sign-in' }).click();
	await expect(
		page.getByRole('heading', { level: 1, name: 'Terminal sign-in approved' })
	).toBeVisible();

	// Decline while already signed in.
	await visit(page, `/approve#${declineLink}`);
	await page.getByRole('button', { name: 'Decline' }).click();
	await expect(
		page.getByRole('heading', { level: 1, name: 'Terminal sign-in declined' })
	).toBeVisible();

	const stats = await server.stats();
	expect(stats.cliApprovalDecisions).toEqual([
		{ link: approveLink, approve: true },
		{ link: declineLink, approve: false }
	]);

	// Unknown / expired link never leaks the secret.
	await visit(page, '/approve#expired-secret-99');
	await page.getByRole('button', { name: 'Approve terminal sign-in' }).click();
	await expect(page.getByText('This link has expired or was already used.')).toBeVisible();
	await expect(page.locator('body')).not.toContainText('expired-secret-99');
});

test('Access §4a.2: invited /register supports account creation and Sign in instead attachment', async ({
	page
}) => {
	const createToken = 'invite-create-01';
	const attachToken = 'invite-attach-02';
	const server = await newAuthCase(page, {
		authEnabled: true,
		links: {
			[createToken]: {
				kind: 'organization_invite',
				org_id: 'org-1',
				org_name: 'Northstar Labs',
				member_id: 'm-1',
				member_name: 'Nadia'
			},
			[attachToken]: {
				kind: 'organization_invite',
				org_id: 'org-1',
				org_name: 'Northstar Labs',
				member_id: 'm-2',
				member_name: 'Devon'
			}
		}
	});

	await visit(page, `/register#${createToken}`);
	await expect(page.getByText("You're invited to Northstar Labs as Nadia.")).toBeVisible();
	await page.getByLabel('Username').fill('nadia');
	await page.getByLabel('Password (at least 12 characters)').fill('strong-invite-pass');
	await page.getByLabel('Password again').fill('strong-invite-pass');
	await page.getByRole('button', { name: 'Create account' }).click();
	await expect(page).toHaveURL(/\/welcome$/);

	// Second invite attaches an existing account via "Sign in instead".
	await visit(page, `/register#${attachToken}`);
	await expect(page.getByText("You're invited to Northstar Labs as Devon.")).toBeVisible();
	await page.getByRole('button', { name: 'Sign in instead' }).click();
	await page.getByLabel('Username').fill('devon');
	await page.getByLabel('Password', { exact: true }).fill('private-password');
	await page.getByRole('button', { name: 'Sign in and join' }).click();
	await expect(page).toHaveURL(/\/welcome$/);

	const stats = await server.stats();
	expect(stats.registered).toEqual([
		{ mode: 'create', login: 'nadia', link: createToken },
		{ mode: 'attach', login: 'devon', link: attachToken }
	]);
});

test('Access §4a.4: Change password works from AccountMenu, Preferences, and Security settings', async ({
	page
}) => {
	const org1 = organizationView({
		id: 'org-alpha',
		name: 'Alpha Studio',
		created_at: '2026-11-18T00:00:00Z',
		recovery: 'acknowledged'
	});
	const server = await newAuthCase(page, {
		authEnabled: true,
		organizations: [org1],
		currentPassword: 'private-password'
	});
	await visit(page, '/sign-in');
	await page.getByLabel('Username').fill('admin');
	await page.getByLabel('Password', { exact: true }).fill('private-password');
	await page.getByRole('button', { name: 'Sign in', exact: true }).click();
	await expect(page).toHaveURL(/\/welcome$/);

	// Open from AccountMenu on /welcome.
	await page.getByRole('button', { name: /admin/i }).click();
	await page.getByRole('button', { name: 'Change password' }).click();
	const dialog = page.getByRole('dialog', { name: 'Change password' });
	await expect(dialog).toBeVisible();

	// Wrong current password shows exact spec copy.
	await dialog.getByLabel('Current password').fill('wrong-current-pass');
	await dialog.getByLabel('New password (at least 12 characters)').fill('brand new passphrase');
	await dialog.getByLabel('New password again').fill('brand new passphrase');
	await dialog.getByRole('button', { name: 'Change password' }).click();
	await expect(dialog.getByText("That isn't your current password.")).toBeVisible();

	// Correct current password succeeds and closes with toast.
	await dialog.getByLabel('Current password').fill('private-password');
	await dialog.getByRole('button', { name: 'Change password' }).click();
	await expect(dialog).toBeHidden();
	await expect(
		page.getByText("Password changed. You're signed out everywhere else.")
	).toBeVisible();

	const stats = await server.stats();
	expect(stats.passwordChanges).toEqual([
		{ current_password: 'private-password', new_password: 'brand new passphrase' }
	]);

	// Also reachable inside the shell on /preferences and /settings/security.
	await enterMockedOrganization(page, 'Alpha Studio');

	await visit(page, '/preferences');
	await page.getByRole('button', { name: 'Change password' }).click();
	await expect(page.getByRole('dialog', { name: 'Change password' })).toBeVisible();
	await page.getByRole('button', { name: 'Cancel' }).click();

	await visit(page, '/settings/security');
	await page.getByRole('button', { name: 'Change password' }).click();
	await expect(page.getByRole('dialog', { name: 'Change password' })).toBeVisible();
	await page.getByRole('button', { name: 'Cancel' }).click();
});

test('Access: Organization switcher in Sidebar lists organizations and switches active organization', async ({
	page
}) => {
	const alpha = {
		id: 'org-alpha',
		name: 'Alpha Studio',
		created_at: '2026-11-18T00:00:00Z',
		recovery: 'acknowledged' as const
	};
	await signInAsAdmin(page, [alpha]);
	await enterMockedOrganization(page, 'Alpha Studio');

	const switcher = page.getByTestId('org-switcher');
	await expect(switcher).toBeVisible();
	await switcher.click();
	await expect(page.getByRole('menuitem', { name: /Alpha Studio/ })).toBeVisible();
	await expect(page.getByRole('menuitem', { name: /Northwind Labs/ })).toBeVisible();
	await expect(page.getByRole('menuitem', { name: 'All organizations' })).toBeVisible();
	await expect(page.getByRole('menuitem', { name: 'Create organization' })).toBeVisible();

	await page.getByRole('menuitem', { name: /Northwind Labs/ }).click();
	await expect(page.getByRole('link', { name: 'Northwind Labs home' })).toBeVisible();
});
