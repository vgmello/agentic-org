import { describe, expect, it, vi } from 'vitest';
import {
	readAccess,
	readCliApprovalDecide,
	readCliApprovalDecision,
	readInstance,
	readTlsEnableRequest,
	readTlsEnableResult,
	WireDecodeError
} from '../http/wire';
import { createAuthlessJsonHttp, createGatewayPublicHttp, createJsonHttp } from '../http/json-http';
import { NotDispatchedError } from '../http/errors';
import { createSessionController, type SessionState } from './session-controller';
import { createSessionGateway, SessionRequestError, type AccessView } from './session-gateway';
import type { SessionRecord } from './session-record';
import { describeCliApprovalFailure } from './cli-approve';
import {
	describeTlsFailure,
	hstsLabel,
	isAuthAlreadyEnabled,
	listenSchemeLabel,
	validateTlsFields
} from '$lib/modules/settings/features/security/security-copy';

const proof = 'a'.repeat(64);
const view = {
	instance_id: 'hq',
	account: { id: 'owner-id', login: 'local-owner' },
	admin: true,
	organizations: [],
	server_time_ms: 100,
	idle_expires_at: '2099-01-01T00:00:00.000Z'
};

describe('TASK-76 / TASK-62 / SIGN-40 wire readers', () => {
	it('decodes AccessView for known and unknown listen schemes', () => {
		expect(readAccess({ auth_enabled: false, listen_scheme: 'http_loopback' })).toEqual({
			auth_enabled: false,
			listen_scheme: 'http_loopback'
		});
		expect(readAccess({ auth_enabled: true, listen_scheme: 'http_published' })).toEqual({
			auth_enabled: true,
			listen_scheme: 'http_published'
		});
		expect(readAccess({ auth_enabled: true, listen_scheme: 'https' })).toEqual({
			auth_enabled: true,
			listen_scheme: 'https'
		});
		expect(readAccess({ auth_enabled: false, listen_scheme: 'future' })).toEqual({
			auth_enabled: false,
			listen_scheme: 'unknown'
		});
		expect(() => readAccess({ auth_enabled: 'no', listen_scheme: 'https' })).toThrow(
			WireDecodeError
		);
	});

	it('decodes optional auth_enabled on InstanceView while keeping omitted canonical shape', () => {
		const base = {
			instance_id: 'hq',
			name: 'default',
			server_version: '0.1.0',
			web_status: 'ok',
			listen_scheme: 'http_loopback',
			server_time_ms: 100
		};
		expect(readInstance(base)).toEqual(base);
		expect(readInstance({ ...base, auth_enabled: false }).auth_enabled).toBe(false);
		expect(readInstance({ ...base, auth_enabled: true }).auth_enabled).toBe(true);
		expect(() => readInstance({ ...base, auth_enabled: 'false' })).toThrow(WireDecodeError);
	});

	it('validates TlsEnableRequest and decodes TlsEnableResult', () => {
		expect(readTlsEnableRequest({ mode: 'self_signed' })).toEqual({ mode: 'self_signed' });
		expect(readTlsEnableRequest({ mode: 'self_signed', hsts: 'on' })).toEqual({
			mode: 'self_signed',
			hsts: 'on'
		});
		expect(
			readTlsEnableRequest({ mode: 'supplied', cert: 'CERT', key: 'KEY', hsts: 'clear' })
		).toEqual({ mode: 'supplied', cert: 'CERT', key: 'KEY', hsts: 'clear' });
		for (const bad of [
			{ mode: 'self_signed', cert: 'CERT' },
			{ mode: 'supplied', cert: 'CERT' },
			{ mode: 'supplied', cert: '', key: 'KEY' },
			{ mode: 'self_signed', hsts: 'invalid' },
			{ mode: 'other' }
		]) {
			expect(() => readTlsEnableRequest(bad)).toThrow(WireDecodeError);
		}
		expect(
			readTlsEnableResult({
				scheme: 'https',
				origin: 'https://127.0.0.1:8443',
				fingerprint: 'sha256:abcd',
				source: 'made',
				restart_required: true
			})
		).toEqual({
			scheme: 'https',
			origin: 'https://127.0.0.1:8443',
			fingerprint: 'sha256:abcd',
			source: 'made',
			restart_required: true
		});
		expect(() =>
			readTlsEnableResult({
				scheme: 'https',
				origin: '',
				fingerprint: 'sha256:abcd',
				source: 'made',
				restart_required: true
			})
		).toThrow(WireDecodeError);
	});

	it('validates CLI approval request and decision response', () => {
		expect(readCliApprovalDecide({ link: 'tok123', approve: true })).toEqual({
			link: 'tok123',
			approve: true
		});
		expect(() => readCliApprovalDecide({ link: '', approve: true })).toThrow(WireDecodeError);
		expect(readCliApprovalDecision({ status: 'approved' })).toEqual({ status: 'approved' });
		expect(readCliApprovalDecision({ status: 'declined' })).toEqual({ status: 'declined' });
		expect(readCliApprovalDecision({ status: 'other' })).toEqual({ status: 'unknown' });
	});
});

describe('session controller authless boot, opt-in auth/TLS, and CLI approval', () => {
	function makeController(
		handler: (path: string, init: RequestInit | undefined) => { status: number; body: unknown }
	) {
		const calls: { url: string; method: string; proof: string | null; body?: unknown }[] = [];
		const fetchFn = vi.fn<typeof fetch>(async (url, init) => {
			const parsedUrl = new URL(String(url));
			calls.push({
				url: parsedUrl.pathname + parsedUrl.search,
				method: init?.method ?? 'GET',
				proof: new Headers(init?.headers).get('X-Org-Session-Proof'),
				body: init?.body ? JSON.parse(String(init.body)) : undefined
			});
			const reply = handler(parsedUrl.pathname, init);
			return Response.json(reply.body, { status: reply.status });
		});
		const options = {
			origin: 'http://127.0.0.1:4173',
			fetch: fetchFn,
			timeoutMs: 2000,
			maxRequestBytes: 64 * 1024,
			maxResponseBytes: 64 * 1024
		};
		const http = createJsonHttp(options);
		const authlessHttp = createAuthlessJsonHttp(options);
		const gateway = createSessionGateway(createGatewayPublicHttp(options), http, authlessHttp);
		let stored: SessionRecord | undefined;
		const store = {
			read: vi.fn(async () => stored),
			write: vi.fn(async (next: SessionRecord) => {
				stored = next;
			}),
			clear: vi.fn(async () => {
				stored = undefined;
			})
		};
		const states: SessionState[] = [];
		const accesses: AccessView[] = [];
		const broadcasts: unknown[] = [];
		const locks = {
			request: ((_name: string, callback: () => unknown) =>
				Promise.resolve(callback())) as LockManager['request']
		};
		const controller = createSessionController({
			store,
			gateway,
			http,
			authlessHttp,
			locks,
			channel: {
				postMessage: (message) => broadcasts.push(message),
				addEventListener: () => {},
				removeEventListener: () => {}
			},
			now: () => 500,
			changed: (next) => states.push(next),
			accessChanged: (next) => accesses.push(next)
		});
		return { controller, calls, states, accesses, broadcasts, store, stored: () => stored };
	}

	it('boots directly into signed-in authless mode and sends requests without X-Org-Session-Proof', async () => {
		const env = makeController((path) => {
			if (path === '/api/v1/version')
				return { status: 200, body: { result: { api_version: 1, contract_revision: 2 } } };
			if (path === '/api/v1/access')
				return {
					status: 200,
					body: { result: { auth_enabled: false, listen_scheme: 'http_loopback' } }
				};
			if (path === '/api/v1/session') return { status: 200, body: { result: view } };
			if (path === '/api/v1/organizations')
				return { status: 200, body: { result: { organizations: [], creating: [] } } };
			if (path === '/api/v1/cli-approvals/decide')
				return { status: 200, body: { result: { status: 'approved' } } };
			return { status: 404, body: { error: { code: 'not_found', message: 'Not found.' } } };
		});

		await env.controller.boot();
		expect(env.controller.access()).toEqual({
			auth_enabled: false,
			listen_scheme: 'http_loopback'
		});
		expect(env.states.at(-1)).toEqual({ kind: 'signed-in', session: view, authless: true });

		const orgs = await env.controller.read('/api/v1/organizations');
		expect(orgs.status).toBe(200);
		expect(env.calls.find((c) => c.url === '/api/v1/organizations')?.proof).toBeNull();

		const approval = await env.controller.decideCliApproval({ link: 'secret-tok', approve: true });
		expect(approval).toEqual({ status: 'approved' });
		expect(env.calls.find((c) => c.url === '/api/v1/cli-approvals/decide')?.proof).toBeNull();
	});

	it('falls back to auth_enabled=true when /api/v1/access returns 404', async () => {
		const env = makeController((path) => {
			if (path === '/api/v1/version')
				return { status: 200, body: { result: { api_version: 1, contract_revision: 2 } } };
			return { status: 404, body: { error: { code: 'not_found', message: 'Not found.' } } };
		});
		await env.controller.boot();
		expect(env.controller.access()).toEqual({ auth_enabled: true, listen_scheme: 'unknown' });
		expect(env.states.at(-1)).toEqual({ kind: 'signed-out', reason: 'ended' });
	});

	it('enables auth, stores proof, flips auth_enabled=true, then enables HTTPS with proof', async () => {
		let authEnabled = false;
		const env = makeController((path) => {
			if (path === '/api/v1/version')
				return { status: 200, body: { result: { api_version: 1, contract_revision: 2 } } };
			if (path === '/api/v1/access')
				return {
					status: 200,
					body: { result: { auth_enabled: authEnabled, listen_scheme: 'http_loopback' } }
				};
			if (path === '/api/v1/session') return { status: 200, body: { result: view } };
			if (path === '/api/v1/instance/auth/enable') {
				authEnabled = true;
				return {
					status: 200,
					body: {
						result: {
							...view,
							account: { id: 'owner-id', login: 'alice' },
							proof
						}
					}
				};
			}
			if (path === '/api/v1/instance/tls') {
				return {
					status: 200,
					body: {
						result: {
							scheme: 'https',
							origin: 'https://127.0.0.1:4173',
							fingerprint: 'sha256:1234',
							source: 'made',
							restart_required: true
						}
					}
				};
			}
			return { status: 404, body: { error: { code: 'not_found', message: 'Not found.' } } };
		});

		await env.controller.boot();
		expect(env.controller.access().auth_enabled).toBe(false);

		await env.controller.enableAuth({ login: 'alice', password: 'correct horse battery' });
		expect(env.controller.access().auth_enabled).toBe(true);
		expect(env.stored()?.proof).toBe(proof);
		expect(env.states.at(-1)).toEqual({
			kind: 'signed-in',
			session: { ...view, account: { id: 'owner-id', login: 'alice' } }
		});
		expect(env.broadcasts).toContainEqual({ kind: 'signed-in', account_id: 'owner-id' });

		const tls = await env.controller.enableTls({ mode: 'self_signed', hsts: 'off' });
		expect(tls.origin).toBe('https://127.0.0.1:4173');
		expect(tls.restart_required).toBe(true);
		expect(env.calls.find((c) => c.url === '/api/v1/instance/tls')?.proof).toBe(proof);
	});

	it('updates listen_scheme immediately on live HTTPS rotation and rejects malformed TLS requests before fetch', async () => {
		const env = makeController((path) => {
			if (path === '/api/v1/version')
				return { status: 200, body: { result: { api_version: 1, contract_revision: 2 } } };
			if (path === '/api/v1/access')
				return { status: 200, body: { result: { auth_enabled: true, listen_scheme: 'https' } } };
			if (path === '/api/v1/session/sign-in')
				return { status: 200, body: { result: { ...view, proof } } };
			if (path === '/api/v1/instance/tls')
				return {
					status: 200,
					body: {
						result: {
							scheme: 'https',
							origin: 'https://127.0.0.1:4173',
							fingerprint: 'sha256:rotated',
							source: 'supplied',
							restart_required: false
						}
					}
				};
			return { status: 404, body: { error: { code: 'not_found', message: 'Not found.' } } };
		});

		await env.controller.signIn({ login: 'local-owner', password: 'correct horse battery' });
		await expect(env.controller.enableTls({ mode: 'supplied', cert: '', key: '' })).rejects.toThrow(
			NotDispatchedError
		);
		expect(env.calls.some((c) => c.url === '/api/v1/instance/tls')).toBe(false);

		const rotated = await env.controller.enableTls({
			mode: 'supplied',
			cert: 'PEM-CERT',
			key: 'PEM-KEY'
		});
		expect(rotated.restart_required).toBe(false);
		expect(env.controller.access().listen_scheme).toBe('https');
	});
});

describe('security-copy and cli-approve helpers', () => {
	it('formats listen schemes, HSTS labels, TLS validation and failure messages', () => {
		expect(listenSchemeLabel('http_loopback')).toBe('HTTP (loopback)');
		expect(listenSchemeLabel('http_published')).toBe('HTTP (published)');
		expect(listenSchemeLabel('https')).toBe('HTTPS');
		expect(listenSchemeLabel('unknown')).toBe('Unknown');
		expect(hstsLabel('off')).toContain('Off');
		expect(hstsLabel('on')).toContain('On');
		expect(hstsLabel('clear')).toContain('Clear');
		expect(validateTlsFields('self_signed', '', '')).toBeUndefined();
		expect(validateTlsFields('supplied', ' ', '')).toEqual({
			cert: 'Paste the PEM certificate chain.',
			key: 'Paste the PEM private key.'
		});
		expect(
			isAuthAlreadyEnabled(
				new SessionRequestError(409, { code: 'conflict', message: 'already enabled' })
			)
		).toBe(true);
		expect(
			describeTlsFailure(new SessionRequestError(409, { code: 'conflict', message: 'server' }))
		).toContain('Enable authentication above');
		expect(
			describeTlsFailure(
				new SessionRequestError(403, { code: 'not_authorized', message: 'server' })
			)
		).toContain('Only the instance administrator');
		expect(
			describeTlsFailure(
				new SessionRequestError(400, { code: 'invalid_request', message: 'server' })
			)
		).toContain('invalid');
	});

	it('maps CLI approval failure statuses without leaking server text', () => {
		expect(
			describeCliApprovalFailure(
				new SessionRequestError(404, { code: 'not_found', message: 'secret detail' })
			)
		).toEqual({
			kind: 'link_used',
			message: 'This link has expired or was already used.'
		});
		expect(
			describeCliApprovalFailure(
				new SessionRequestError(401, { code: 'not_authorized', message: 'secret detail' })
			).kind
		).toBe('auth_required');
		expect(describeCliApprovalFailure(new Error('network')).kind).toBe('unreachable');
	});
});
