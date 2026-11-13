import { afterEach, describe, expect, it, vi } from 'vitest';
import { createGatewayPublicHttp, createJsonHttp } from './json-http';
import { NotDispatchedError } from './errors';

const origin = 'http://localhost:4173';
const credentials = { login: 'alex', password: 'private-password' };
function setup(response = () => Response.json({ result: {} })) {
	const fetch = vi.fn<typeof globalThis.fetch>(async () => response());
	const options = { origin, fetch, timeoutMs: 1000, maxRequestBytes: 256, maxResponseBytes: 256 };
	return { fetch, options, publicHttp: createGatewayPublicHttp(options), protectedHttp: createJsonHttp(options) };
}
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

describe('P6a public gateway transport', () => {
	it('reads only the fixed public version endpoint without a proof', async () => {
		const { publicHttp, fetch } = setup();
		await publicHttp.version();
		const [url, init] = fetch.mock.calls[0];
		expect(String(url)).toBe(origin + '/api/v1/version');
		expect(init).toMatchObject({ method: 'GET', credentials: 'same-origin', mode: 'same-origin', redirect: 'error', cache: 'no-store' });
		expect(init?.body).toBeUndefined();
		expect(new Headers(init?.headers).get('X-Org-Session-Proof')).toBeNull();
	});
	it('posts validated credentials only to sign-in, ignoring attempted target/header overrides', async () => {
		const { publicHttp, fetch } = setup();
		await publicHttp.signIn({ ...credentials, path: 'https://other.example/', method: 'GET', proof: 'injected-proof' });
		const [url, init] = fetch.mock.calls[0];
		expect(String(url)).toBe(origin + '/api/v1/session/sign-in');
		expect(String(url)).not.toContain(credentials.password);
		expect(init).toMatchObject({ method: 'POST', credentials: 'same-origin', mode: 'same-origin', redirect: 'error', cache: 'no-store' });
		expect(JSON.parse(init!.body as string)).toEqual(credentials);
		expect(new Headers(init?.headers).get('Content-Type')).toBe('application/json');
		expect(new Headers(init?.headers).get('X-Org-Session-Proof')).toBeNull();
	});
	it.each([null, {}, { login: 'alex', password: '' }, { login: 'alex', password: 123 }])('refuses invalid credentials before fetch: %j', async (input) => {
		const { publicHttp, fetch } = setup();
		await expect(publicHttp.signIn(input)).rejects.toBeInstanceOf(NotDispatchedError);
		expect(fetch).not.toHaveBeenCalled();
	});
	it('does not echo a hostile credential getter', async () => {
		const { publicHttp, fetch } = setup();
		const input = { login: 'alex', get password() { throw new Error(credentials.password); } };
		const error = await publicHttp.signIn(input).catch(error => error);
		expect(error).toBeInstanceOf(NotDispatchedError);
		expect(String(error)).not.toContain(credentials.password);
		expect(fetch).not.toHaveBeenCalled();
	});
	it('does not mistake an absent proof for the literal word undefined in a password', async () => {
		const { publicHttp, fetch } = setup();
		const password = 'undefined-is-valid-password-text';
		await publicHttp.signIn({ login: 'alex', password });
		expect(JSON.parse(fetch.mock.calls[0][1]!.body as string)).toEqual({ login: 'alex', password });
	});
	it.each(['/api/v1/version', '/api/v1/session/sign-in', '/api/v1/capabilities'])('keeps the protected API proof-required for %s', async path => {
		const { protectedHttp, fetch } = setup();
		// @ts-expect-error Forged JavaScript cannot use this API as a cookie-only request path.
		await expect(protectedHttp({ path, method: 'GET' })).rejects.toBeInstanceOf(NotDispatchedError);
		expect(fetch).not.toHaveBeenCalled();
		await protectedHttp({ path, method: 'GET', proof: 'valid-opaque-proof' });
		expect(new Headers(fetch.mock.calls[0][1]?.headers).get('X-Org-Session-Proof')).toBe('valid-opaque-proof');
	});
	it('does not permit a caller-configured foreign browser origin', () => {
		vi.stubGlobal('location', { origin });
		const { options } = setup();
		expect(() => createGatewayPublicHttp({ ...options, origin: 'https://other.example' })).toThrow('this browser origin');
	});
	it('bounds the UTF-8 credential body before fetch without echoing it', async () => {
		const { publicHttp, fetch } = setup();
		const error = await publicHttp.signIn({ login: 'alex', password: 'é'.repeat(256) }).catch(error => error);
		expect(error).toBeInstanceOf(NotDispatchedError);
		expect(String(error)).not.toContain('é');
		expect(fetch).not.toHaveBeenCalled();
	});
	it('bounds the public response body and rejects non-JSON responses', async () => {
		for (const response of [() => Response.json({ result: 'x'.repeat(256) }), () => new Response('private-error', { headers: { 'Content-Type': 'text/html' } })]) {
			const { publicHttp, fetch } = setup(response);
			const error = await publicHttp.version().catch(error => error);
			expect(error).toBeInstanceOf(Error);
			expect(String(error)).not.toContain('private-error');
			expect(fetch.mock.calls[0][1]?.signal?.aborted).toBe(true);
		}
	});
	it.each(['version', 'signIn'] as const)('honors cancellation before dispatch for %s', async operation => {
		const { publicHttp, fetch } = setup();
		const signal = AbortSignal.abort();
		await expect(operation === 'version' ? publicHttp.version(signal) : publicHttp.signIn(credentials, signal)).rejects.toBeInstanceOf(NotDispatchedError);
		expect(fetch).not.toHaveBeenCalled();
	});
	it('enforces the public request deadline after possible dispatch', async () => {
		vi.useFakeTimers();
		const { options } = setup();
		const fetch = vi.fn<typeof globalThis.fetch>(() => new Promise(() => {}));
		const publicHttp = createGatewayPublicHttp({ ...options, fetch });
		const result = publicHttp.version().catch(error => error);
		await vi.advanceTimersByTimeAsync(1000);
		const error = await result;
		expect(error).toBeInstanceOf(Error);
		expect(error).not.toBeInstanceOf(NotDispatchedError);
		expect(fetch.mock.calls[0][1]?.signal?.aborted).toBe(true);
	});
	it('does not treat an absent proof as a URL secret on a host containing undefined', async () => {
		const { options, fetch } = setup(); const publicHttp = createGatewayPublicHttp({ ...options, origin: 'https://undefined.example' });
		await publicHttp.version(); expect(String(fetch.mock.calls[0][0])).toBe('https://undefined.example/api/v1/version');
	});

});
