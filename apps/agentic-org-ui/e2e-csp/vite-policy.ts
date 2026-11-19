import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import type { IncomingMessage, ServerResponse } from 'node:http';
import type { Plugin } from 'vite';

type Report = { documentURI: string; directive: string; blockedURI: string; sample: string };
const policies = Object.fromEntries(['html', 'non-html'].map((name) => [name,
	readFileSync(new URL(`../../../config/csp/${name}.txt`, import.meta.url), 'utf8').replace(/\n$/, '')
]));
const withReports = (policy: string) => policy.replace('script-src ', "script-src 'report-sample' ") + '; report-uri /__csp-report';
const htmlResponse = (res: ServerResponse) => /^text\/html(?:;|$)/i.test(String(res.getHeader('Content-Type')));
const buffer = (chunk: string | Uint8Array, encoding?: BufferEncoding | (() => void)) =>
	typeof chunk === 'string' ? Buffer.from(chunk, typeof encoding === 'string' ? encoding : undefined) : Buffer.from(chunk);
async function bodyText(req: IncomingMessage) {
	const chunks = [];
	for await (const chunk of req) chunks.push(Buffer.from(chunk));
	return Buffer.concat(chunks).toString('utf8');
}

/** An isolated collector and policy boundary for the test-owned dev server only. */
export function cspMiddleware() {
	let reports: Report[] = [];
	const errors: string[] = [];
	let missingHeaders = 0;
	return async (req: IncomingMessage, res: ServerResponse, next: () => void) => {
		delete req.headers['if-none-match'];
		delete req.headers['if-modified-since'];
		const writeHead = res.writeHead.bind(res), write = res.write.bind(res), end = res.end.bind(res);
		const chunks: Buffer[] = [];
		const policy = (body = '') => {
			const hashes = [...body.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script\s*>/gi)]
				.filter((match) => !/\bsrc\s*=/i.test(match[1]))
				.map((match) => `'sha256-${createHash('sha256').update(match[2].replace(/\r\n?/g, '\n')).digest('base64')}'`).join(' ');
			res.setHeader('Content-Security-Policy', withReports(htmlResponse(res) ? policies.html.replace('{script_hashes}', hashes) : policies['non-html']));
		};
		res.on('finish', () => {
			if (htmlResponse(res) && !res.getHeader('Content-Security-Policy')) missingHeaders++;
		});
		res.writeHead = ((code: number, messageOrHeaders?: string | Record<string, unknown> | string[], headers?: Record<string, unknown> | string[]) => {
			res.statusCode = code;
			if (typeof messageOrHeaders === 'string') res.statusMessage = messageOrHeaders;
			const values = typeof messageOrHeaders === 'string' ? headers : messageOrHeaders;
			const entries = Array.isArray(values)
				? Array.from({ length: values.length / 2 }, (_, index) => [values[index * 2], values[index * 2 + 1]])
				: Object.entries(values ?? {});
			const seen = new Set<string>();
			for (const [name, value] of entries) {
				if (Array.isArray(values) && seen.has(name.toLowerCase())) res.appendHeader(name, value as string);
				else res.setHeader(name, value as string);
				seen.add(name.toLowerCase());
			}
			if (!htmlResponse(res)) { policy(); writeHead(code); }
			return res;
		}) as typeof res.writeHead;
		res.write = ((chunk: Uint8Array | string, encoding?: BufferEncoding | (() => void), callback?: () => void) => {
			if (!htmlResponse(res)) return write(chunk, encoding as BufferEncoding, callback);
			chunks.push(buffer(chunk, encoding));
			process.nextTick(() => { if (typeof encoding === 'function') encoding(); else callback?.(); });
			return true; // SvelteKit waits for drain on any falsy return.
		}) as typeof res.write;
		res.end = ((chunk?: Uint8Array | string | (() => void), encoding?: BufferEncoding | (() => void), callback?: () => void) => {
			if (htmlResponse(res)) {
				if (chunk !== undefined && typeof chunk !== 'function') chunks.push(buffer(chunk, encoding));
				const body = Buffer.concat(chunks);
				policy(body.toString('utf8'));
				res.writeHead = writeHead;
				return end(body, typeof chunk === 'function' ? chunk : typeof encoding === 'function' ? encoding : callback);
			}
			return end(chunk as string, encoding as BufferEncoding, callback);
		}) as typeof res.end;
		const path = new URL(req.url ?? '/', 'http://localhost').pathname;
		if (!['/__csp-report', '/__csp-status', '/__csp-clear'].includes(path)) return next();
		try {
			if (path === '/__csp-status' && req.method === 'GET') {
				res.setHeader('Content-Type', 'application/json');
				res.end(JSON.stringify({ reports, errors, missingHeaders }));
				return;
			}
			if (req.method !== 'POST') throw new Error('Collector requires POST');
			const value = JSON.parse(await bodyText(req));
			if (path === '/__csp-report') {
				const report = value?.['csp-report'];
				if ((value?.type !== undefined && value.type !== 'csp-violation')
					|| ['document-uri', 'effective-directive', 'blocked-uri'].some((key) => typeof report?.[key] !== 'string')
					|| (report['script-sample'] !== undefined && typeof report['script-sample'] !== 'string')) {
					throw new Error('Unknown CSP report shape');
				}
				reports.push({ documentURI: report['document-uri'], directive: report['effective-directive'], blockedURI: report['blocked-uri'], sample: report['script-sample'] ?? '' });
				res.statusCode = 204; res.end(); return;
			}
			if (path !== '/__csp-clear' || !/^c[0-9a-f]{8}$/.test(value?.marker)
				|| !['chromium', 'webkit', 'firefox'].includes(value?.browser)) throw new Error('Invalid control clear');
			reports = reports.filter((report) => !(new URL(report.documentURI).searchParams.get('csp-control') === value.marker
				&& report.blockedURI === 'inline' && ['script-src', 'script-src-elem'].includes(report.directive)
				&& (value.browser === 'webkit' || report.sample.includes(value.marker))));
			res.setHeader('Content-Type', 'application/json'); res.end('{}');
		} catch (error) {
			errors.push(String(error));
			res.statusCode = 400;
			res.end('Invalid CSP collector request');
		}
	};
}

export function testCspPolicy(): Plugin {
	return {
		name: 'org-test-csp-policy', apply: 'serve', enforce: 'pre',
		configureServer(server) { server.middlewares.use(cspMiddleware()); }
	};
}
