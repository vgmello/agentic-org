import type { createGatewayPublicHttp, createJsonHttp } from '../http/json-http';
import { readEstablishedSession, readResponse, readSession, readVersion, WireDecodeError, type GatewayError, type Reader } from '../http/wire';

/** HTTP failure metadata only: no server message or credential appears in an error. */
export class SessionRequestError extends Error {
	readonly retryAfterMs: number | undefined;
	readonly sessionReason: GatewayError['session_reason'];
	constructor(readonly status: number, error: GatewayError) {
		super('Session request failed');
		this.name = 'SessionRequestError';
		this.retryAfterMs = error.retry_after_ms;
		this.sessionReason = error.session_reason;
	}
}
export type SessionView = ReturnType<typeof readSession>;
export type EstablishedSession = ReturnType<typeof readEstablishedSession>;
// readResponse validates an object; this projection discards additive response fields.
const emptySessionResult: Reader<Record<string, never>> = () => ({});

/** Fixed session operations over the shared bounded, same-origin HTTP boundary. */
export function createSessionGateway(publicHttp: ReturnType<typeof createGatewayPublicHttp>, protectedHttp: ReturnType<typeof createJsonHttp>) {
	async function result<T>(request: Promise<{ status: number; body: unknown }>, reader: Reader<T>): Promise<T> {
		const response = await request;
		const envelope = readResponse(response.body, reader);
		if ('error' in envelope) {
			if (response.status >= 200 && response.status < 300) throw new WireDecodeError();
			throw new SessionRequestError(response.status, envelope.error);
		}
		if (response.status !== 200) throw new WireDecodeError();
		return envelope.result;
	}
	return Object.freeze({
		version: (signal?: AbortSignal) => result(publicHttp.version(signal), readVersion),
		signIn: (credentials: unknown, signal?: AbortSignal) => result(publicHttp.signIn(credentials, signal), readEstablishedSession),
		get: (proof: string, signal?: AbortSignal) => result(protectedHttp({ method: 'GET', path: '/api/v1/session', proof, signal }), readSession),
		refresh: (proof: string, signal?: AbortSignal) => result(protectedHttp({ method: 'POST', path: '/api/v1/session/refresh', body: {}, proof, signal }), readEstablishedSession),
		signOut: (proof: string, signal?: AbortSignal) => result(protectedHttp({ method: 'POST', path: '/api/v1/session/sign-out', body: {}, proof, signal }), emptySessionResult)
	});
}
export type SessionGateway = ReturnType<typeof createSessionGateway>;
