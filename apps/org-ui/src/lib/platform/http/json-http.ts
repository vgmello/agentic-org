/** Same-origin JSON boundary. B1-02 adapters supply concrete routes and decode the returned body. */
export function createJsonHttp({ origin, fetch: send = globalThis.fetch, timeoutMs, maxResponseBytes }: {
	origin: string; fetch?: typeof fetch; timeoutMs: number; maxResponseBytes: number;
}) {
	const base = new URL(origin);
	if (base.origin !== origin || !['http:', 'https:'].includes(base.protocol)) throw new Error('Invalid browser origin');
	if (typeof globalThis.location !== 'undefined' && globalThis.location.origin !== origin)
		throw new Error('The API must use this browser origin');
	if (!Number.isSafeInteger(timeoutMs) || timeoutMs <= 0 || timeoutMs > 2 ** 31 - 1 || !Number.isSafeInteger(maxResponseBytes) || maxResponseBytes <= 0)
		throw new Error('HTTP requests require finite time and body limits');
	return async ({ path, method, body, proof, signal }: {
		path: string; method: 'GET' | 'POST'; body?: unknown; proof: string; signal?: AbortSignal;
	}): Promise<{ status: number; body: unknown }> => {
		const url = new URL(path, base);
		if (!path.startsWith('/api/v1/') || path.includes('\\') || url.origin !== origin || !url.pathname.startsWith('/api/v1/') || url.hash || url.username || url.password)
			throw new Error('Only same-origin API paths are allowed');
		if (!proof || url.href.includes(proof) || url.href.includes(encodeURIComponent(proof))) throw new Error('Session proof must be supplied only in the header');
		const headers = new Headers({ Accept: 'application/json', 'X-Org-Session-Proof': proof });
		const encoded = body === undefined ? undefined : JSON.stringify(body);
		if (encoded !== undefined) headers.set('Content-Type', 'application/json');
		const controller = new AbortController();
		const cancel = () => controller.abort(new Error('Request cancelled'));
		if (signal?.aborted) throw new Error('Request cancelled');
		signal?.addEventListener('abort', cancel, { once: true });
		const timer = setTimeout(() => controller.abort(new Error('Request timed out')), timeoutMs);
		let aborted: (() => void) | undefined;
		const deadline = new Promise<never>((_, reject) => {
			aborted = () => reject(controller.signal.reason);
			controller.signal.addEventListener('abort', aborted, { once: true });
		});
		try {
			return await Promise.race([deadline, (async () => {
				const response = await send(url, { method, headers, body: encoded, credentials: 'same-origin', redirect: 'error', cache: 'no-store', signal: controller.signal });
				if (!/^application\/(?:[\w.+-]+\+)?json(?:;|$)/i.test(response.headers.get('content-type') ?? '')) throw new Error('Expected JSON');
				const reader = response.body?.getReader();
				if (!reader) throw new Error('Missing JSON body');
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
				} finally { reader.releaseLock(); }
				const bytes = new Uint8Array(size);
				let at = 0;
				for (const chunk of chunks) { bytes.set(chunk, at); at += chunk.length; }
				return { status: response.status, body: JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes)) as unknown };
			})()]);
		} finally {
			clearTimeout(timer);
			signal?.removeEventListener('abort', cancel);
			if (aborted) controller.signal.removeEventListener('abort', aborted);
		}
	};
}
