import { expect, test, type BrowserContext, type Page } from '@playwright/test';
import type { Harness } from './session-browser-harness';
import { sessionView as view } from './fakes/session-server';

declare global { interface Window { p6: Harness } }
const firstProof = 'a'.repeat(64), secondProof = 'b'.repeat(64);
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
	const response = await context.request.post('/__p6-auth/new'); expect(response.ok()).toBe(true);
	const { id } = await response.json();
	await context.addCookies([{ name: 'p6_auth_case', value: id, url: new URL('/', response.url()).href }]);
	const stats = async (): Promise<{ refreshCount: number; gets: number; requestHeaders: Record<string, string>[] }> => {
		const response = await context.request.get(`/__p6-auth/control?id=${id}`); expect(response.ok()).toBe(true); return response.json();
	};
	const control = async (action: string, value?: unknown) => { expect((await context.request.post('/__p6-auth/control', { data: { id, action, value } })).ok()).toBe(true); };
	return { stats, refreshCount: async () => (await stats()).refreshCount, gets: async () => (await stats()).gets,
		holdRefresh: () => control('hold'), releaseRefresh: () => control('release'),
		setSignOutFails: (value: boolean) => control('signOutFails', value), setRevision: (value: number) => control('revision', value) };
}

test.describe('P6a auth foundation @webkit', () => {
	test('SIGN-7/14/16: native storage, secret-free broadcasts and one cross-tab refresh lock', async ({ page, context }) => {
		const api = await backend(context); const second = await context.newPage(); await mount(page); await mount(second);
		await page.evaluate(() => window.p6.controller.signIn({ login: 'alex', password: 'private-password' }));
		await expect.poll(() => second.evaluate(() => window.p6.states.at(-1)?.kind)).toBe('signed-in');
		await api.holdRefresh(); const trigger = Date.now();
		const a = page.evaluate(time => window.p6.controller.refresh(time), trigger);
		await expect.poll(api.refreshCount).toBe(1);
		const b = second.evaluate(time => window.p6.controller.refresh(time), trigger);
		await expect.poll(() => second.evaluate(async () => (await navigator.locks.query()).pending?.length)).toBe(1);
		await api.releaseRefresh(); await Promise.all([a, b]); expect(await api.refreshCount()).toBe(1);
		await expect.poll(() => second.evaluate(async () => (await window.p6.store.read())?.proof)).toBe(secondProof);
		const protectedRequests = (await api.stats()).requestHeaders.filter(headers => headers['x-org-session-proof']);
		expect(protectedRequests.length).toBeGreaterThan(0);
		for (const headers of protectedRequests) expect(headers.cookie).toContain(`org_session=${headers['x-org-session-proof']}`);
		const messages = await second.evaluate(() => window.p6.broadcasts);
		expect(messages).toContainEqual({ kind: 'signed-in', account_id: 'alex-id' });
		expect(JSON.stringify(messages)).not.toContain(firstProof); expect(JSON.stringify(messages)).not.toContain(secondProof);
		expect(JSON.stringify(await page.evaluate(() => window.p6.states))).not.toContain(secondProof);
		await api.setSignOutFails(true);
		expect(await page.evaluate(() => window.p6.controller.signOut().then(() => 'unexpected', error => error.message))).toBe('Session request failed');
		expect(await page.evaluate(async () => (await window.p6.store.read())?.proof)).toBe(secondProof);
		await api.setSignOutFails(false); await page.evaluate(() => window.p6.controller.signOut());
		await expect.poll(() => second.evaluate(() => window.p6.states.at(-1))).toEqual({ kind: 'signed-out', reason: 'signed-out' });
		expect(await second.evaluate(() => window.p6.store.read())).toBeUndefined();
	});
	test('SIGN-7: a reload recovers the matching cookie/proof pair from actual IndexedDB', async ({ page, context }) => {
		const api = await backend(context); await mount(page);
		await page.evaluate(() => window.p6.controller.signIn({ login: 'alex', password: 'private-password' }));
		await page.evaluate(() => window.p6.dispose()); await mount(page); await page.evaluate(() => window.p6.controller.boot());
		expect(await page.evaluate(() => window.p6.states.at(-1))).toEqual({ kind: 'signed-in', session: view }); expect(await api.gets()).toBe(1);
		const request = (await api.stats()).requestHeaders.find(headers => headers['x-org-session-proof'] === firstProof);
		expect(request?.cookie).toContain(`org_session=${firstProof}`);
	});
	test('SIGN-10/14: another account clears old views and later rotation cannot bypass Reload', async ({ page, context }) => {
		await backend(context); const second = await context.newPage(); await mount(page); await mount(second);
		await page.evaluate(() => window.p6.controller.signIn({ login: 'alex', password: 'private-password' }));
		await expect.poll(() => second.evaluate(() => window.p6.states.at(-1)?.kind)).toBe('signed-in');
		await second.evaluate(() => window.p6.controller.signIn({ login: 'Blair', password: 'private-password' }));
		await expect.poll(() => page.evaluate(() => window.p6.states.at(-1))).toEqual({ kind: 'signed-out', reason: 'changed-account', login: 'Blair' });
		await second.evaluate(() => { const channel = new BroadcastChannel('org-session'); channel.postMessage({ kind: 'rotated' }); channel.close(); });
		await expect.poll(() => page.evaluate(() => window.p6.broadcasts.some(value => (value as { kind?: string }).kind === 'rotated'))).toBe(true);
		expect(await page.evaluate(() => window.p6.states.at(-1))).toEqual({ kind: 'signed-out', reason: 'changed-account', login: 'Blair' });
		await page.evaluate(() => window.p6.controller.boot());
		expect(await page.evaluate(() => window.p6.states.at(-1))).toEqual({ kind: 'signed-out', reason: 'changed-account', login: 'Blair' });
	});
	test('SIGN-7: the real server refuses cookie-only, wrong-proof and proof-only recovery', async ({ page, context }) => {
		await backend(context); await mount(page);
		await page.evaluate(() => window.p6.controller.signIn({ login: 'alex', password: 'private-password' }));
		const cookie = (await context.cookies()).find(cookie => cookie.name === 'org_session');
		expect(cookie).toMatchObject({ httpOnly: true, sameSite: 'Strict', path: '/api/v1', secure: false });
		expect(await page.evaluate(async () => (await fetch('/api/v1/session', { credentials: 'same-origin' })).status)).toBe(401);
		expect(await page.evaluate(async () => (await fetch('/api/v1/session', { headers: { 'X-Org-Session-Proof': 'wrong' }, credentials: 'same-origin' })).status)).toBe(401);
		await context.clearCookies({ name: 'org_session' });
		expect(await page.evaluate(async proof => (await fetch('/api/v1/session', { headers: { 'X-Org-Session-Proof': proof }, credentials: 'same-origin' })).status, firstProof)).toBe(401);
	});
	test('SIGN-7: a missing persisted proof never falls back to the cookie', async ({ page, context }) => {
		const api = await backend(context); await mount(page);
		await page.evaluate(() => window.p6.controller.signIn({ login: 'alex', password: 'private-password' }));
		await page.evaluate(async () => {
			const request = indexedDB.open('org-session', 1);
			await new Promise<void>((resolve, reject) => { request.onerror = () => reject(request.error); request.onsuccess = () => {
				const db = request.result, tx = db.transaction('session', 'readwrite');
				tx.objectStore('session').put({ instance_id: 'hq', account_id: 'alex-id', login: 'alex', rotated_at_ms: 100 }, 'current');
				tx.oncomplete = () => { db.close(); resolve(); }; tx.onabort = () => { db.close(); reject(tx.error); };
			}; });
			await window.p6.controller.boot();
		});
		expect(await api.gets()).toBe(0); expect(await page.evaluate(() => window.p6.states.at(-1))).toEqual({ kind: 'signed-out', reason: 'ended' });
	});
	test('SIGN-7: revision 1 stops before reading a stored session', async ({ page, context }) => {
		const api = await backend(context); await mount(page);
		await page.evaluate(() => window.p6.controller.signIn({ login: 'alex', password: 'private-password' })); await api.setRevision(1);
		await page.evaluate(() => window.p6.controller.boot()); expect(await api.gets()).toBe(0);
		expect(await page.evaluate(() => window.p6.states.at(-1))).toEqual({ kind: 'update-required' });
		expect(await page.evaluate(async () => (await window.p6.store.read())?.proof)).toBe(firstProof);
		await page.evaluate(() => { const channel = new BroadcastChannel('org-session'); channel.postMessage({ kind: 'rotated' }); channel.close(); });
		await expect.poll(() => page.evaluate(() => window.p6.broadcasts.some(value => (value as { kind?: string }).kind === 'rotated'))).toBe(true);
		expect(await api.gets()).toBe(0); expect(await page.evaluate(() => window.p6.states.at(-1))).toEqual({ kind: 'update-required' });
	});
});
