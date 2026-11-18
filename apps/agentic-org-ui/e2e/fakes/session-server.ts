import { createHash, randomBytes, randomUUID } from 'node:crypto';
import type { IncomingMessage, ServerResponse } from 'node:http';

export const sessionView = {
	instance_id: 'hq',
	account: { id: 'alex-id', login: 'alex' },
	admin: false,
	organizations: [],
	server_time_ms: 100,
	idle_expires_at: 'sooner'
};
/** Instance-specific cookie name, spelled as the gateway does (apps/agentic-org-web/src/sessions.rs `BrowserSessions::new`). */
export const sessionCookieName = `org_session_${createHash('sha256').update(sessionView.instance_id).digest('hex').slice(0, 16)}`;
/** One browser pair: two independent 256-bit random draws, as web-api-v1 §4 requires. */
export interface SessionPair {
	readonly cookie: string;
	readonly proof: string;
}
const draw = () => randomBytes(32).toString('hex');
const lowerHex64 = /^[0-9a-f]{64}$/;
const notFound = { error: { code: 'not_found', message: 'Not found.' } };
const invalidRequest = { error: { code: 'invalid_request', message: 'Invalid session request.' } };
const knownSessionPaths = new Set([
	'/api/v1/session',
	'/api/v1/session/sign-in',
	'/api/v1/session/refresh',
	'/api/v1/session/sign-out'
]);
interface CreateParams {
	name: string;
	slug: string;
	organization_repository: { kind: string; remote_url: string };
	state_branch: string;
}
type Refusal = { code: string; field?: string; rule?: string };
/** `lost` records the create and answers with an intermediary's error page instead of the outcome. */
type CreateStep =
	| 'accepted'
	| 'completed'
	| 'unknown'
	| 'lost'
	| 'unavailable'
	| { rejected: Refusal; status: number }
	| { refused: Refusal };
type LookupStep =
	| 'accepted'
	| 'completed'
	| 'missing'
	| 'unavailable'
	| 'expired'
	| { rejected: Refusal };
const ulid = /^[0-7][0-9A-HJKMNP-TV-Z]{25}$/;
const hostLookup = /^\/api\/v1\/operations\/host\/([^/]+)$/;
function newCase() {
	return {
		pair: undefined as SessionPair | undefined,
		issued: [] as SessionPair[],
		view: sessionView,
		refreshCount: 0,
		gets: 0,
		signOutFails: false,
		/** The host end of an invalidated sign-out outlives the gateway's backend timeout. */
		signOutTimesOut: false,
		/** A 400/403 refused before authentication: the pair and the cookie are kept. */
		signOutRefused: undefined as 400 | 403 | undefined,
		revision: 2,
		/** What GET /api/v1/organizations lists: a real instance lists none until A4 (SIGN-9). */
		organizations: [] as ({ id: string; name: string; created_at: string; recovery: string } & Record<string, unknown>)[],
		/**
		 * Scripted answers of POST /api/v1/organizations and of the host operation lookup, one per
		 * request; when a script is empty the create is `accepted` and the lookup `completed`.
		 */
		createScript: [] as CreateStep[],
		lookupScript: [] as LookupStep[],
		creates: [] as { operation_id: string; params: CreateParams }[],
		lookups: [] as string[],
		hold: false,
		release: undefined as (() => void) | undefined,
		requestHeaders: [] as Record<string, string>[]
	};
}
async function body(req: IncomingMessage) {
	const chunks: Buffer[] = [];
	let size = 0;
	for await (const chunk of req) {
		size += chunk.length;
		if (size > 4096) throw new Error('Test request too large');
		chunks.push(Buffer.from(chunk));
	}
	return JSON.parse(Buffer.concat(chunks).toString('utf8'));
}
/** Whether a request body is exactly an empty JSON object, as the gateway requires of sign-out. */
async function emptyObjectBody(req: IncomingMessage) {
	try {
		const value = await body(req);
		return (
			typeof value === 'object' &&
			value !== null &&
			!Array.isArray(value) &&
			Object.keys(value).length === 0
		);
	} catch {
		return false;
	}
}
/**
 * Mirrors the gateway's `authenticate`: exactly one canonical proof header and exactly one canonical
 * session cookie. Like the gateway, each Cookie item is trimmed, and items that cannot be ours (no `=`
 * or another name) are skipped rather than refused.
 */
function presentedPair(req: IncomingMessage): SessionPair | undefined {
	const proof = req.headers['x-org-session-proof'];
	if (typeof proof !== 'string' || !lowerHex64.test(proof)) return undefined;
	let cookie: string | undefined;
	for (const raw of (req.headers.cookie ?? '').split(';')) {
		const item = raw.trim();
		const separator = item.indexOf('=');
		if (separator < 0 || item.slice(0, separator) !== sessionCookieName) continue;
		if (cookie !== undefined) return undefined;
		cookie = item.slice(separator + 1);
	}
	return cookie !== undefined && lowerHex64.test(cookie) ? { cookie, proof } : undefined;
}

/** Test-owned HTTP fixture: native browser requests, cookies and proof headers reach Node unchanged. */
export function sessionTestMiddleware() {
	const cases = new Map<string, ReturnType<typeof newCase>>();
	return async (req: IncomingMessage, res: ServerResponse, next: () => void) => {
		const url = new URL(req.url ?? '/', 'http://localhost');
		const id = req.headers.cookie?.match(/(?:^|;\s*)p6_auth_case=([^;]+)/)?.[1];
		const state = id ? cases.get(id) : undefined;
		if (!url.pathname.startsWith('/__p6-auth/') && !(state && url.pathname.startsWith('/api/v1/')))
			return next();
		// The cookie half travels only in Set-Cookie; clearing keeps the same scope with Max-Age=0.
		const json = (value: unknown, status = 200, cookie?: { value: string; clear?: boolean }) => {
			res.statusCode = status;
			res.setHeader('Content-Type', 'application/json');
			if (cookie)
				res.setHeader(
					'Set-Cookie',
					`${sessionCookieName}=${cookie.value};${cookie.clear ? ' Max-Age=0;' : ''} HttpOnly; SameSite=Strict; Path=/api/v1`
				);
			res.end(JSON.stringify(value));
		};
		try {
			if (url.pathname === '/__p6-auth/new' && req.method === 'POST') {
				const id = randomUUID();
				cases.set(id, newCase());
				json({ id });
				return;
			}
			if (url.pathname === '/__p6-auth/control') {
				const value = req.method === 'POST' ? await body(req) : { id: url.searchParams.get('id') };
				const controlled = cases.get(value.id);
				if (!controlled) {
					json({}, 404);
					return;
				}
				if (req.method === 'POST') {
					if (value.action === 'hold') controlled.hold = true;
					else if (value.action === 'release') {
						controlled.hold = false;
						controlled.release?.();
					} else if (value.action === 'signOutFails')
						controlled.signOutFails = value.value === true;
					else if (value.action === 'signOutTimesOut')
						controlled.signOutTimesOut = value.value === true;
					else if (value.action === 'signOutRefused')
						controlled.signOutRefused =
							value.value === 400 || value.value === 403 ? value.value : undefined;
					else if (value.action === 'revision') controlled.revision = value.value;
					else if (value.action === 'organizations' && Array.isArray(value.value))
						controlled.organizations = value.value;
					else if (value.action === 'createScript' && Array.isArray(value.value))
						controlled.createScript = value.value;
					else if (value.action === 'lookupScript' && Array.isArray(value.value))
						controlled.lookupScript = value.value;
					else {
						json({}, 400);
						return;
					}
				}
				json({
					refreshCount: controlled.refreshCount,
					gets: controlled.gets,
					requestHeaders: controlled.requestHeaders,
					issued: controlled.issued,
					creates: controlled.creates,
					lookups: controlled.lookups
				});
				return;
			}
			if (!state) {
				json({}, 404);
				return;
			}
			state.requestHeaders.push({
				'x-org-session-proof': String(req.headers['x-org-session-proof'] ?? ''),
				cookie: req.headers.cookie ?? ''
			});
			const respond = (result: unknown, cookie?: { value: string; clear?: boolean }) =>
				json({ result }, 200, cookie);
			// Sign-in and refresh both draw a fresh independent pair; the previous pair stops authenticating.
			const establish = () => {
				const pair = { cookie: draw(), proof: draw() };
				state.pair = pair;
				state.issued.push(pair);
				respond({ ...state.view, proof: pair.proof }, { value: pair.cookie });
			};
			// Like the gateway: a query on a session path is refused, then every unknown /api/v1
			// path is decided by its shape alone (JSON 404) before any pair is inspected.
			const sessionPath =
				url.pathname === '/api/v1/session' || url.pathname.startsWith('/api/v1/session/');
			if (sessionPath && url.search !== '') {
				json(invalidRequest, 400);
				return;
			}
			if (
				url.pathname !== '/api/v1/version' &&
				url.pathname !== '/api/v1/organizations' &&
				url.pathname !== '/api/v1/instance' &&
				!hostLookup.test(url.pathname) &&
				!(sessionPath && knownSessionPaths.has(url.pathname))
			) {
				json(notFound, 404);
				return;
			}
			if (url.pathname === '/api/v1/version') {
				if (req.method === 'GET' && url.search === '')
					respond({ api_version: 1, contract_revision: state.revision });
				else json(invalidRequest, 400);
				return;
			}
			if (url.pathname === '/api/v1/session/sign-in' && req.method === 'POST') {
				const { login } = await body(req);
				state.view =
					login === 'Blair'
						? { ...sessionView, account: { id: 'blair-id', login: 'Blair' } }
						: login === 'admin'
							? { ...sessionView, account: { id: 'admin-id', login: 'admin' }, admin: true }
							: sessionView;
				establish();
				return;
			}
			const signOut = url.pathname === '/api/v1/session/sign-out' && req.method === 'POST';
			// Host/origin refusals come before authentication: nothing changes, the cookie stays.
			if (signOut && state.signOutRefused) {
				json(
					state.signOutRefused === 403
						? { error: { code: 'not_authorized', message: 'Request origin is not authorized.' } }
						: invalidRequest,
					state.signOutRefused
				);
				return;
			}
			// A sign-out that authenticates or is refused with 401 clears the cookie half.
			const ended = signOut ? { value: '', clear: true } : undefined;
			// Cookie-only, proof-only, stale and mismatched pairs share the gateway's single denial shape.
			const presented = presentedPair(req);
			if (
				!presented ||
				!state.pair ||
				presented.cookie !== state.pair.cookie ||
				presented.proof !== state.pair.proof
			) {
				json(
					{
						error: {
							code: 'not_authorized',
							message: 'Session ended. Sign in again.',
							session_reason: 'session_ended'
						}
					},
					401,
					ended
				);
				return;
			}
			if (url.pathname === '/api/v1/session' && req.method === 'GET') {
				state.gets++;
				respond(state.view);
				return;
			}
			// The organization view of web-api-v1 section 3, made from what the create asked for.
			const link = (operation_id: string) => {
				const asked = state.creates.find((c) => c.operation_id === operation_id)?.params;
				const id = `org-${operation_id.slice(-6).toLowerCase()}`;
				const view = {
					id,
					slug: asked?.slug ?? 'unknown',
					name: asked?.name ?? 'Unknown',
					created_at: '2026-11-18T10:00:00.000Z',
					employee_id: 'person-1',
					recovery: 'pending',
					offices: [
						{ id: `${id}-hq`, organization_id: id, name: 'HQ', role: 'hq', host: 'hq-mac', status: 'starting' }
					]
				};
				if (!state.organizations.some((o) => o.id === id)) state.organizations.push(view);
				return view;
			};
			const refusal = (error: Refusal) => ({ message: 'Refused.', ...error });
			if (url.pathname === '/api/v1/instance') {
				if (req.method === 'GET' && url.search === '')
					respond({
						instance_id: state.view.instance_id,
						name: 'default',
						server_version: '0.0.0-test',
						web_status: 'ok',
						listen_scheme: 'http_loopback',
						server_time_ms: Date.now()
					});
				else json(invalidRequest, 400);
				return;
			}
			if (url.pathname === '/api/v1/organizations' && req.method === 'POST') {
				const sent = await body(req);
				const operation_id = sent?.operation_id;
				if (
					typeof operation_id !== 'string' ||
					!ulid.test(operation_id) ||
					Object.keys(sent).sort().join() !== 'operation_id,params,scope' ||
					sent.scope?.kind !== 'instance' ||
					sent.scope?.instance_id !== state.view.instance_id ||
					Object.keys(sent.params ?? {}).sort().join() !== 'name,organization_repository,slug,state_branch'
				) {
					json(invalidRequest, 400);
					return;
				}
				const step = state.createScript.shift() ?? 'accepted';
				if (typeof step === 'object' && 'refused' in step) {
					// The web process refuses before any host call: nothing is recorded.
					json({ error: { ...refusal(step.refused), code: 'invalid_request' } }, 400);
					return;
				}
				state.creates.push({ operation_id, params: sent.params });
				if (step === 'lost') {
					// What a browser gets when the answer is lost on the way: no JSON it can read.
					res.statusCode = 502;
					res.setHeader('Content-Type', 'text/html');
					res.end('<html>Bad gateway</html>');
				}
				else if (step === 'accepted') json({ outcome: 'accepted', operation_id }, 202);
				else if (step === 'completed') json({ outcome: 'completed', operation_id, result: link(operation_id) });
				else if (step === 'unknown') json({ outcome: 'unknown', operation_id }, 504);
				else if (step === 'unavailable') json({ error: { code: 'unavailable', message: 'Offline.' } }, 503);
				else json({ outcome: 'rejected', operation_id, error: refusal(step.rejected) }, step.status);
				return;
			}
			const looked = hostLookup.exec(url.pathname);
			if (looked) {
				const operation_id = looked[1];
				if (
					req.method !== 'GET' ||
					!ulid.test(operation_id) ||
					url.search !== `?instance_id=${state.view.instance_id}`
				) {
					json(invalidRequest, 400);
					return;
				}
				state.lookups.push(operation_id);
				const step = state.lookupScript.shift() ?? 'completed';
				const known = state.creates.some((c) => c.operation_id === operation_id);
				const result = (rest: Record<string, unknown>) => respond({ operation_id, kind: 'host', ...rest });
				if (step === 'missing' || !known) json(notFound, 404);
				else if (step === 'unavailable') json({ error: { code: 'unavailable', message: 'Offline.' } }, 503);
				else if (step === 'expired') json({ error: { code: 'expired', message: 'Expired.' } }, 410);
				else if (step === 'accepted') result({ outcome: 'accepted', state: 'reserved' });
				else if (step === 'completed') result({ outcome: 'completed', state: 'linked', result: link(operation_id) });
				else result({ outcome: 'rejected', error: refusal(step.rejected) });
				return;
			}
			if (url.pathname === '/api/v1/organizations') {
				if (req.method === 'GET' && url.search === '')
					respond({ organizations: state.organizations, creating: [] });
				else json(invalidRequest, 400);
				return;
			}
			if (url.pathname === '/api/v1/session/refresh' && req.method === 'POST') {
				state.refreshCount++;
				if (state.hold)
					await new Promise<void>((resolve) => {
						state.release = resolve;
					});
				establish();
				return;
			}
			if (signOut) {
				// A 503 may come before dispatch (web-api-v1 §5), and the host deletes the stored
				// record first and never ends sessions on a restart (ADR-0013). An unavailable reply
				// (pool refusal, or the gateway's backend timeout from
				// apps/agentic-org-web/src/service.rs `CallKind::unknown`) therefore changes nothing:
				// the pair keeps authenticating and the cookie half stays, so a retry can end it.
				if (state.signOutFails) {
					json({ error: { code: 'unavailable', message: 'Offline.' } }, 503);
					return;
				}
				if (state.signOutTimesOut) {
					json({ error: { code: 'timeout', message: 'Backend did not answer in time.' } }, 503);
					return;
				}
				state.pair = undefined;
				// The body is checked only after that invalidation, so a refused body (anything
				// but `{}`) is a 400 that still ends the pair and clears the cookie half. The UI
				// cannot tell it from a 400 before authentication and keeps its record until the
				// next 401; it always sends `{}`, so only a foreign request reaches this.
				if (!(await emptyObjectBody(req))) {
					json(invalidRequest, 400, ended);
					return;
				}
				respond({}, ended);
				return;
			}
			// A known session path with another method is an invalid request after authentication.
			json(invalidRequest, 400);
		} catch {
			json(invalidRequest, 400);
		}
	};
}
