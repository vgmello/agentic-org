import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createServer, type Server } from 'node:http';
import { createHash } from 'node:crypto';
import { cspMiddleware } from '../../../e2e-csp/vite-policy';

const html = "default-src 'none'; script-src 'self' {script_hashes}; style-src 'self' 'unsafe-inline'; img-src 'self'; font-src 'self'; connect-src 'self'; manifest-src 'self'; worker-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; object-src 'none'";
const other = "default-src 'none'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'";
const hash = (text: string) => `'sha256-${createHash('sha256').update(text).digest('base64')}'`;
let server: Server;
let origin: string;
const chunks = ['<script>\r\n\tπ\rlone\r', '\n </script><script src="/x.js"></script><script>\n second \n</script>'];
const body = chunks.join('');
const callbacks: string[] = [];
beforeAll(async () => {
	const middleware = cspMiddleware();
	server = createServer((req, res) => {
		const originalHead = res.writeHead.bind(res), originalEnd = res.end.bind(res);
		return middleware(req, res, () => {
		if (req.url === '/missing') { res.writeHead = originalHead; res.end = originalEnd; res.setHeader('Content-Type', 'text/html'); res.end('bypassed wrapper'); return; }
		if (req.url === '/message') { res.writeHead(201, 'Fixture', { 'Content-Type': 'text/html' }); res.end('<p>custom status</p>'); return; }
		if (req.url === '/record') { res.writeHead(200, { 'Content-Type': 'text/html', 'Set-Cookie': 'first=one', 'set-cookie': 'second=two' }); res.end('<p>record headers</p>'); return; }
		if (req.url === '/array') { res.setHeader('Set-Cookie', 'old=replaced'); res.writeHead(200, ['Content-Type', 'text/html', 'Set-Cookie', 'first=one', 'set-cookie', 'second=two']); res.end('<p>raw headers</p>'); return; }
		if (req.url === '/encoded') {
			res.setHeader('Content-Type', 'text/html');
			res.write(Buffer.from(chunks[0]).toString('hex'), 'hex', () => { callbacks.push(req.url!); });
			res.write(Buffer.from(chunks[1]), () => { callbacks.push('buffer'); }); res.end(() => { callbacks.push('end'); }); return;
		}
		if (req.url === '/encoded-end') { res.setHeader('Content-Type', 'text/html'); res.end(Buffer.from(body).toString('hex'), 'hex', () => { callbacks.push(req.url!); }); return; }
		if (req.url === '/end-callback') { res.setHeader('Content-Type', 'text/html'); res.end(body, () => { callbacks.push('end-arg2'); }); return; }
		if (req.url === '/empty') { res.setHeader('Content-Type', 'text/html'); res.end(); return; }
		if (req.url === '/plain') { res.writeHead(200, { 'Content-Type': 'application/javascript' }); res.write('export'); res.end(' default 1'); return; }
		if (req.url === '/conditional') { res.setHeader('Content-Type', 'text/html'); res.end(String(req.headers['if-none-match']) + String(req.headers['if-modified-since'])); return; }
		res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
		expect(res.write(chunks[0])).toBe(true);
		res.end(Buffer.from(chunks[1]));
	}); });
	await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
	const address = server.address();
	if (!address || typeof address === 'string') throw new Error('No fixture port');
	origin = `http://127.0.0.1:${address.port}`;
});
afterAll(() => new Promise<void>((resolve, reject) => server.close((error) => error ? reject(error) : resolve())));
const status = () => fetch(`${origin}/__csp-status`).then((r) => r.json());
const post = (path: string, value: unknown) => fetch(`${origin}${path}`, { method: 'POST', body: JSON.stringify(value) });

describe('B1-05 serve-only CSP middleware', () => {
	it('buffers HTML and independently hashes every inline script after newline normalization without trimming', async () => {
		const response = await fetch(origin);
		expect(await response.text()).toBe(body);
		expect(response.headers.get('Content-Security-Policy')).toBe(html.replace('{script_hashes}', `${hash('\n\tπ\nlone\n ')} ${hash('\n second \n')}`).replace("script-src ", "script-src 'report-sample' ") + '; report-uri /__csp-report');
	});
	it('assigns the literal non-HTML policy to streamed JS including report additions', async () => {
		const response = await fetch(`${origin}/plain`);
		expect(await response.text()).toBe('export default 1');
		expect(response.headers.get('Content-Security-Policy')).toBe(other.replace("script-src ", "script-src 'report-sample' ") + '; report-uri /__csp-report');
	});
	it('preserves status messages, raw headers, encoded chunks, callbacks and empty HTML', async () => {
		const message = await fetch(`${origin}/message`);
		expect(message.status).toBe(201); expect(message.statusText).toBe('Fixture');
		expect(message.headers.get('Content-Security-Policy')).toContain("style-src 'self' 'unsafe-inline'");
		const raw = await fetch(`${origin}/array`);
		expect(raw.headers.get('Content-Type')).toBe('text/html');
		expect(raw.headers.getSetCookie()).toEqual(['first=one', 'second=two']);
		const record = await fetch(`${origin}/record`);
		expect(record.headers.getSetCookie()).toEqual(['second=two']);
		expect(raw.headers.get('Content-Security-Policy')).toContain("style-src 'self' 'unsafe-inline'");
		for (const path of ['/encoded', '/encoded-end', '/end-callback']) {
			const response = await fetch(`${origin}${path}`);
			expect(await response.text()).toBe(body);
			expect(response.headers.get('Content-Security-Policy')).toContain(hash('\n\tπ\nlone\n '));
		}
		expect(callbacks).toEqual(['/encoded', 'buffer', 'end', '/encoded-end', 'end-arg2']);
		const empty = await fetch(`${origin}/empty`);
		expect(await empty.text()).toBe('');
		expect(empty.headers.get('Content-Security-Policy')).toContain("style-src 'self' 'unsafe-inline'");
	});
	it('strips both conditional headers before the handler sees them', async () => {
		const response = await fetch(`${origin}/conditional`, { headers: { 'If-None-Match': '*', 'If-Modified-Since': 'yesterday' } });
		expect(response.status).toBe(200);
		expect(await response.text()).toBe('undefinedundefined');
	});
	it('collects all three browser envelopes and clears only the marked blocked-inline control', async () => {
		const report = { 'document-uri': `${origin}/welcome?csp-control=c12345678`, 'effective-directive': 'script-src-elem', 'blocked-uri': 'inline', 'script-sample': '/*c12345678*/window.bad=true' };
		for (const browser of ['chromium', 'firefox', 'webkit']) {
			const value = browser === 'webkit' ? { type: 'csp-violation', 'csp-report': { ...report, 'script-sample': undefined } } : { 'csp-report': report };
			expect((await post('/__csp-report', value)).status).toBe(204);
			expect((await status()).reports).toHaveLength(1);
			expect((await post('/__csp-clear', { marker: 'c12345678', browser })).status).toBe(200);
			expect((await status()).reports).toEqual([]);
		}
		for (const variant of [
			{ ...report, 'document-uri': `${origin}/agents` },
			{ ...report, 'effective-directive': 'style-src-elem' },
			{ ...report, 'blocked-uri': 'https://bad.test' },
			{ ...report, 'script-sample': 'different' }
		]) await post('/__csp-report', { 'csp-report': variant });
		await post('/__csp-clear', { marker: 'c12345678', browser: 'chromium' });
		expect((await status()).reports).toHaveLength(4);
	});
	it('records invalid report shapes and clear requests as errors instead of silently dropping them', async () => {
		for (const invalid of [null, {}, { type: 'different', 'csp-report': { 'document-uri': 'http://localhost/', 'effective-directive': 'script-src', 'blocked-uri': 'inline' } }, { 'csp-report': 42 }, ...['document-uri', 'effective-directive', 'blocked-uri', 'script-sample'].map((key) => ({ 'csp-report': { 'document-uri': 'http://localhost/', 'effective-directive': 'script-src', 'blocked-uri': 'inline', [key]: 42 } }))]) expect((await post('/__csp-report', invalid)).status).toBe(400);
		for (const invalid of [{ marker: '', browser: 'chromium' }, { marker: 'c12345678', browser: 'unknown' }]) expect((await post('/__csp-clear', invalid)).status).toBe(400);
		const response = await fetch(`${origin}/__csp-report`, { method: 'POST', body: '{bad' });
		expect(response.status).toBe(400);
		expect((await fetch(`${origin}/__csp-clear`)).status).toBe(400);
		expect((await post('/__csp-status', { marker: 'c12345678', browser: 'chromium' })).status).toBe(400);
		expect((await fetch(`${origin}/__csp-clear`, { method: 'DELETE', body: JSON.stringify({ marker: 'c12345678', browser: 'chromium' }) })).status).toBe(400);
		const snapshot = await status();
		expect(snapshot.errors).toHaveLength(14);
		expect(snapshot.missingHeaders).toBe(0);
	});
	it('counts HTML written by a downstream handler that bypasses the wrapper', async () => {
		const response = await fetch(`${origin}/missing`);
		expect(response.headers.get('Content-Security-Policy')).toBeNull();
		expect((await status()).missingHeaders).toBe(1);
	});
});
