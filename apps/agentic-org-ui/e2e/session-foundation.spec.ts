import { expect, test, type BrowserContext, type Page } from '@playwright/test';
import type { Harness } from './session-browser-harness';
import { sessionCookieName, sessionView as view, type SessionPair } from './fakes/session-server';

declare global {
	interface Window {
		p6: Harness;
	}
}
const lowerHex64 = /^[0-9a-f]{64}$/;
const denied = { error: { code: 'not_authorized', session_reason: 'session_ended' } };
async function mount(page: Page) {
	await page.goto('/welcome');
	await expect(page.locator('html')).toHaveAttribute('data-hydrated', 'true');
	await page.evaluate(async () => {
		const source = '/e2e/session-browser-harness.ts';
		const module = await import(/* @vite-ignore */ source);
		window.p6 = module.createHarness();
	});
}
async function backend(context: BrowserContext) {
	const response = await context.request.post('/__p6-auth/new');
	expect(response.ok()).toBe(true);
	const { id } = await response.json();
	await context.addCookies([
		{ name: 'p6_auth_case', value: id, url: new URL('/', response.url()).href }
	]);
	const stats = async (): Promise<{
		refreshCount: number;
		gets: number;
		requestHeaders: Record<string, string>[];
		issued: SessionPair[];
	}> => {
		const response = await context.request.get(`/__p6-auth/control?id=${id}`);
		expect(response.ok()).toBe(true);
		return response.json();
	};
	const control = async (action: string, value?: unknown) => {
		expect(
			(await context.request.post('/__p6-auth/control', { data: { id, action, value } })).ok()
		).toBe(true);
	};
	return {
		stats,
		refreshCount: async () => (await stats()).refreshCount,
		gets: async () => (await stats()).gets,
		issued: async () => (await stats()).issued,
		holdRefresh: () => control('hold'),
		releaseRefresh: () => control('release'),
		setSignOutFails: (value: boolean) => control('signOutFails', value),
		setSignOutRefused: (value: 400 | 403 | null) => control('signOutRefused', value),
		setRevision: (value: number) => control('revision', value)
	};
}
/** The HttpOnly cookie half, visible only to the test runner, never to page script. */
const browserCookie = async (context: BrowserContext) =>
	(await context.cookies()).find((cookie) => cookie.name === sessionCookieName);
async function setBrowserCookie(context: BrowserContext, page: Page, value: string) {
	await context.addCookies([
		{
			name: sessionCookieName,
			value,
			domain: new URL(page.url()).hostname,
			path: '/api/v1',
			httpOnly: true,
			sameSite: 'Strict'
		}
	]);
}
/** A native same-origin protected read: the browser attaches the cookie and the caller chooses the proof header. */
const getSession = (page: Page, proof?: string) =>
	page.evaluate(async (proof) => {
		const response = await fetch('/api/v1/session', {
			headers: proof === undefined ? {} : { 'X-Org-Session-Proof': proof },
			credentials: 'same-origin'
		});
		return { status: response.status, body: await response.json() };
	}, proof);
const storedProof = (page: Page) =>
	page.evaluate(async () => (await window.p6.store.read())?.proof);

test.describe('P6a auth foundation @webkit', () => {
	test('SIGN-7/14/16: native storage, secret-free broadcasts and one cross-tab refresh lock', async ({
		page,
		context
	}) => {
		const api = await backend(context);
		const second = await context.newPage();
		await mount(page);
		await mount(second);
		await page.evaluate(() =>
			window.p6.controller.signIn({ login: 'alex', password: 'private-password' })
		);
		await expect.poll(() => second.evaluate(() => window.p6.states.at(-1)?.kind)).toBe('signed-in');
		await api.holdRefresh();
		const trigger = Date.now();
		const a = page.evaluate((time) => window.p6.controller.refresh(time), trigger);
		await expect.poll(api.refreshCount).toBe(1);
		const b = second.evaluate((time) => window.p6.controller.refresh(time), trigger);
		await expect
			.poll(() => second.evaluate(async () => (await navigator.locks.query()).pending?.length))
			.toBe(1);
		await api.releaseRefresh();
		await Promise.all([a, b]);
		expect(await api.refreshCount()).toBe(1);
		const issued = await api.issued();
		expect(issued).toHaveLength(2);
		const [firstPair, secondPair] = issued;
		await expect.poll(() => storedProof(second)).toBe(secondPair.proof);
		const protectedRequests = (await api.stats()).requestHeaders.filter(
			(headers) => headers['x-org-session-proof']
		);
		expect(protectedRequests.length).toBeGreaterThan(0);
		// Each protected request carries the proof header plus the separately issued cookie of the same pair; the cookie never repeats the proof.
		for (const headers of protectedRequests) {
			const pair = issued.find((pair) => pair.proof === headers['x-org-session-proof']);
			expect(pair).toBeDefined();
			expect(headers.cookie).toContain(`${sessionCookieName}=${pair!.cookie}`);
			expect(headers.cookie).not.toContain(pair!.proof);
		}
		const messages = await second.evaluate(() => window.p6.broadcasts);
		expect(messages).toContainEqual({ kind: 'signed-in', account_id: 'alex-id' });
		for (const secret of [firstPair.proof, secondPair.proof, firstPair.cookie, secondPair.cookie])
			expect(JSON.stringify(messages)).not.toContain(secret);
		const states = JSON.stringify(await page.evaluate(() => window.p6.states));
		for (const secret of [secondPair.proof, firstPair.cookie, secondPair.cookie])
			expect(states).not.toContain(secret);
		await api.setSignOutFails(true);
		expect(
			await page.evaluate(() =>
				window.p6.controller.signOut().then(
					() => 'unexpected',
					(error) => error.message
				)
			)
		).toBe('Session request failed');
		// The 503 reply cleared the cookie half, so this browser's pair ends in every tab despite the refusal.
		expect(await storedProof(page)).toBeUndefined();
		expect(await page.evaluate(() => window.p6.states.at(-1))).toEqual({
			kind: 'signed-out',
			reason: 'signed-out'
		});
		await expect
			.poll(() => second.evaluate(() => window.p6.states.at(-1)))
			.toEqual({ kind: 'signed-out', reason: 'signed-out' });
		expect(await second.evaluate(() => window.p6.store.read())).toBeUndefined();
		expect(await browserCookie(context)).toBeUndefined();
		// The binding was invalidated before the 503: the old pair no longer authenticates.
		await setBrowserCookie(context, page, secondPair.cookie);
		expect(await getSession(page, secondPair.proof)).toMatchObject({ status: 401, body: denied });
	});
	for (const status of [400, 403] as const)
		test(`SIGN-OUT ${status}: a refusal before authentication changes nothing`, async ({
			page,
			context
		}) => {
			const api = await backend(context);
			await mount(page);
			await page.evaluate(() =>
				window.p6.controller.signIn({ login: 'alex', password: 'private-password' })
			);
			const [pair] = await api.issued();
			const statesBefore = await page.evaluate(() => window.p6.states.length);
			await api.setSignOutRefused(status);
			expect(
				await page.evaluate(() =>
					window.p6.controller.signOut().then(
						() => 'unexpected',
						(error) => `${error.message} ${error.status}`
					)
				)
			).toBe(`Session request failed ${status}`);
			expect((await browserCookie(context))?.value).toBe(pair.cookie);
			expect(await storedProof(page)).toBe(pair.proof);
			expect(await page.evaluate(() => window.p6.states.length)).toBe(statesBefore);
			expect(await page.evaluate(() => window.p6.broadcasts)).not.toContainEqual({
				kind: 'signed-out'
			});
			expect((await getSession(page, pair.proof)).status).toBe(200);
		});
	test('unknown /api/v1 paths are a JSON 404 decided by shape before the pair', async ({
		page,
		context
	}) => {
		await backend(context);
		await mount(page);
		for (const path of ['/api/v1/session/other', '/api/v1/session/sign-out/x', '/api/v1/tasks'])
			expect(
				await page.evaluate(async (path) => {
					const response = await fetch(path, { method: 'POST', body: '{}' });
					return { status: response.status, body: await response.json() };
				}, path)
			).toEqual({ status: 404, body: { error: { code: 'not_found', message: 'Not found.' } } });
	});
	test('SIGN-7: a reload recovers the matching cookie/proof pair from actual IndexedDB', async ({
		page,
		context
	}) => {
		const api = await backend(context);
		await mount(page);
		await page.evaluate(() =>
			window.p6.controller.signIn({ login: 'alex', password: 'private-password' })
		);
		await page.evaluate(() => window.p6.dispose());
		await mount(page);
		await page.evaluate(() => window.p6.controller.boot());
		expect(await page.evaluate(() => window.p6.states.at(-1))).toEqual({
			kind: 'signed-in',
			session: view
		});
		expect(await api.gets()).toBe(1);
		const [firstPair] = await api.issued();
		const request = (await api.stats()).requestHeaders.find(
			(headers) => headers['x-org-session-proof'] === firstPair.proof
		);
		expect(request?.cookie).toContain(`${sessionCookieName}=${firstPair.cookie}`);
		expect(request?.cookie).not.toContain(firstPair.proof);
	});
	test('SIGN-10/14: another account clears old views and later rotation cannot bypass Reload', async ({
		page,
		context
	}) => {
		await backend(context);
		const second = await context.newPage();
		await mount(page);
		await mount(second);
		await page.evaluate(() =>
			window.p6.controller.signIn({ login: 'alex', password: 'private-password' })
		);
		await expect.poll(() => second.evaluate(() => window.p6.states.at(-1)?.kind)).toBe('signed-in');
		await second.evaluate(() =>
			window.p6.controller.signIn({ login: 'Blair', password: 'private-password' })
		);
		await expect
			.poll(() => page.evaluate(() => window.p6.states.at(-1)))
			.toEqual({ kind: 'signed-out', reason: 'changed-account', login: 'Blair' });
		await second.evaluate(() => {
			const channel = new BroadcastChannel('org-session');
			channel.postMessage({ kind: 'rotated' });
			channel.close();
		});
		await expect
			.poll(() =>
				page.evaluate(() =>
					window.p6.broadcasts.some((value) => (value as { kind?: string }).kind === 'rotated')
				)
			)
			.toBe(true);
		expect(await page.evaluate(() => window.p6.states.at(-1))).toEqual({
			kind: 'signed-out',
			reason: 'changed-account',
			login: 'Blair'
		});
		await page.evaluate(() => window.p6.controller.boot());
		expect(await page.evaluate(() => window.p6.states.at(-1))).toEqual({
			kind: 'signed-out',
			reason: 'changed-account',
			login: 'Blair'
		});
	});
	test('SIGN-7: unrelated named and nameless cookies never deny the session pair', async ({
		page,
		context,
		browserName
	}) => {
		const api = await backend(context);
		await mount(page);
		await page.evaluate(() =>
			window.p6.controller.signIn({ login: 'alex', password: 'private-password' })
		);
		const [pair] = await api.issued();
		expect((await getSession(page, pair.proof)).status).toBe(200);
		// Another loopback app's cookies: one without a name (no `=`) and one named.
		await page.evaluate(() => {
			document.cookie = 'orphan-value; path=/';
			document.cookie = 'other_app=1; path=/';
		});
		const jar = await page.evaluate(() => document.cookie);
		expect(jar).toContain('other_app=1');
		// WebKit does not store a nameless cookie; Chromium does, so it covers the
		// nameless case while both browsers cover an unrelated named cookie.
		if (browserName === 'chromium') expect(jar).toContain('orphan-value');
		expect((await getSession(page, pair.proof)).status).toBe(200);
	});
	test('SIGN-7: the real server refuses cookie-only, wrong-proof and proof-only recovery', async ({
		page,
		context
	}) => {
		const api = await backend(context);
		await mount(page);
		await page.evaluate(() =>
			window.p6.controller.signIn({ login: 'alex', password: 'private-password' })
		);
		const [firstPair] = await api.issued();
		const cookie = await browserCookie(context);
		expect(cookie).toMatchObject({
			value: firstPair.cookie,
			httpOnly: true,
			sameSite: 'Strict',
			path: '/api/v1',
			secure: false
		});
		// Independent values: the proof reaches the browser only in the response body (then IndexedDB) and the request header.
		expect(sessionCookieName).toMatch(/^org_session_[0-9a-f]{16}$/);
		expect(cookie!.value).toMatch(lowerHex64);
		expect(firstPair.proof).toMatch(lowerHex64);
		expect(cookie!.value).not.toBe(firstPair.proof);
		expect((await context.cookies()).some((cookie) => cookie.value.includes(firstPair.proof))).toBe(
			false
		);
		expect(await page.evaluate(() => document.cookie)).not.toContain(sessionCookieName);
		expect(await storedProof(page)).toBe(firstPair.proof);
		expect((await getSession(page, firstPair.proof)).status).toBe(200);
		const cookieOnly = await getSession(page);
		expect(cookieOnly.status).toBe(401);
		expect(cookieOnly.body).toMatchObject(denied);
		expect(await getSession(page, 'wrong')).toMatchObject({ status: 401, body: denied });
		expect(await getSession(page, firstPair.cookie)).toMatchObject({ status: 401, body: denied });
		await context.clearCookies({ name: sessionCookieName });
		expect(await getSession(page, firstPair.proof)).toMatchObject({ status: 401, body: denied });
	});
	test('P-8: refresh rotates both values, stale halves are refused and sign-out clears the pair', async ({
		page,
		context
	}) => {
		const api = await backend(context);
		await mount(page);
		await page.evaluate(() =>
			window.p6.controller.signIn({ login: 'alex', password: 'private-password' })
		);
		const [firstPair] = await api.issued();
		expect((await browserCookie(context))?.value).toBe(firstPair.cookie);
		await page.evaluate((time) => window.p6.controller.refresh(time), Date.now());
		expect(await api.refreshCount()).toBe(1);
		const issued = await api.issued();
		expect(issued).toHaveLength(2);
		const secondPair = issued[1];
		expect(secondPair.cookie).not.toBe(firstPair.cookie);
		expect(secondPair.proof).not.toBe(firstPair.proof);
		expect(secondPair.cookie).not.toBe(secondPair.proof);
		expect((await browserCookie(context))?.value).toBe(secondPair.cookie);
		expect(await storedProof(page)).toBe(secondPair.proof);
		expect(await getSession(page, firstPair.proof)).toMatchObject({ status: 401, body: denied });
		expect((await getSession(page, secondPair.proof)).status).toBe(200);
		await setBrowserCookie(context, page, firstPair.cookie);
		expect(await getSession(page, secondPair.proof)).toMatchObject({ status: 401, body: denied });
		expect(await getSession(page, firstPair.proof)).toMatchObject({ status: 401, body: denied });
		await setBrowserCookie(context, page, secondPair.cookie);
		expect((await getSession(page, secondPair.proof)).status).toBe(200);
		await page.evaluate(() => window.p6.controller.signOut());
		expect(await browserCookie(context)).toBeUndefined();
		expect(await page.evaluate(() => window.p6.store.read())).toBeUndefined();
		expect(await getSession(page, secondPair.proof)).toMatchObject({ status: 401, body: denied });
		await setBrowserCookie(context, page, secondPair.cookie);
		expect(await getSession(page, secondPair.proof)).toMatchObject({ status: 401, body: denied });
	});
	test('SIGN-7: a missing persisted proof never falls back to the cookie', async ({
		page,
		context
	}) => {
		const api = await backend(context);
		await mount(page);
		await page.evaluate(() =>
			window.p6.controller.signIn({ login: 'alex', password: 'private-password' })
		);
		await page.evaluate(async () => {
			const request = indexedDB.open('org-session', 1);
			await new Promise<void>((resolve, reject) => {
				request.onerror = () => reject(request.error);
				request.onsuccess = () => {
					const db = request.result,
						tx = db.transaction('session', 'readwrite');
					tx.objectStore('session').put(
						{ instance_id: 'hq', account_id: 'alex-id', login: 'alex', rotated_at_ms: 100 },
						'current'
					);
					tx.oncomplete = () => {
						db.close();
						resolve();
					};
					tx.onabort = () => {
						db.close();
						reject(tx.error);
					};
				};
			});
			await window.p6.controller.boot();
		});
		expect(await api.gets()).toBe(0);
		expect(await page.evaluate(() => window.p6.states.at(-1))).toEqual({
			kind: 'signed-out',
			reason: 'ended'
		});
	});
	test('SIGN-7: revision 1 stops before reading a stored session', async ({ page, context }) => {
		const api = await backend(context);
		await mount(page);
		await page.evaluate(() =>
			window.p6.controller.signIn({ login: 'alex', password: 'private-password' })
		);
		await api.setRevision(1);
		await page.evaluate(() => window.p6.controller.boot());
		expect(await api.gets()).toBe(0);
		expect(await page.evaluate(() => window.p6.states.at(-1))).toEqual({ kind: 'update-required' });
		const [firstPair] = await api.issued();
		expect(await storedProof(page)).toBe(firstPair.proof);
		await page.evaluate(() => {
			const channel = new BroadcastChannel('org-session');
			channel.postMessage({ kind: 'rotated' });
			channel.close();
		});
		await expect
			.poll(() =>
				page.evaluate(() =>
					window.p6.broadcasts.some((value) => (value as { kind?: string }).kind === 'rotated')
				)
			)
			.toBe(true);
		expect(await api.gets()).toBe(0);
		expect(await page.evaluate(() => window.p6.states.at(-1))).toEqual({ kind: 'update-required' });
	});
});
