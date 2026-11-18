import { describe, expect, it, vi } from 'vitest';
import { createGatewayPublicHttp, createJsonHttp } from '../http/json-http';
import { WireDecodeError } from '../http/wire';
import { createSessionGateway, SessionRequestError } from './session-gateway';

const proof = 'a'.repeat(64);
const view = {
	instance_id: 'hq',
	account: { id: 'alex-id', login: 'alex' },
	admin: false,
	organizations: [],
	server_time_ms: 100,
	idle_expires_at: 'sooner'
};
function setup(body: unknown, status = 200) {
	const fetch = vi.fn<typeof globalThis.fetch>(async () => Response.json(body, { status }));
	const options = {
		origin: 'http://localhost:4173',
		fetch,
		timeoutMs: 1000,
		maxRequestBytes: 4096,
		maxResponseBytes: 4096
	};
	return {
		fetch,
		gateway: createSessionGateway(createGatewayPublicHttp(options), createJsonHttp(options))
	};
}
describe('P6a fixed session operations', () => {
	it.each([
		['version', '/api/v1/version', 'GET', { api_version: 1, contract_revision: 2 }, undefined],
		[
			'signIn',
			'/api/v1/session/sign-in',
			'POST',
			{ ...view, proof },
			{ login: 'alex', password: 'private-password' }
		],
		['get', '/api/v1/session', 'GET', view, undefined],
		['refresh', '/api/v1/session/refresh', 'POST', { ...view, proof }, {}],
		['signOut', '/api/v1/session/sign-out', 'POST', {}, {}]
	] as const)(
		'decodes %s through the canonical endpoint and result envelope',
		async (operation, path, method, expected, body) => {
			const { gateway, fetch } = setup({ result: expected });
			const result =
				operation === 'version'
					? await gateway.version()
					: operation === 'signIn'
						? await gateway.signIn(body)
						: await gateway[operation](proof);
			expect(result).toEqual(expected);
			const [url, init] = fetch.mock.calls[0];
			expect(String(url)).toBe('http://localhost:4173' + path);
			expect(init?.method).toBe(method);
			expect(init?.body === undefined ? undefined : JSON.parse(init.body as string)).toEqual(body);
			expect(new Headers(init?.headers).get('X-Org-Session-Proof')).toBe(
				['version', 'signIn'].includes(operation) ? null : proof
			);
		}
	);
	it('B6 register posts the link secret only in the JSON body, without a proof header', async () => {
		const { gateway, fetch } = setup({ result: { ...view, proof } });
		const request = { link: 'k7Q2-secret', login: 'alex', password: 'long-enough-pw' };
		expect(await gateway.register(request)).toEqual({ ...view, proof });
		const [url, init] = fetch.mock.calls[0];
		expect(String(url)).toBe('http://localhost:4173/api/v1/links/register');
		expect(String(url)).not.toContain('k7Q2');
		expect(init?.method).toBe('POST');
		expect(new Headers(init?.headers).get('Content-Type')).toBe('application/json');
		expect(new Headers(init?.headers).get('X-Org-Session-Proof')).toBeNull();
		expect(JSON.parse(init?.body as string)).toEqual(request);
	});
	it('B6 register refuses a request without a link before any dispatch', async () => {
		const { gateway, fetch } = setup({ result: { ...view, proof } });
		await expect(gateway.register({ link: '', login: 'alex', password: 'long-enough-pw' })).rejects.toThrow();
		expect(fetch).not.toHaveBeenCalled();
	});
	it('B6 errors carry the credential field and rule but never the server message', async () => {
		const { gateway } = setup(
			{ error: { code: 'invalid_request', message: 'server words', field: 'login', rule: 'taken' } },
			400
		);
		const error = await gateway
			.register({ link: 'x', login: 'alex', password: 'long-enough-pw' })
			.catch((e: unknown) => e);
		expect(error).toBeInstanceOf(SessionRequestError);
		expect(error).toMatchObject({ status: 400, code: 'invalid_request', field: 'login', rule: 'taken' });
		expect(String((error as Error).message)).not.toContain('server words');
	});
	it('does not publish an unsolicited proof from GET session', async () => {
		const { gateway } = setup({ result: { ...view, proof } });
		expect(await gateway.get(proof)).toEqual(view);
	});
	it.each(['signIn', 'refresh'] as const)(
		'%s requires a canonical response proof',
		async (operation) => {
			for (const returned of [undefined, 'invalid', 'A'.repeat(64)]) {
				const { gateway } = setup({ result: { ...view, proof: returned } });
				await expect(
					operation === 'signIn'
						? gateway.signIn({ login: 'alex', password: 'password' })
						: gateway.refresh(proof)
				).rejects.toBeInstanceOf(WireDecodeError);
			}
		}
	);
	it.each([400, 401, 403, 429, 503])(
		'preserves safe failure metadata at HTTP %i without server messages/proofs',
		async (status) => {
			const { gateway } = setup(
				{
					error: {
						code: 'not_authorized',
						message: proof,
						session_reason: 'future_reason',
						retry_after_ms: 500
					}
				},
				status
			);
			const error = await gateway.get(proof).catch((error) => error);
			expect(error).toBeInstanceOf(SessionRequestError);
			expect(error).toMatchObject({ status, sessionReason: 'unknown', retryAfterMs: 500 });
			expect(JSON.stringify(error)).not.toContain(proof);
			expect(String(error)).not.toContain(proof);
		}
	);
	it.each([200, 201, 299])('refuses an error envelope on a success status %i', async (status) => {
		const { gateway } = setup({ error: { code: 'not_authorized', message: 'No' } }, status);
		await expect(gateway.get(proof)).rejects.toBeInstanceOf(WireDecodeError);
	});
	it.each([201, 299, 400, 401, 503])(
		'refuses a result envelope at unexpected status %i',
		async (status) => {
			const { gateway } = setup({ result: view }, status);
			await expect(gateway.get(proof)).rejects.toBeInstanceOf(WireDecodeError);
		}
	);
	it.each([
		{},
		{ result: view, error: { code: 'not_authorized', message: 'No' } },
		{ result: null },
		{ result: {} },
		{ error: { code: 1, message: 'No' } }
	])('refuses malformed envelopes before publishing state: %j', async (body) => {
		const { gateway } = setup(body);
		await expect(gateway.get(proof)).rejects.toBeInstanceOf(WireDecodeError);
	});
	it.each([{}, { x: 1 }])('accepts the published empty-result vector %j', async (result) => {
		const { gateway } = setup({ result });
		expect(await gateway.signOut(proof)).toEqual({});
	});
	it('discards additive sign-out response fields, including unsolicited credentials', async () => {
		const { gateway } = setup({ result: { ...view, proof, future: 'ignored' } });
		expect(await gateway.signOut(proof)).toEqual({});
	});
	it.each([null, [], 1, '', 'success'])(
		'still requires an object for sign-out result: %j',
		async (result) => {
			const { gateway } = setup({ result });
			await expect(gateway.signOut(proof)).rejects.toBeInstanceOf(WireDecodeError);
		}
	);
	it.each(['get', 'refresh', 'signOut'] as const)(
		'forwards cancellation to %s and never retries it',
		async (operation) => {
			const { gateway, fetch } = setup({ result: {} });
			await expect(gateway[operation](proof, AbortSignal.abort())).rejects.toThrow('cancelled');
			expect(fetch).not.toHaveBeenCalled();
		}
	);
	it('does not retry a failed refresh or sign-out after possible dispatch', async () => {
		for (const operation of ['refresh', 'signOut'] as const) {
			const { gateway, fetch } = setup({ error: { code: 'not_authorized', message: 'No' } }, 401);
			await expect(gateway[operation](proof)).rejects.toMatchObject({ status: 401 });
			expect(fetch).toHaveBeenCalledTimes(1);
		}
	});
	it('decodes the gateway protected-auth denial exactly as apps/agentic-org-web sends it', async () => {
		for (const operation of ['get', 'refresh', 'signOut'] as const) {
			const { gateway } = setup(
				{
					error: {
						code: 'not_authorized',
						message: 'Session ended. Sign in again.',
						session_reason: 'session_ended'
					}
				},
				401
			);
			const error = await gateway[operation](proof).catch((error) => error);
			expect(error).toBeInstanceOf(SessionRequestError);
			expect(error).toMatchObject({
				status: 401,
				sessionReason: 'session_ended',
				retryAfterMs: undefined
			});
		}
	});
	it('relies on the browser for the HttpOnly cookie: the proof header is the only credential it sends', async () => {
		const { gateway, fetch } = setup({ result: view });
		await gateway.get(proof);
		const [, init] = fetch.mock.calls[0];
		expect(init).toMatchObject({ credentials: 'same-origin', mode: 'same-origin' });
		const headers = new Headers(init?.headers);
		expect(headers.get('Cookie')).toBeNull();
		expect([...headers.keys()].sort()).toEqual(['accept', 'x-org-session-proof']);
	});
	it('never reads, writes or derives the session cookie in the session or HTTP adapters', () => {
		const sources = import.meta.glob(
			['./*.ts', '../http/*.ts', '!./*.test.ts', '!../http/*.test.ts'],
			{ query: '?raw', import: 'default', eager: true }
		) as Record<string, string>;
		expect(Object.keys(sources)).toEqual(
			expect.arrayContaining([
				'./session-gateway.ts',
				'./session-controller.ts',
				'./session-store.ts',
				'../http/json-http.ts'
			])
		);
		for (const [path, source] of Object.entries(sources))
			expect({
				path,
				cookie: /document\.cookie|cookieStore|org_session|Set-Cookie|['"]Cookie['"]/i.test(source)
			}).toEqual({ path, cookie: false });
	});
	it('validates the public version rather than accepting arbitrary result fields', async () => {
		const { gateway } = setup({ result: { api_version: '1', contract_revision: 2 } });
		await expect(gateway.version()).rejects.toBeInstanceOf(WireDecodeError);
	});
});
