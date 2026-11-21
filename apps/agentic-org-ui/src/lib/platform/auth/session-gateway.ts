import { NotDispatchedError } from '../http/errors';
import type {
	createAuthlessJsonHttp,
	createGatewayPublicHttp,
	createJsonHttp
} from '../http/json-http';
import {
	readAccess,
	readCliApprovalDecide,
	readCliApprovalDecision,
	readEstablishedSession,
	readResponse,
	readSession,
	readTlsEnableRequest,
	readTlsEnableResult,
	readVersion,
	WireDecodeError,
	type AccessView,
	type CliApprovalDecision,
	type GatewayError,
	type Reader,
	type TlsEnableResult
} from '../http/wire';

/** HTTP failure metadata only: no server message or credential appears in an error. */
export class SessionRequestError extends Error {
	readonly retryAfterMs: number | undefined;
	readonly sessionReason: GatewayError['session_reason'];
	readonly code: string;
	readonly field: GatewayError['field'];
	readonly rule: GatewayError['rule'];
	constructor(
		readonly status: number,
		error: GatewayError
	) {
		super('Session request failed');
		this.name = 'SessionRequestError';
		this.retryAfterMs = error.retry_after_ms;
		this.sessionReason = error.session_reason;
		// Stable machine fields only; the server's message text never reaches the page.
		this.code = error.code;
		this.field = error.field;
		this.rule = error.rule;
	}
}
export type SessionView = ReturnType<typeof readSession>;
export type EstablishedSession = ReturnType<typeof readEstablishedSession>;
export type { AccessView, CliApprovalDecision, TlsEnableResult };
export type LinkInspection =
	| { kind: 'administrator_registration' }
	| { kind: 'organization_invite'; org_id: string; org_name: string; member_id: string }
	| { kind: 'set_password'; login: string };

const readLinkInspection: Reader<LinkInspection> = (value) => {
	if (!value || typeof value !== 'object' || Array.isArray(value)) throw new WireDecodeError();
	const v = value as Record<string, unknown>;
	if (v.kind === 'administrator_registration') return { kind: 'administrator_registration' };
	if (
		v.kind === 'organization_invite' &&
		typeof v.org_id === 'string' &&
		typeof v.org_name === 'string' &&
		typeof v.member_id === 'string'
	) {
		return {
			kind: 'organization_invite',
			org_id: v.org_id,
			org_name: v.org_name,
			member_id: v.member_id
		};
	}
	if (v.kind === 'set_password' && typeof v.login === 'string') {
		return { kind: 'set_password', login: v.login };
	}
	throw new WireDecodeError();
};

const readSetPasswordResult: Reader<{ login: string }> = (value) => {
	if (
		!value ||
		typeof value !== 'object' ||
		typeof (value as { login?: unknown }).login !== 'string'
	)
		throw new WireDecodeError();
	return { login: (value as { login: string }).login };
};

// readResponse validates an object; this projection discards additive response fields.
const emptySessionResult: Reader<Record<string, never>> = () => ({});

/** Fixed session operations over the shared bounded, same-origin HTTP boundary. */
export function createSessionGateway(
	publicHttp: ReturnType<typeof createGatewayPublicHttp>,
	protectedHttp: ReturnType<typeof createJsonHttp>,
	authlessHttp?: ReturnType<typeof createAuthlessJsonHttp>
) {
	async function result<T>(
		request: Promise<{ status: number; body: unknown }>,
		reader: Reader<T>
	): Promise<T> {
		const response = await request;
		const envelope = readResponse(response.body, reader);
		if ('error' in envelope) {
			if (response.status >= 200 && response.status < 300) throw new WireDecodeError();
			throw new SessionRequestError(response.status, envelope.error);
		}
		if (response.status !== 200) throw new WireDecodeError();
		return envelope.result;
	}
	const requireAuthless = () => {
		if (!authlessHttp) throw new NotDispatchedError('Local authless transport unavailable');
		return authlessHttp;
	};
	return Object.freeze({
		version: (signal?: AbortSignal) => result(publicHttp.version(signal), readVersion),
		access: (signal?: AbortSignal) => result(publicHttp.access(signal), readAccess),
		signIn: (credentials: unknown, signal?: AbortSignal) =>
			result(publicHttp.signIn(credentials, signal), readEstablishedSession),
		register: (request: unknown, signal?: AbortSignal) =>
			result(publicHttp.register(request, signal), readEstablishedSession),
		registerSignIn: (request: unknown, signal?: AbortSignal) =>
			result(publicHttp.registerSignIn(request, signal), readEstablishedSession),
		inspectLink: (
			request: { link: string; kind: 'registration' | 'set_password' },
			signal?: AbortSignal
		) => result(publicHttp.inspectLink(request, signal), readLinkInspection),
		setPassword: (request: { link: string; password: string }, signal?: AbortSignal) =>
			result(publicHttp.setPassword(request, signal), readSetPasswordResult),
		enableAuth: (credentials: unknown, signal?: AbortSignal) =>
			result(publicHttp.enableAuth(credentials, signal), readEstablishedSession),
		get: (proof: string, signal?: AbortSignal) =>
			result(protectedHttp({ method: 'GET', path: '/api/v1/session', proof, signal }), readSession),
		getLocal: (signal?: AbortSignal) =>
			result(requireAuthless()({ method: 'GET', path: '/api/v1/session', signal }), readSession),
		enableTls: (request: unknown, proof?: string, signal?: AbortSignal) => {
			let body;
			try {
				body = readTlsEnableRequest(request);
			} catch {
				return Promise.reject(new NotDispatchedError('Invalid TLS configuration request'));
			}
			const send =
				proof !== undefined
					? protectedHttp({ method: 'POST', path: '/api/v1/instance/tls', body, proof, signal })
					: requireAuthless()({ method: 'POST', path: '/api/v1/instance/tls', body, signal });
			return result(send, readTlsEnableResult);
		},
		decideCliApproval: (request: unknown, proof?: string, signal?: AbortSignal) => {
			let body;
			try {
				body = readCliApprovalDecide(request);
			} catch {
				return Promise.reject(new NotDispatchedError('Invalid CLI approval request'));
			}
			const send =
				proof !== undefined
					? protectedHttp({
							method: 'POST',
							path: '/api/v1/cli-approvals/decide',
							body,
							proof,
							signal
						})
					: requireAuthless()({
							method: 'POST',
							path: '/api/v1/cli-approvals/decide',
							body,
							signal
						});
			return result(send, readCliApprovalDecision);
		},
		refresh: (proof: string, signal?: AbortSignal) =>
			result(
				protectedHttp({ method: 'POST', path: '/api/v1/session/refresh', body: {}, proof, signal }),
				readEstablishedSession
			),
		signOut: (proof: string, signal?: AbortSignal) =>
			result(
				protectedHttp({
					method: 'POST',
					path: '/api/v1/session/sign-out',
					body: {},
					proof,
					signal
				}),
				emptySessionResult
			)
	});
}
export type SessionGateway = ReturnType<typeof createSessionGateway>;
