import { afterEach, describe, expect, it, vi } from 'vitest';
import { createJsonHttp } from './json-http';

const proof = 'opaque-proof-value';
const origin = 'http://localhost:4173';
const request = { path: '/api/v1/test-only', method: 'POST' as const, proof, body: { expected_version: 3 } };
function setup(response = () => Response.json({ ok: true })) {
	const send = vi.fn<typeof fetch>(async () => response());
	return { send, http: createJsonHttp({ origin, fetch: send, timeoutMs: 1000, maxResponseBytes: 100, maxRequestBytes: 100 }) };
}
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

describe('same-origin JSON boundary (test paths, no production routes)', () => {
	it('refuses missing or overflowing timeout/body budgets', () => {
		for (const timeoutMs of [0, NaN, 2 ** 31])
			expect(() => createJsonHttp({ origin, timeoutMs, maxResponseBytes: 100, maxRequestBytes: 100 })).toThrow('limits');
		expect(() => createJsonHttp({ origin, timeoutMs: 1000, maxResponseBytes: Infinity, maxRequestBytes: 100 })).toThrow('limits');
		for (const maxRequestBytes of [0, NaN, Infinity])
			expect(() => createJsonHttp({ origin, timeoutMs: 1000, maxResponseBytes: 100, maxRequestBytes })).toThrow('limits');
	});
	it('keeps proof in a header, sends the cookie and refuses redirects/cache', async () => {
		const { send, http } = setup();
		expect(await http(request)).toEqual({ status: 200, body: { ok: true } });
		const [url, init] = send.mock.calls[0];
		expect(String(url)).toBe(`${origin}/api/v1/test-only`);
		expect(String(url)).not.toContain(proof);
		expect(init).toMatchObject({ method: 'POST', credentials: 'same-origin', redirect: 'error', cache: 'no-store', body: '{"expected_version":3}' });
		expect(new Headers(init?.headers).get('X-Org-Session-Proof')).toBe(proof);
	});

	it('refuses a misconfigured foreign origin even if a caller supplies it as the base', () => {
		vi.stubGlobal('location', { origin });
		expect(() => createJsonHttp({ origin: 'https://evil.example', timeoutMs: 1000, maxResponseBytes: 100, maxRequestBytes: 100 })).toThrow('this browser origin');
	});

	it.each(['https://evil.example/api/v1/test', '//evil.example/api/v1/test', '/api/v1/../../private', '/api/v1/%2e%2e/%2e%2e/private', '/api/v1/test#secret', '/api/v1/\\evil.example', `/api/v1/test?proof=${proof}`])('refuses unsafe path %s before calling fetch', async (path) => {
		const { send, http } = setup();
		await expect(http({ ...request, path })).rejects.toThrow();
		expect(send).not.toHaveBeenCalled();
	});

	it('rejects partially encoded proofs in path and query before dispatch', async () => {
		const { send, http } = setup();
		for (const path of ['/api/v1/%6fpaque-proof-value', '/api/v1/test?proof=%6fpaque-proof-value'])
			await expect(http({ ...request, path })).rejects.toThrow('proof');
		expect(send).not.toHaveBeenCalled();
	});

	it('rejects session proofs in JSON values and keys before dispatch', async () => {
		const { send, http } = setup();
		for (const body of [{ session: { proof } }, { [proof]: true }, { text: `prefix ${proof} suffix` }])
			await expect(http({ ...request, body })).rejects.toThrow('proof');
		// Detection must run on JSON values, even when escaping hides their raw representation.
		await expect(http({ ...request, proof: 'quoted"proof', body: { value: 'quoted"proof' } })).rejects.toThrow('proof');
		expect(send).not.toHaveBeenCalled();
	});

	it('rejects oversized UTF-8 request bodies before dispatch', async () => {
		const { send, http } = setup();
		await expect(http({ ...request, body: { text: 'é'.repeat(50) } })).rejects.toThrow('request exceeds');
		expect(send).not.toHaveBeenCalled();
	});

	it('cancels rejected streaming bodies and aborts the transport', async () => {
		const cancel = vi.fn();
		const { http, send } = setup(() => new Response(new ReadableStream({ cancel }), {
			headers: { 'Content-Type': 'text/html' }
		}));
		await expect(http(request)).rejects.toThrow('Expected JSON');
		expect(cancel).toHaveBeenCalledOnce();
		expect(send.mock.calls[0][1]?.signal?.aborted).toBe(true);
	});

	it('does not infer success/rejection from HTTP status', async () => {
		const { http } = setup(() => Response.json({ error: { code: 'conflict' } }, { status: 409 }));
		expect(await http(request)).toEqual({ status: 409, body: { error: { code: 'conflict' } } });
	});

	it('refuses missing proof and an already cancelled request before dispatch', async () => {
		const { send, http } = setup();
		await expect(http({ ...request, proof: '' })).rejects.toThrow('proof');
		await expect(http({ ...request, signal: AbortSignal.abort() })).rejects.toThrow('cancelled');
		expect(send).not.toHaveBeenCalled();
	});

	it('times out even when an injected transport ignores cancellation', async () => {
		vi.useFakeTimers();
		const send = vi.fn<typeof fetch>(() => new Promise(() => {}));
		const http = createJsonHttp({ origin, fetch: send, timeoutMs: 25, maxResponseBytes: 100, maxRequestBytes: 100 });
		const pending = expect(http(request)).rejects.toThrow('timed out');
		await vi.advanceTimersByTimeAsync(25);
		await pending;
		expect(send.mock.calls[0][1]?.signal?.aborted).toBe(true);
		expect(vi.getTimerCount()).toBe(0);
	});

	it('cancels an active request without exposing a durable-operation success or rejection', async () => {
		const send = vi.fn<typeof fetch>(() => new Promise(() => {}));
		const http = createJsonHttp({ origin, fetch: send, timeoutMs: 1000, maxResponseBytes: 100, maxRequestBytes: 100 });
		const controller = new AbortController();
		const pending = expect(http({ ...request, signal: controller.signal })).rejects.toThrow('cancelled');
		controller.abort();
		await pending;
		expect(send.mock.calls[0][1]?.signal?.aborted).toBe(true);
	});

	it('bounds the body while reading, cancels excess bytes and never trusts Content-Length', async () => {
		const cancel = vi.fn();
		const { http } = setup(() => new Response(new ReadableStream({
			start(controller) { controller.enqueue(new Uint8Array(101)); }, cancel
		}), { headers: { 'Content-Type': 'application/json', 'Content-Length': '1' } }));
		await expect(http(request)).rejects.toThrow('limit');
		expect(cancel).toHaveBeenCalled();
	});

	it.each([
		() => new Response('<html>SPA fallback</html>', { headers: { 'Content-Type': 'text/html' } }),
		() => new Response('{bad json', { headers: { 'Content-Type': 'application/json' } })
	])('does not treat invalid API bodies as a successful read', async (response) => {
		await expect(setup(response).http(request)).rejects.toThrow();
	});
});
