import { NotDispatchedError } from './errors';
import { readSignIn } from './wire';

type JsonRequest = { path: string; signal?: AbortSignal } & (
	| { method: 'GET'; body?: never }
	| { method: 'POST'; body: unknown }
);
export type JsonHttpRequest = JsonRequest & { proof: string };
type JsonHttpOptions = {
	origin: string; fetch?: typeof fetch; timeoutMs: number; maxRequestBytes: number; maxResponseBytes: number;
};

function validApiTarget(path: string): boolean {
	if (typeof path !== 'string' || !path.startsWith('/api/v1/') || /[\x00-\x20\x7f]/.test(path) || path.includes('#')) return false;
	const pathname = path.split(/[?#]/, 1)[0];
	return pathname.slice(1).split('/').every((segment) => {
		if (!segment || segment.includes('\\') || /%(?:2f|5c)/i.test(segment)) return false;
		const dots = segment.replace(/%2e/ig, '.');
		return dots !== '.' && dots !== '..';
	});
}

function confined(path: string, url: URL, origin: string): boolean {
	// D-GW1: callers encode path/query data; URL parsing must preserve the entire request target.
	return validApiTarget(path) && url.href === origin + path && url.origin === origin &&
		url.pathname.startsWith('/api/v1/') && !/%(?:2f|5c)/i.test(url.pathname) &&
		!url.hash && !url.username && !url.password;
}
function assertHeaderProof(proof: unknown): asserts proof is string {
	// Transport accepts header-safe opaque text; session decoding enforces the wire proof format.
	if (typeof proof !== 'string' || !/^[\x21-\x7e]{1,1024}$/.test(proof))
		throw new NotDispatchedError('Invalid session proof');
}
function assertNoProof(url: URL, proof: string) {
	if ([url.href, decodeURIComponent(url.pathname), ...Array.from(url.searchParams).flat()].some((part) => part.includes(proof)))
		throw new NotDispatchedError('Session proof must be supplied only in the header');
}

function createBoundedJsonHttp({ origin, fetch: send = globalThis.fetch, timeoutMs, maxRequestBytes, maxResponseBytes }: JsonHttpOptions, authenticated: boolean) {
	let base: URL;
	try { base = new URL(origin); } catch { throw new NotDispatchedError('Invalid browser origin'); }
	if (base.origin !== origin || !['http:', 'https:'].includes(base.protocol)) throw new NotDispatchedError('Invalid browser origin');
	if (typeof globalThis.location !== 'undefined' && globalThis.location.origin !== origin)
		throw new NotDispatchedError('The API must use this browser origin');
	if (!Number.isSafeInteger(timeoutMs) || timeoutMs <= 0 || timeoutMs > 2 ** 31 - 1 || !Number.isSafeInteger(maxRequestBytes) || maxRequestBytes <= 0 || !Number.isSafeInteger(maxResponseBytes) || maxResponseBytes <= 0)
		throw new NotDispatchedError('HTTP requests require finite time and body limits');
	return async (request: JsonRequest & { proof?: string }): Promise<{ status: number; body: unknown }> => {
		let url: URL;
		let headers: Headers;
		let encoded: string | undefined;
		let method: JsonHttpRequest['method'];
		let signal: AbortSignal | undefined;
		let sessionProof: string | undefined;
		try {
			const { path, body, proof } = request;
			method = request.method;
			signal = request.signal;
			if (authenticated) { assertHeaderProof(proof); sessionProof = proof; }
			url = new URL(path, base);
			if (!confined(path, url, origin)) throw new NotDispatchedError('Only same-origin API paths are allowed');
			if (sessionProof !== undefined) assertNoProof(url, sessionProof);
			if (method !== 'GET' && method !== 'POST') throw new NotDispatchedError('Unsupported HTTP method');
			if (method === 'GET' && body !== undefined) throw new NotDispatchedError('GET requests cannot have a body');
			headers = new Headers({ Accept: 'application/json' });
			if (sessionProof !== undefined) headers.set('X-Org-Session-Proof', sessionProof);
			encoded = body === undefined ? undefined : JSON.stringify(body, (key, value: unknown) => {
				if (sessionProof !== undefined && (key.includes(sessionProof) || (typeof value === 'string' && value.includes(sessionProof))))
					throw new NotDispatchedError('Session proof must be supplied only in the header');
				return value;
			});
			if (method === 'POST' && encoded === undefined) throw new NotDispatchedError('POST requires a JSON body');
			if (encoded !== undefined && new TextEncoder().encode(encoded).byteLength > maxRequestBytes)
				throw new NotDispatchedError('JSON request exceeds its limit');
			if (encoded !== undefined) headers.set('Content-Type', 'application/json');
			if (signal?.aborted) throw new NotDispatchedError('Request cancelled');
		} catch (error) {
			// URL, Headers and JSON errors can echo their inputs. Never carry those messages/proofs out.
			if (error instanceof NotDispatchedError) throw error;
			throw new NotDispatchedError();
		}
		const controller = new AbortController();
		const cancel = () => controller.abort(new Error('Request cancelled'));
		signal?.addEventListener('abort', cancel, { once: true });
		const timer = setTimeout(() => controller.abort(new Error('Request timed out')), timeoutMs);
		let aborted: (() => void) | undefined;
		const deadline = new Promise<never>((_, reject) => {
			aborted = () => reject(controller.signal.reason);
			controller.signal.addEventListener('abort', aborted, { once: true });
		});
		try {
			return await Promise.race([deadline, (async () => {
				// From this point, failures never prove a mutation was not dispatched.
				const response = await send(url, { method, headers, body: encoded, mode: 'same-origin', credentials: 'same-origin', redirect: 'error', cache: 'no-store', signal: controller.signal });
				if (controller.signal.aborted || !/^application\/(?:[\w.+-]+\+)?json(?:;|$)/i.test(response.headers.get('content-type') ?? '')) {
					void response.body?.cancel().catch(() => {});
					if (controller.signal.aborted) throw controller.signal.reason;
					throw new Error('Expected JSON');
				}
				const reader = response.body?.getReader();
				if (!reader) throw new Error('Missing JSON body');
				const cancelBody = () => { void reader.cancel().catch(() => {}); };
				controller.signal.addEventListener('abort', cancelBody, { once: true });
				const chunks: Uint8Array[] = [];
				let size = 0;
				try {
					while (true) {
						const { done, value } = await reader.read();
						if (done) break;
						size += value.length;
						if (size > maxResponseBytes) { await reader.cancel(); throw new Error('JSON response exceeds its limit'); }
						chunks.push(value);
					}
				} finally {
					controller.signal.removeEventListener('abort', cancelBody);
					reader.releaseLock();
				}
				const bytes = new Uint8Array(size);
				let at = 0;
				for (const chunk of chunks) { bytes.set(chunk, at); at += chunk.length; }
				try {
					return { status: response.status, body: JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes)) as unknown };
				} catch { throw new Error('Invalid JSON response'); }
			})()]);
		} catch (error) {
			// The preflight marker is valid only before send, even for a faulty injected fetch/stream.
			if (error instanceof NotDispatchedError) throw new Error('Request failed after possible dispatch');
			throw error;
		} finally {
			clearTimeout(timer);
			signal?.removeEventListener('abort', cancel);
			if (aborted) controller.signal.removeEventListener('abort', aborted);
			controller.abort();
		}
	};
}

/** Protected calls always require a proof, including calls to a public route through this API. */
export function createJsonHttp(options: JsonHttpOptions): (request: JsonHttpRequest) => Promise<{ status: number; body: unknown }> {
	return createBoundedJsonHttp(options, true);
}

/** The only public gateway operations; callers cannot choose an unauthenticated request target. */
export function createGatewayPublicHttp(options: JsonHttpOptions) {
	const send = createBoundedJsonHttp(options, false);
	return Object.freeze({
		version: (signal?: AbortSignal) => send({ path: '/api/v1/version', method: 'GET', signal }),
		async signIn(credentials: unknown, signal?: AbortSignal) {
			let body;
			try { body = readSignIn(credentials); }
			catch { throw new NotDispatchedError('Invalid sign-in request'); }
			return send({ path: '/api/v1/session/sign-in', method: 'POST', body, signal });
		}
	});
}
