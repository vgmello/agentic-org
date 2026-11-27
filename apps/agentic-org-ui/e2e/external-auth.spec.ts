import { expect, test, type Page } from '@playwright/test';
import { enterMockedOrganization } from './mock-organization';
import { visit } from './ready';
import { signInAsAdmin } from './web-session';

async function openOrganization(page: Page) {
	await signInAsAdmin(page, [
		{
			id: 'org-byoa-e2e',
			name: 'Lumen Labs',
			created_at: '2026-11-18T00:00:00Z',
			recovery: 'acknowledged'
		}
	]);
	await enterMockedOrganization(page, 'Lumen Labs');
}

interface MockByoaState {
	active?: {
		session_id: string;
		employee_id: string;
		handle?: string;
		workspace_label: string;
		pubkey_fingerprint: string;
		approved_by: string;
		approved_at: string;
		expires_at: string;
		active: boolean;
		took_over_managed: boolean;
	};
	pending: {
		request_id: string;
		employee_id: string;
		handle?: string;
		office_id: string;
		office_name: string;
		hostname?: string;
		workspace_path?: string;
		workspace_label: string;
		pubkey_fingerprint: string;
		managed_turn_active: boolean;
		requested_at: string;
		expires_at: string;
		code_prefix: string;
		workspace_familiarity: 'first_time' | 'returning_new_key' | 'returning_known_key';
		last_approved_by?: string;
		last_approved_at?: string;
	}[];
	history: {
		at: string;
		event: string;
		session_id?: string;
		actor_id?: string;
		workspace_label?: string;
		hostname?: string;
		pubkey_fingerprint?: string;
	}[];
}

function initialPendingState(nowMs: number): MockByoaState {
	const requested = new Date(nowMs - 60_000).toISOString();
	const expires = new Date(nowMs + 9 * 60_000).toISOString();
	return {
		pending: [
			{
				request_id: 'req-first',
				employee_id: 'kai',
				handle: '@kai',
				office_id: 'off-hq',
				office_name: 'HQ Mac',
				hostname: 'field-laptop.local',
				workspace_path: '/Users/dev/new-sandbox',
				workspace_label: 'field-laptop:~/new-sandbox',
				pubkey_fingerprint: '1111222233334444',
				managed_turn_active: false,
				requested_at: requested,
				expires_at: expires,
				code_prefix: 'ABCD-••••',
				workspace_familiarity: 'first_time'
			},
			{
				request_id: 'req-new-key',
				employee_id: 'kai',
				handle: '@kai',
				office_id: 'off-hq',
				office_name: 'HQ Mac',
				hostname: 'studio-mac.local',
				workspace_path: '/Users/dev/agentic-org',
				workspace_label: 'studio-mac:~/agentic-org',
				pubkey_fingerprint: '5555666677778888',
				managed_turn_active: false,
				requested_at: requested,
				expires_at: expires,
				code_prefix: 'EFGH-••••',
				workspace_familiarity: 'returning_new_key',
				last_approved_by: 'Alex Rivera',
				last_approved_at: new Date(nowMs - 5 * 86_400_000).toISOString()
			},
			{
				request_id: 'req-known',
				employee_id: 'kai',
				handle: '@kai',
				office_id: 'off-hq',
				office_name: 'HQ Mac',
				hostname: 'workstation.local',
				workspace_path: '/Users/dev/core-workspace',
				workspace_label: 'workstation:~/core-workspace',
				pubkey_fingerprint: '9999aaaabbbbcccc',
				managed_turn_active: true,
				requested_at: requested,
				expires_at: expires,
				code_prefix: 'K7P4-••••',
				workspace_familiarity: 'returning_known_key',
				last_approved_by: 'Alex Rivera',
				last_approved_at: new Date(nowMs - 2 * 86_400_000).toISOString()
			}
		],
		history: [
			{
				at: new Date(nowMs - 2 * 86_400_000).toISOString(),
				event: 'expired',
				session_id: 'sess-prev',
				actor_id: 'Alex Rivera',
				workspace_label: 'workstation:~/core-workspace',
				hostname: 'workstation.local',
				pubkey_fingerprint: '9999aaaabbbbcccc'
			}
		]
	};
}

interface CapturedByoaPayloads {
	sessionGets: number;
	approve?: Record<string, unknown>;
	renew?: Record<string, unknown>;
	revoke?: Record<string, unknown>;
}

async function wireFullByoaRoutes(page: Page, state: MockByoaState): Promise<CapturedByoaPayloads> {
	const captured: CapturedByoaPayloads = { sessionGets: 0 };

	await page.route('**/api/v1/organizations/*/members/kai/external-sessions*', async (route) => {
		if (route.request().method() !== 'GET') return route.fallback();
		captured.sessionGets += 1;
		await route.fulfill({
			status: 200,
			contentType: 'application/json',
			body: JSON.stringify({
				result: {
					employee_id: 'kai',
					...(state.active ? { active: state.active } : {}),
					pending: state.pending,
					history: state.history
				}
			})
		});
	});

	await page.route('**/api/v1/organizations/*/external-auth/approve', async (route) => {
		captured.approve = JSON.parse(route.request().postData() ?? '{}') as Record<string, unknown>;
		state.active = {
			session_id: 'sess-live-1',
			employee_id: 'kai',
			handle: 'kai',
			workspace_label: 'workstation:~/core-workspace',
			pubkey_fingerprint: '9999aaaabbbbcccc',
			approved_by: 'Alex Rivera',
			approved_at: new Date().toISOString(),
			expires_at: new Date(Date.now() + 6 * 3600_000).toISOString(),
			active: true,
			took_over_managed: Boolean(captured.approve.takeover)
		};
		state.pending = state.pending.filter((p) => p.request_id !== 'req-known');
		state.history = [
			{
				at: new Date().toISOString(),
				event: 'approved',
				session_id: 'sess-live-1',
				actor_id: 'Alex Rivera',
				workspace_label: 'workstation:~/core-workspace',
				pubkey_fingerprint: '9999aaaabbbbcccc'
			},
			...state.history
		];
		await route.fulfill({
			status: 200,
			contentType: 'application/json',
			body: JSON.stringify({ result: state.active })
		});
	});

	await page.route(
		'**/api/v1/organizations/*/members/kai/external-sessions/renew',
		async (route) => {
			captured.renew = JSON.parse(route.request().postData() ?? '{}') as Record<string, unknown>;
			state.active = {
				...state.active!,
				expires_at: new Date(Date.now() + 7 * 86_400_000).toISOString()
			};
			state.history = [
				{
					at: new Date().toISOString(),
					event: 'renewed',
					session_id: 'sess-live-1',
					actor_id: 'Alex Rivera',
					workspace_label: 'workstation:~/core-workspace'
				},
				...state.history
			];
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({ result: state.active })
			});
		}
	);

	await page.route(
		'**/api/v1/organizations/*/members/kai/external-sessions/revoke',
		async (route) => {
			captured.revoke = JSON.parse(route.request().postData() ?? '{}') as Record<string, unknown>;
			state.active = undefined;
			state.history = [
				{
					at: new Date().toISOString(),
					event: 'revoked',
					session_id: 'sess-live-1',
					actor_id: 'Alex Rivera',
					workspace_label: 'workstation:~/core-workspace'
				},
				...state.history
			];
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					result: { employee_id: 'kai', revoked: true, session_id: 'sess-live-1' }
				})
			});
		}
	);

	return captured;
}

test('BYOA-1: pending familiarity badges, approve with takeover & TTL, active lease expiring-soon, Renew (+7d), and Revoke', async ({
	page
}) => {
	await openOrganization(page);
	const state = initialPendingState(Date.now());
	const captured = await wireFullByoaRoutes(page, state);

	await visit(page, '/members/kai');
	const section = page.getByTestId('external-auth-section');
	await expect(section).toBeVisible();

	// All 3 workspace familiarity badges, masked lookup codes, and managed-turn indicator render.
	await expect(section.getByText('First-time workspace')).toBeVisible();
	await expect(section.getByText('Returning workspace (new key)')).toBeVisible();
	await expect(section.getByText('Returning workspace (known key)')).toBeVisible();
	await expect(section.getByText('Managed turn active')).toBeVisible();
	await expect(section.getByText('K7P4-••••')).toBeVisible();
	await expect(section.getByText(/Last approved by Alex Rivera/).first()).toBeVisible();

	// Approve the managed-turn request with Crockford validation + 7d TTL + explicit takeover checkbox.
	const knownCard = section.locator('[data-request-id="req-known"]');
	await knownCard.getByRole('button', { name: 'Approve…' }).click();
	const dialog = page.getByRole('dialog');
	await expect(dialog).toBeVisible();
	// L1: Dialog renders both workspace_label and workspace_path.
	await expect(dialog).toContainText('workstation:~/core-workspace');
	await expect(dialog).toContainText('/Users/dev/core-workspace');

	const codeInput = dialog.getByLabel('Approval code');
	const submitBtn = dialog.getByRole('button', { name: 'Approve with takeover' });
	await expect(submitBtn).toBeDisabled();

	await codeInput.fill('u1234567');
	await dialog.getByRole('checkbox').click();
	await expect(submitBtn).toBeEnabled();
	await submitBtn.click();
	await expect(dialog.getByRole('alert')).toContainText('0–9 and A–Z');

	// M2: Pasting spaced codes (> 11 chars) is not truncated before formatting.
	const getsBeforeApprove = captured.sessionGets;
	await codeInput.fill('   k7p4  -  9m2x   ');
	await expect(codeInput).toHaveValue('K7P4-9M2X');
	await dialog.getByText('7d', { exact: true }).click();
	await submitBtn.click();
	await expect(dialog).toBeHidden();

	expect(captured.approve).toMatchObject({
		code: 'K7P4-9M2X',
		ttl_seconds: 604_800,
		takeover: true
	});
	expect(Object.keys(captured.approve ?? {}).sort()).toEqual([
		'code',
		'command_id',
		'takeover',
		'ttl_seconds'
	]);

	// Active External lease shows External badge + Expiring soon (< 24h) warning.
	const activeCard = section.getByTestId('external-auth-active-lease');
	await expect(activeCard).toBeVisible();
	// M3: Single GET reload on approve (no duplicate reload).
	expect(captured.sessionGets - getsBeforeApprove).toBe(1);
	await expect(activeCard.getByText('External', { exact: true })).toBeVisible();
	await expect(activeCard.getByText('Expiring soon (< 24h)')).toBeVisible();

	// 1-click Renew (+7d) extends lease and clears the <24h warning.
	await activeCard.getByRole('button', { name: 'Renew (+7d)' }).click();
	await expect(activeCard.getByText('Expiring soon (< 24h)')).toHaveCount(0);
	expect(captured.renew).toMatchObject({ session_id: 'sess-live-1', ttl_seconds: 604_800 });

	// Revoke via destructive confirmation dialog clears active lease and logs Revoked in history.
	const getsBeforeRevoke = captured.sessionGets;
	await activeCard.getByRole('button', { name: 'Revoke…' }).click();
	const confirm = page.getByRole('dialog');
	await expect(confirm).toContainText('Revoke external lease for Kai Lindqvist?');
	await confirm.getByRole('button', { name: 'Revoke lease' }).click();
	await expect(confirm).toBeHidden();
	expect(captured.revoke).toMatchObject({ session_id: 'sess-live-1' });
	await expect(section.getByTestId('external-auth-active-lease')).toHaveCount(0);
	await expect(section.getByTestId('external-auth-history-block')).toContainText('Revoked');
	// M3: Single GET reload on revoke (no duplicate reload).
	expect(captured.sessionGets - getsBeforeRevoke).toBe(1);
});

test(
	'BYOA-2: handles 429 retry-after and 409 takeover prompt, clears code on close, and restores focus on Escape',
	{ tag: '@webkit' },
	async ({ page }) => {
		await openOrganization(page);
		const state = initialPendingState(Date.now());
		state.pending = [state.pending[0]];
		let attempt = 0;

		await page.route('**/api/v1/organizations/*/members/kai/external-sessions*', async (route) => {
			if (route.request().method() === 'GET') {
				await route.fulfill({
					status: 200,
					contentType: 'application/json',
					body: JSON.stringify({
						result: { employee_id: 'kai', pending: state.pending, history: [] }
					})
				});
				return;
			}
			await route.fallback();
		});

		await page.route('**/api/v1/organizations/*/external-auth/approve', async (route) => {
			attempt += 1;
			if (attempt === 1) {
				await route.fulfill({
					status: 429,
					contentType: 'application/json',
					body: JSON.stringify({
						error: {
							code: 'rate_limited',
							message: 'Too many attempts.',
							retry_after_ms: 45_000
						}
					})
				});
				return;
			}
			await route.fulfill({
				status: 409,
				contentType: 'application/json',
				body: JSON.stringify({
					error: { code: 'conflict', message: 'Managed turn active.' }
				})
			});
		});

		// Navigate via real SPA link from /teams/members -> /members/kai so Back tests registerModalBack.
		await page.getByRole('link', { name: /Kai Lindqvist/ }).click();
		await expect(page).toHaveURL(/\/members\/kai$/);
		const approveBtn = page
			.getByTestId('external-auth-section')
			.getByRole('button', { name: 'Approve…' });
		await approveBtn.click();

		const dialog = page.getByRole('dialog');
		const codeInput = dialog.getByLabel('Approval code');
		await expect(codeInput).toBeFocused();
		await codeInput.fill('abcd1234');

		// M4: Browser Back dismisses the open modal without navigating away, clears code, and restores focus.
		await page.goBack();
		await expect(dialog).toHaveCount(0);
		await expect(page).toHaveURL(/\/members\/kai$/);
		await expect(approveBtn).toBeFocused();

		await approveBtn.click();
		await expect(dialog.getByLabel('Approval code')).toHaveValue('');
		await codeInput.fill('abcd1234');

		// 429 surfaces retry seconds from error.retry_after_ms.
		await dialog.getByRole('button', { name: 'Approve', exact: true }).click();
		await expect(dialog.getByRole('alert')).toContainText('Try again in 45 s.');

		// 409 reveals managed-turn takeover confirmation.
		await dialog.getByRole('button', { name: 'Approve', exact: true }).click();
		await expect(dialog.getByRole('alert')).toContainText('Confirm takeover');
		await expect(dialog.getByRole('checkbox')).toBeVisible();

		// Escape closes dialog and restores focus to the opener button; reopening starts with empty code.
		await page.keyboard.press('Escape');
		await expect(dialog).toHaveCount(0);
		await expect(approveBtn).toBeFocused();

		await approveBtn.click();
		await expect(page.getByRole('dialog').getByLabel('Approval code')).toHaveValue('');
		await page.keyboard.press('Escape');
		await expect(approveBtn).toBeFocused();
	}
);

test('BYOA-3: 320px narrow viewport, 200% text scaling, and reduced motion stay free of horizontal overflow', async ({
	page
}) => {
	await page.emulateMedia({ reducedMotion: 'reduce' });
	await openOrganization(page);
	const nowMs = Date.now();
	const state = initialPendingState(nowMs);
	state.active = {
		session_id: 'sess-reflow-0193a5c0-0000-7000-8000-0000000000aa',
		employee_id: 'kai',
		handle: 'kai',
		workspace_label: 'studio-mac:/Users/developer/very-long-workspace-directory-name/agentic-org',
		pubkey_fingerprint: 'abcdef0123456789',
		approved_by: 'Alex Rivera',
		approved_at: new Date(nowMs - 3600_000).toISOString(),
		expires_at: new Date(nowMs + 5 * 3600_000).toISOString(),
		active: true,
		took_over_managed: false
	};

	await page.route('**/api/v1/organizations/*/members/kai/external-sessions*', async (route) => {
		if (route.request().method() === 'GET') {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					result: {
						employee_id: 'kai',
						active: state.active,
						pending: state.pending,
						history: state.history
					}
				})
			});
			return;
		}
		await route.fallback();
	});

	await page.setViewportSize({ width: 320, height: 640 });
	await visit(page, '/members/kai');
	const section = page.getByTestId('external-auth-section');
	await expect(section.getByTestId('external-auth-active-lease')).toBeVisible();

	const overflow320 = await page.evaluate(
		() => document.documentElement.scrollWidth - document.documentElement.clientWidth
	);
	expect(overflow320).toBeLessThanOrEqual(1);

	// Simulate 200% text reflow at desktop width and check both section and open Approve dialog.
	await page.setViewportSize({ width: 1280, height: 800 });
	await page.addStyleTag({ content: 'html { font-size: 32px !important; }' });
	const overflow200 = await page.evaluate(
		() => document.documentElement.scrollWidth - document.documentElement.clientWidth
	);
	expect(overflow200).toBeLessThanOrEqual(1);

	await section
		.locator('[data-request-id="req-known"]')
		.getByRole('button', { name: 'Approve…' })
		.click();
	const dialog = page.getByRole('dialog');
	await expect(dialog).toBeVisible();
	const dialogBox = await dialog.boundingBox();
	expect(dialogBox).not.toBeNull();
	expect(dialogBox!.width).toBeLessThanOrEqual(1280);
});

test(
	'BYOA-4: approving a BYOA item in Inbox removes it cleanly without stranding focus on <body> (H2)',
	{ tag: '@webkit' },
	async ({ page }) => {
		await openOrganization(page);
		const nowMs = Date.now();
		let pending = [initialPendingState(nowMs).pending[2]];

		await page.route('**/api/v1/organizations/*/external-auth/pending*', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({ result: { pending } })
			});
		});

		await page.route('**/api/v1/organizations/*/external-auth/approve', async (route) => {
			pending = [];
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					result: {
						session_id: 'sess-inbox-1',
						employee_id: 'kai',
						handle: 'kai',
						workspace_label: 'workstation:~/core-workspace',
						pubkey_fingerprint: '9999aaaabbbbcccc',
						approved_by: 'Alex Rivera',
						approved_at: new Date().toISOString(),
						expires_at: new Date(Date.now() + 86_400_000).toISOString(),
						active: true,
						took_over_managed: true
					}
				})
			});
		});

		await visit(page, '/inbox?mock=external-auth');
		const byoaRow = page.locator('[data-triage-row="external-auth:pending:req-known"]');
		await expect(byoaRow).toBeVisible();
		await byoaRow.click();

		const actions = page.getByTestId('inbox-external-auth-actions');
		await expect(actions).toBeVisible();
		await actions.getByRole('button', { name: 'Approve sign-in…' }).click();

		const dialog = page.getByRole('dialog');
		await expect(dialog).toBeVisible();

		// Complete approval with spaced paste + takeover; BYOA item disappears on refresh and focus never strands on <body>.
		await dialog.getByLabel('Approval code').fill('  k7p4   9m2x  ');
		await dialog.getByRole('checkbox').click();
		await dialog.getByRole('button', { name: 'Approve with takeover' }).click();

		await expect(dialog).toHaveCount(0);
		await expect(byoaRow).toHaveCount(0);

		// Assert focus moved to a real Inbox element (heading / row / page title), NEVER stranded on <body>.
		await expect
			.poll(() =>
				page.evaluate(() => ({
					tag: document.activeElement?.tagName ?? 'NONE',
					id: document.activeElement?.id ?? ''
				}))
			)
			.not.toMatchObject({ tag: 'BODY' });
	}
);
