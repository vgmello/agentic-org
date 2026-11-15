import { randomUUID } from 'node:crypto';
import type { IncomingMessage, ServerResponse } from 'node:http';

export const sessionView = { instance_id: 'hq', account: { id: 'alex-id', login: 'alex' }, admin: false, organizations: [], server_time_ms: 100,
	absolute_expires_at: 'later', idle_expires_at: 'sooner' };
const firstProof = 'a'.repeat(64), secondProof = 'b'.repeat(64);
function newCase() {
	return { proof: firstProof, view: sessionView, refreshCount: 0, gets: 0, signOutFails: false, revision: 2,
		hold: false, release: undefined as (() => void) | undefined, requestHeaders: [] as Record<string, string>[] };
}
async function body(req: IncomingMessage) {
	const chunks: Buffer[] = []; let size = 0;
	for await (const chunk of req) { size += chunk.length; if (size > 4096) throw new Error('Test request too large'); chunks.push(Buffer.from(chunk)); }
	return JSON.parse(Buffer.concat(chunks).toString('utf8'));
}

/** Test-owned HTTP fixture: native browser requests, cookies and proof headers reach Node unchanged. */
export function sessionTestMiddleware() {
	const cases = new Map<string, ReturnType<typeof newCase>>();
	return async (req: IncomingMessage, res: ServerResponse, next: () => void) => {
		const url = new URL(req.url ?? '/', 'http://localhost');
		const id = req.headers.cookie?.match(/(?:^|;\s*)p6_auth_case=([^;]+)/)?.[1];
		const state = id ? cases.get(id) : undefined;
		if (!url.pathname.startsWith('/__p6-auth/') && !(state && url.pathname.startsWith('/api/v1/'))) return next();
		const json = (value: unknown, status = 200, cookie?: string) => {
			res.statusCode = status; res.setHeader('Content-Type', 'application/json');
			if (cookie) res.setHeader('Set-Cookie', `org_session=${cookie}; HttpOnly; SameSite=Strict; Path=/api/v1`);
			res.end(JSON.stringify(value));
		};
		try {
			if (url.pathname === '/__p6-auth/new' && req.method === 'POST') {
				const id = randomUUID(); cases.set(id, newCase()); json({ id }); return;
			}
			if (url.pathname === '/__p6-auth/control') {
				const value = req.method === 'POST' ? await body(req) : { id: url.searchParams.get('id') };
				const controlled = cases.get(value.id); if (!controlled) { json({}, 404); return; }
				if (req.method === 'POST') {
					if (value.action === 'hold') controlled.hold = true;
					else if (value.action === 'release') { controlled.hold = false; controlled.release?.(); }
					else if (value.action === 'signOutFails') controlled.signOutFails = value.value === true;
					else if (value.action === 'revision') controlled.revision = value.value;
					else { json({}, 400); return; }
				}
				json({ refreshCount: controlled.refreshCount, gets: controlled.gets, requestHeaders: controlled.requestHeaders }); return;
			}
			if (!state) { json({}, 404); return; }
			state.requestHeaders.push({ 'x-org-session-proof': String(req.headers['x-org-session-proof'] ?? ''), cookie: req.headers.cookie ?? '' });
			const respond = (result: unknown, cookie?: string) => json({ result }, 200, cookie);
			if (url.pathname === '/api/v1/version' && req.method === 'GET') { respond({ api_version: 1, contract_revision: state.revision }); return; }
			if (url.pathname === '/api/v1/session/sign-in' && req.method === 'POST') {
				const { login } = await body(req);
				state.view = login === 'Blair' ? { ...sessionView, account: { id: 'blair-id', login: 'Blair' } } : sessionView;
				respond({ ...state.view, proof: state.proof }, state.proof); return;
			}
			if (req.headers['x-org-session-proof'] !== state.proof || !req.headers.cookie?.includes(`org_session=${state.proof}`)) {
				json({ error: { code: 'not_authorized', message: 'Session ended.' } }, 401); return;
			}
			if (url.pathname === '/api/v1/session' && req.method === 'GET') { state.gets++; respond(state.view); return; }
			if (url.pathname === '/api/v1/session/refresh' && req.method === 'POST') {
				state.refreshCount++; if (state.hold) await new Promise<void>(resolve => { state.release = resolve; });
				state.proof = secondProof; respond({ ...state.view, proof: state.proof }, state.proof); return;
			}
			if (url.pathname === '/api/v1/session/sign-out' && req.method === 'POST') {
				if (state.signOutFails) json({ error: { code: 'unavailable', message: 'Offline.' } }, 503);
				else respond({}); return;
			}
			json({}, 404);
		} catch { json({}, 400); }
	};
}
