import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { enterMockedOrganization } from './mock-organization';
import { organizationView, signInAsAdmin } from './web-session';

const REPOSITORY = 'https://example.test/org/state.git';

async function newAuthCase(
	page: Page,
	options: {
		authEnabled?: boolean;
		listenScheme?: 'http_loopback' | 'http_published' | 'https';
		organizations?: ReturnType<typeof organizationView>[];
		cliApprovalLinks?: string[];
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
	if (options.listenScheme !== undefined) await control('listenScheme', options.listenScheme);
	if (options.organizations) await control('organizations', options.organizations);
	if (options.cliApprovalLinks) await control('cliApprovalLinks', options.cliApprovalLinks);
	const stats = async () => {
		const res = await context.request.get(`/__p6-auth/control?id=${id}`);
		expect(res.ok()).toBe(true);
		return res.json() as Promise<{
			authEnabled: boolean;
			listenScheme: string;
			hsts: string;
			creates: { operation_id: string }[];
			requestHeaders: Record<string, string>[];
			tlsRequests: Record<string, unknown>[];
			cliApprovalDecisions: { link: string; approve: boolean }[];
		}>;
	};
	return { id, control, stats };
}

const axe = async (page: Page) =>
	(await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa']).analyze())
		.violations;

test(
	'TASK-76 AC #2: fresh local authless HQ lands directly on /create, creates an organization without proof, and reloads into /welcome without Sign out',
	{ tag: '@webkit' },
	async ({ page }) => {
		const server = await newAuthCase(page, { authEnabled: false, listenScheme: 'http_loopback' });
		await visit(page, '/');
		await expect(page).toHaveURL(/\/create$/);
		await expect(
			page.getByRole('heading', { level: 1, name: 'Create organization' })
		).toBeVisible();
		await expect(page.getByTestId('local-mode-bar')).toContainText(
			'Local mode · Authentication off'
		);
		expect(await axe(page)).toEqual([]);

		// Visiting /sign-in in authless mode also bypasses the sign-in wall directly into /create.
		await visit(page, '/sign-in');
		await expect(page).toHaveURL(/\/create$/);

		await page.getByLabel('Name', { exact: true }).fill('Local Studio');
		await page.getByLabel('Org Git URL').fill(REPOSITORY);
		await page.getByLabel('State branch').fill('main');
		await server.control('createScript', ['completed']);
		await page.getByRole('button', { name: 'Create organization', exact: true }).click();
		await expect(
			page.getByRole('heading', { name: 'Created Local Studio. You own it.' })
		).toBeVisible();

		// Every request in authless mode went without an X-Org-Session-Proof header.
		const stats = await server.stats();
		expect(stats.creates).toHaveLength(1);
		expect(stats.requestHeaders.every((h) => h['x-org-session-proof'] === '')).toBe(true);

		// With 1 organization present, / lands directly on /welcome listing it, with no Sign out button.
		await visit(page, '/');
		await expect(page).toHaveURL(/\/welcome$/);
		await expect(page.getByTestId('organization-row')).toContainText('Local Studio');
		await page.getByRole('button', { name: 'Local mode · Authentication off' }).click();
		await expect(page.getByRole('link', { name: 'Security & Access' })).toBeVisible();
		await expect(page.getByRole('button', { name: 'Sign out' })).toHaveCount(0);
	}
);

test('TASK-76 AC #3 + TASK-62 AC #3: opt-in authentication flips auth_enabled, enforces 401 on unauthenticated reload, and unlocks HTTPS settings', async ({
	page
}) => {
	const server = await newAuthCase(page, { authEnabled: false, listenScheme: 'http_loopback' });
	await visit(page, '/security');
	await expect(page.getByRole('heading', { level: 1, name: 'Security & Access' })).toBeVisible();
	await expect(page.getByText('Disabled (local HTTP mode)')).toBeVisible();
	await expect(page.getByText('HTTP (loopback)')).toBeVisible();
	await expect(page.getByTestId('https-auth-guard')).toContainText(
		'Enable authentication above before turning on HTTPS.'
	);
	await expect(page.getByRole('button', { name: 'Enable HTTPS' })).toBeDisabled();
	expect(await axe(page)).toEqual([]);

	// Host validation error (400 field/rule) shows inline under the field.
	await server.control('authEnableError', {
		status: 400,
		code: 'invalid_request',
		field: 'login',
		rule: 'format'
	});
	await page.getByLabel('Username').fill('alice');
	await page.getByLabel('Password (at least 12 characters)').fill('correct horse battery');
	await page.getByLabel('Confirm password').fill('correct horse battery');
	await page.getByRole('button', { name: 'Enable authentication' }).click();
	await expect(
		page.getByText('Use 1 to 64 characters, without spaces at the start or end.')
	).toBeVisible();

	// Valid submission enables auth, keeps user signed in, and unlocks HTTPS controls.
	await page.getByRole('button', { name: 'Enable authentication' }).click();
	await expect(page.getByTestId('auth-enabled-summary')).toContainText('Signed in as alice.');
	await expect(page.getByTestId('https-auth-guard')).toHaveCount(0);
	await expect(page.getByRole('button', { name: 'Enable HTTPS' })).toBeEnabled();
	expect((await server.stats()).authEnabled).toBe(true);

	// Simulating a fresh unauthenticated browser (clearing cookies & IndexedDB) enforces 401 -> /sign-in.
	await page.evaluate(async () => {
		await new Promise<void>((resolve) => {
			const req = indexedDB.deleteDatabase('org-session');
			req.onsuccess = req.onerror = req.onblocked = () => resolve();
		});
	});
	await page.context().clearCookies({ name: /^org_session_/ });
	await visit(page, '/');
	await expect(page).toHaveURL(/\/sign-in$/);

	// Signing in with the newly enabled credentials succeeds.
	await page.getByLabel('Username').fill('alice');
	await page.getByLabel('Password', { exact: true }).fill('correct horse battery');
	await page.getByRole('button', { name: 'Sign in', exact: true }).click();
	await expect(page).toHaveURL(/\/welcome$/);
});

test('TASK-62 AC #3: admin can enable HTTPS (self-signed & supplied PEM) and live-rotate certificates from Settings -> Security & Access', async ({
	page
}) => {
	await signInAsAdmin(page, [
		{
			id: 'org-tls',
			name: 'Lumen Labs',
			created_at: '2026-11-21T00:00:00Z',
			recovery: 'acknowledged'
		}
	]);
	const cookies = await page.context().cookies();
	const id = cookies.find((c) => c.name === 'p6_auth_case')!.value;

	// Verify inside the Shell Settings -> Security & Access route as well.
	await enterMockedOrganization(page, 'Lumen Labs');
	await visit(page, '/settings/security');
	await expect(page.getByRole('heading', { level: 2, name: 'Security & Access' })).toBeVisible();
	await expect(page.getByText('HTTP (loopback)')).toBeVisible();

	// Enable self-signed HTTPS with HSTS = on.
	await page.getByLabel('Strict-Transport-Security (HSTS)').selectOption('on');
	await page.getByRole('button', { name: 'Enable HTTPS' }).click();
	await expect(page.getByTestId('https-result')).toContainText(
		'HTTPS configured. Restart HQ to switch the browser listener.'
	);
	await expect(page.getByTestId('https-origin')).toHaveText('https://127.0.0.1:4173');
	await expect(page.getByTestId('https-fingerprint')).toContainText('sha256:selfsigned');
	await expect(page.getByTestId('https-restart-note')).toContainText('https://127.0.0.1:4173');

	// Switch to supplied PEM: empty fields are caught client-side.
	await page.getByLabel('Supplied PEM certificate and private key').check();
	await page.getByRole('button', { name: 'Enable HTTPS' }).click();
	await expect(page.getByText('Paste the PEM certificate chain.')).toBeVisible();
	await expect(page.getByText('Paste the PEM private key.')).toBeVisible();

	// Server 400 invalid PEM error is displayed cleanly.
	await page.context().request.post('/__p6-auth/control', {
		data: { id, action: 'tlsScript', value: ['invalid'] }
	});
	await page
		.getByLabel('Certificate PEM')
		.fill('-----BEGIN CERTIFICATE-----\nBAD\n-----END CERTIFICATE-----');
	await page
		.getByLabel('Private key PEM')
		.fill('-----BEGIN PRIVATE KEY-----\nBAD\n-----END PRIVATE KEY-----');
	await page.getByRole('button', { name: 'Enable HTTPS' }).click();
	await expect(page.getByTestId('https-error')).toContainText(
		'certificate or private key is invalid'
	);

	// Simulate instance restarted on HTTPS with HSTS=on -> live certificate rotation leaves HSTS untouched (omitted in payload), keeps restart_required=false, and clears key textarea.
	await page.context().request.post('/__p6-auth/control', {
		data: { id, action: 'listenScheme', value: 'https' }
	});
	await visit(page, '/security');
	await expect(page.getByText('HTTPS', { exact: true }).first()).toBeVisible();
	await expect(page.getByLabel('Strict-Transport-Security (HSTS)')).toHaveValue('');
	await page.getByLabel('Supplied PEM certificate and private key').check();
	await page
		.getByLabel('Certificate PEM')
		.fill('-----BEGIN CERTIFICATE-----\nVALID\n-----END CERTIFICATE-----');
	await page
		.getByLabel('Private key PEM')
		.fill('-----BEGIN PRIVATE KEY-----\nVALID\n-----END PRIVATE KEY-----');
	await page.getByRole('button', { name: 'Replace TLS certificate' }).click();
	await expect(page.getByTestId('https-result')).toContainText(
		'TLS certificate updated on the active HTTPS listener.'
	);
	await expect(page.getByTestId('https-fingerprint')).toContainText('sha256:supplied');
	await expect(page.getByTestId('https-restart-note')).toHaveCount(0);
	await expect(page.getByLabel('Private key PEM')).toHaveValue('');
	const afterRotate = (await (
		await page.context().request.get(`/__p6-auth/control?id=${id}`)
	).json()) as { hsts: string; tlsRequests: Record<string, unknown>[] };
	expect(afterRotate.hsts).toBe('on');
	expect('hsts' in afterRotate.tlsRequests.at(-1)!).toBe(false);

	// Explicitly changing HSTS on HTTPS sends hsts and reports restart_required=true.
	await page.getByLabel('Self-signed certificate (generated on the HQ machine)').check();
	await page.getByLabel('Strict-Transport-Security (HSTS)').selectOption('clear');
	await page.getByRole('button', { name: 'Replace TLS certificate' }).click();
	await expect(page.getByTestId('https-result')).toContainText(
		'HTTPS configured. Restart HQ to switch the browser listener.'
	);
});

test('TASK-76 race: external auth enablement returns 401/409 and exits dead authless UI cleanly to /sign-in without phantom signed-in state', async ({
	page
}) => {
	const server = await newAuthCase(page, { authEnabled: false, listenScheme: 'http_loopback' });
	await visit(page, '/security');
	await expect(page.getByText('Disabled (local HTTP mode)')).toBeVisible();

	// Another browser/CLI enables auth before this tab submits -> real backend returns 401 (no cookie+proof pair).
	await server.control('authEnabled', true);
	await page.getByLabel('Username').fill('alice');
	await page.getByLabel('Password (at least 12 characters)').fill('correct horse battery');
	await page.getByLabel('Confirm password').fill('correct horse battery');
	await page.getByRole('button', { name: 'Enable authentication' }).click();
	await expect(page).toHaveURL(/\/sign-in\?next=%2Fsecurity&note=ended$/);

	// Same race returning 409 without a stored pair also exits cleanly to /sign-in.
	const server409 = await newAuthCase(page, { authEnabled: false, listenScheme: 'http_loopback' });
	await visit(page, '/security');
	await expect(page.getByText('Disabled (local HTTP mode)')).toBeVisible();
	await server409.control('authEnableError', { status: 409, code: 'conflict' });
	await page.getByLabel('Username').fill('alice');
	await page.getByLabel('Password (at least 12 characters)').fill('correct horse battery');
	await page.getByLabel('Confirm password').fill('correct horse battery');
	await page.getByRole('button', { name: 'Enable authentication' }).click();
	await expect(page).toHaveURL(/\/sign-in\?next=%2Fsecurity&note=ended$/);
});

test('SIGN-40 / FC-173: /cli-approve clears fragment secret, supports inline sign-in, approve, decline, and expired links without exposing token', async ({
	page
}) => {
	const approveToken = 'cli-approve-secret-token-1';
	const declineToken = 'cli-decline-secret-token-2';
	const server = await newAuthCase(page, {
		authEnabled: true,
		cliApprovalLinks: [approveToken, declineToken]
	});

	// 1. Signed-out user with auth_enabled=true gets inline sign-in; fragment is cleared immediately and never rendered.
	await visit(page, `/cli-approve#${approveToken}`);
	expect(page.url()).not.toContain('#');
	expect(page.url()).not.toContain(approveToken);
	await expect(
		page.getByRole('heading', { level: 1, name: 'Sign in to approve terminal access' })
	).toBeVisible();
	await expect(page.locator('body')).not.toContainText(approveToken);
	expect(await axe(page)).toEqual([]);

	await page.getByLabel('Username').fill('admin');
	await page.getByLabel('Password', { exact: true }).fill('private-password');
	await page.getByRole('button', { name: 'Sign in to continue' }).click();

	await expect(
		page.getByRole('heading', { level: 1, name: 'Approve terminal sign-in' })
	).toBeVisible();
	await expect(page.locator('body')).not.toContainText(approveToken);
	await page.getByRole('button', { name: 'Approve terminal sign-in' }).click();
	await expect(
		page.getByRole('heading', { level: 1, name: 'Terminal sign-in approved' })
	).toBeVisible();

	// 2. Reusing the same token returns 404 -> expired/used state.
	await visit(page, `/cli-approve#${approveToken}`);
	expect(page.url()).not.toContain('#');
	await page.getByRole('button', { name: 'Approve terminal sign-in' }).click();
	await expect(
		page.getByRole('heading', { level: 1, name: 'This link has expired or was already used.' })
	).toBeVisible();

	// 3. Declining a valid token records decline and shows declined state.
	await visit(page, `/cli-approve#${declineToken}`);
	expect(page.url()).not.toContain('#');
	await page.getByRole('button', { name: 'Decline' }).click();
	await expect(
		page.getByRole('heading', { level: 1, name: 'Terminal sign-in declined' })
	).toBeVisible();

	// 4. Opening /cli-approve without a fragment shows expired/used state immediately.
	await visit(page, '/cli-approve');
	await expect(
		page.getByRole('heading', { level: 1, name: 'This link has expired or was already used.' })
	).toBeVisible();

	const stats = await server.stats();
	expect(stats.cliApprovalDecisions).toEqual([
		{ link: approveToken, approve: true },
		{ link: declineToken, approve: false }
	]);
});
