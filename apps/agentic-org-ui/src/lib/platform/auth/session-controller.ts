import type { createJsonHttp } from '../http/json-http';
import { readJsonObject, readResponse, readSafeInteger, versionCompatible } from '../http/wire';
import {
	SessionRequestError,
	type EstablishedSession,
	type SessionGateway,
	type SessionView
} from './session-gateway';
import { readSessionRecord, type SessionRecord, type SessionStore } from './session-record';

/** The API and this page disagree on the contract version: the page must reload (§4a.3 "Page out of date"). */
export class UpdateRequiredError extends Error {
	constructor() {
		super('Gateway update required');
		this.name = 'UpdateRequiredError';
	}
}
export type SessionState =
	| { kind: 'signed-out'; reason?: 'ended' | 'signed-out' | 'changed-account'; login?: string }
	| { kind: 'update-required' }
	| {
			kind: 'signed-in';
			session: SessionView;
			reauth?: 'session_ended' | 'unknown';
	  };
type Options = {
	store: SessionStore;
	gateway: SessionGateway;
	http: ReturnType<typeof createJsonHttp>;
	locks: Pick<LockManager, 'request'>;
	channel: Pick<BroadcastChannel, 'postMessage' | 'addEventListener' | 'removeEventListener'>;
	now: () => number;
	changed: (state: SessionState) => void;
};
const sameAccount = (a: SessionRecord, b: SessionRecord) =>
	a.instance_id === b.instance_id && a.account_id === b.account_id;
const matches = (view: SessionView, record: SessionRecord) =>
	view.instance_id === record.instance_id && view.account.id === record.account_id;

/**
 * Sign-out refusals that prove the host no longer holds this browser's session. Only an
 * unauthenticated 401 does: a 503 may come before dispatch, and the host deletes the stored
 * record first, so an unavailable reply leaves the session in place (ADR-0013, SIGN-29).
 */
const SIGN_OUT_ENDED: ReadonlySet<number> = new Set([401]);

/** Owns the browser pair. UI receives views only; every pair change shares the cross-tab lock. */
export function createSessionController({
	store,
	gateway,
	http,
	locks,
	channel,
	now,
	changed
}: Options) {
	let record: SessionRecord | undefined;
	let state: SessionState = { kind: 'signed-out' };
	let generation = 0;
	let recovery = 0;
	// Notifications share the latest negotiation; they never race past an unknown/rejected version.
	let versionReady: ReturnType<SessionGateway['version']> | undefined;
	// Only the pair operation holding the session lock waits here.
	let versionChanged: (() => void) | undefined;
	const negotiate = (signal?: AbortSignal) => {
		versionReady = gateway.version(signal);
		versionChanged?.();
		return versionReady;
	};
	const authoritative = (version: typeof versionReady) => !disposed && version === versionReady;
	let disposed = false;
	const publish = (next: SessionState) => {
		state = Object.freeze(next);
		if (!disposed) changed(state);
	};
	const active = (token: number) => !disposed && generation === token;
	const ensureOpen = () => {
		if (disposed) throw new Error('Session controller closed');
	};
	const recovering = (token: number, attempt: number) => active(token) && recovery === attempt;
	const ended = (reason: 'ended' | 'signed-out' | 'changed-account', login?: string) => {
		generation++;
		recovery++;
		record = undefined;
		publish({ kind: 'signed-out', reason, ...(login === undefined ? {} : { login }) });
	};
	const reauth = (reason: 'session_ended' | 'unknown' = 'session_ended') => {
		if (state.kind === 'signed-in') publish({ ...state, reauth: reason });
	};
	const viewOf = ({ proof: _proof, ...view }: EstablishedSession): SessionView => view;
	function accept(session: EstablishedSession) {
		return readSessionRecord({
			instance_id: session.instance_id,
			account_id: session.account.id,
			login: session.account.login,
			proof: session.proof,
			rotated_at_ms: now()
		});
	}
	const notify = (
		message: { kind: 'signed-in'; account_id: string } | { kind: 'rotated' | 'signed-out' }
	) => {
		if (!disposed) channel.postMessage(message);
	};
	async function publishEstablished(
		session: EstablishedSession,
		next: SessionRecord,
		message: Parameters<typeof notify>[0]
	) {
		// A dispatched pair change must persist even if boot renegotiates. Publish it
		// against the latest negotiation so a compatible boot cannot strand its view.
		while (!disposed) {
			const version = versionReady;
			if (!version) return;
			const superseded = new Promise<void>((resolve) => {
				versionChanged = resolve;
			});
			try {
				const info = await Promise.race([version, superseded]);
				if (!authoritative(version)) continue;
				if (!info || !versionCompatible(info)) return;
				recovery++;
				if (record && !sameAccount(record, next)) generation++;
				record = next;
				publish({ kind: 'signed-in', session: viewOf(session) });
				notify(message);
				return;
			} catch {
				if (authoritative(version)) return;
			} finally {
				versionChanged = undefined;
			}
		}
	}
	const accountChanged = (stored: SessionRecord) =>
		(record && !sameAccount(record, stored)) ||
		(state.kind === 'signed-out' && state.reason === 'changed-account');
	async function boot() {
		ensureOpen();
		const token = generation,
			attempt = ++recovery;
		const version = await negotiate();
		if (!recovering(token, attempt)) return;
		if (!versionCompatible(version)) {
			generation++;
			recovery++;
			record = undefined;
			publish({ kind: 'update-required' });
			return;
		}
		const stored = await store.read();
		if (!recovering(token, attempt)) return;
		if (!stored) {
			ended('ended');
			return;
		}
		if (accountChanged(stored)) {
			ended('changed-account', stored.login);
			return;
		}
		try {
			const session = await gateway.get(stored.proof);
			if (!recovering(token, attempt)) return;
			if (!matches(session, stored)) {
				ended('ended');
				return;
			}
			record = stored;
			publish({ kind: 'signed-in', session });
		} catch (error) {
			if (!recovering(token, attempt)) return;
			if (error instanceof SessionRequestError && error.status === 401) {
				ended('ended');
				return;
			}
			throw error;
		}
	}
	async function synchronize() {
		ensureOpen();
		const version = versionReady ?? negotiate();
		if (!versionCompatible(await version) || version !== versionReady || disposed) return;
		const token = generation,
			attempt = ++recovery;
		const stored = await store.read();
		if (!recovering(token, attempt)) return;
		if (!stored) {
			ended('signed-out');
			return;
		}
		if (accountChanged(stored)) {
			ended('changed-account', stored.login);
			return;
		}
		try {
			const session = await gateway.get(stored.proof);
			if (!recovering(token, attempt)) return;
			if (!matches(session, stored)) {
				ended('ended');
				return;
			}
			record = stored;
			publish({ kind: 'signed-in', session });
		} catch (error) {
			if (!recovering(token, attempt)) return;
			if (error instanceof SessionRequestError && error.status === 401) reauth(error.sessionReason);
			else throw error;
		}
	}
	const received: EventListener = (event) => {
		const value: unknown = (event as MessageEvent).data;
		if (!value || typeof value !== 'object') return;
		const message = value as Record<string, unknown>;
		if (
			typeof message.kind !== 'string' ||
			!['signed-in', 'rotated', 'signed-out'].includes(message.kind)
		)
			return;
		if (message.kind === 'signed-in' && typeof message.account_id !== 'string') return;
		// The record is authoritative; a broadcast never supplies a proof or a session view.
		void synchronize().catch(() => {
			/* Keep existing state on transient storage/network failure. */
		});
	};
	channel.addEventListener('message', received);
	/** Sign-in and registration both create a pair under the cross-tab session lock. */
	async function establish(
		call: (signal?: AbortSignal) => Promise<EstablishedSession>,
		signal?: AbortSignal
	) {
		ensureOpen();
		return locks.request('org-session:refresh', async () => {
			ensureOpen();
			recovery++;
			const version = negotiate(signal),
				info = await version;
			if (!authoritative(version)) throw new Error('Session changed');
			if (!versionCompatible(info)) {
				generation++;
				recovery++;
				record = undefined;
				publish({ kind: 'update-required' });
				throw new UpdateRequiredError();
			}
			const session = await call(signal);
			const next = accept(session);
			await store.write(next);
			await publishEstablished(session, next, { kind: 'signed-in', account_id: next.account_id });
		});
	}
	async function send(request: { path: string; signal?: AbortSignal } & ({ method: 'GET' } | { method: 'POST'; body: unknown })) {
		ensureOpen();
		const expected = record;
		if (!expected) throw new Error('Sign in required');
		const token = generation;
		const current = () => {
			if (!active(token)) throw new Error('Session changed');
		};
		let response = await http({ ...request, proof: expected.proof });
		current();
		if (response.status !== 401) return response;
		const stored = await store.read();
		current();
		if (!stored || !sameAccount(expected, stored)) {
			ended(stored ? 'changed-account' : 'ended', stored?.login);
			throw new Error('Session changed');
		}
		if (stored.proof !== expected.proof) {
			record = stored;
			response = await http({ ...request, proof: stored.proof });
			current();
			if (response.status !== 401) return response;
		}
		let reason: 'session_ended' | 'unknown' | undefined;
		try {
			const body = readResponse(response.body, readJsonObject);
			if ('error' in body) reason = body.error.session_reason;
		} catch {
			/* Untrusted error payload. */
		}
		reauth(reason);
		return response;
	}
	return Object.freeze({
		boot,
		signIn: (credentials: unknown, signal?: AbortSignal) =>
			establish((signal) => gateway.signIn(credentials, signal), signal),
		/** Redeems a registration link; the new account is signed in exactly as by sign-in. */
		register: (request: unknown, signal?: AbortSignal) =>
			establish((signal) => gateway.register(request, signal), signal),
		async refresh(triggered_at_ms: number, signal?: AbortSignal) {
			ensureOpen();
			readSafeInteger(triggered_at_ms);
			const expected = record;
			if (!expected) return;
			await locks.request('org-session:refresh', async () => {
				ensureOpen();
				const version = versionReady!;
				if (!versionCompatible(await version) || !authoritative(version)) return;
				const stored = await store.read();
				if (!authoritative(version)) return;
				if (!stored) {
					ended('signed-out');
					return;
				}
				if (!sameAccount(expected, stored)) {
					ended('changed-account', stored.login);
					return;
				}
				if (stored.rotated_at_ms > triggered_at_ms) {
					record = stored;
					return;
				}
				try {
					const session = await gateway.refresh(stored.proof, signal);
					if (!matches(session, stored)) {
						if (authoritative(version)) ended('ended');
						return;
					}
					const next = accept(session);
					await store.write(next);
					await publishEstablished(session, next, { kind: 'rotated' });
				} catch (error) {
					if (
						authoritative(version) &&
						error instanceof SessionRequestError &&
						error.status === 401
					)
						reauth(error.sessionReason);
					throw error;
				}
			});
		},
		async signOut(signal?: AbortSignal) {
			ensureOpen();
			const expected = record;
			if (!expected) return;
			await locks.request('org-session:refresh', async () => {
				ensureOpen();
				const version = versionReady!;
				if (!versionCompatible(await version) || !authoritative(version)) return;
				const stored = await store.read();
				if (!authoritative(version)) return;
				if (!stored || !sameAccount(expected, stored)) {
					ended(stored ? 'changed-account' : 'signed-out', stored?.login);
					return;
				}
				// Only a confirmed end clears this browser's pair: success, or an unauthenticated
				// 401 proving the host no longer holds the session. A 503 may come before dispatch
				// and a restart never ends a stored session (ADR-0013), so an unavailable reply,
				// like a 400/403 before authentication or no reply at all, keeps the session and
				// the proof for a retry (UX SIGN-29: "Couldn't sign out", Try again). Documented
				// limit: a refusal after authentication (a 400 body, a 413 too-large body) may
				// already have ended the session, but a 400 is indistinguishable by status from one
				// before authentication, so this record is kept until the next 401. This
				// controller always sends `{}`, so it never causes one.
				let refusal: SessionRequestError | undefined;
				try {
					await gateway.signOut(stored.proof, signal);
				} catch (error) {
					if (!(error instanceof SessionRequestError) || !SIGN_OUT_ENDED.has(error.status))
						throw error;
					refusal = error;
				}
				await store.clear();
				if (authoritative(version)) ended('signed-out');
				notify({ kind: 'signed-out' });
				if (refusal) throw refusal;
			});
		},
		read: (path: string, signal?: AbortSignal) => send({ method: 'GET', path, signal }),
		/**
		 * One POST with the current pair. The caller owns idempotency: its body carries the
		 * operation ID. Like `read`, it is sent again only after a 401 (which proves it was not
		 * admitted) and only when another tab has already rotated this account's proof.
		 */
		submit: (path: string, body: unknown, signal?: AbortSignal) =>
			send({ method: 'POST', path, body, signal }),
		dispose() {
			disposed = true;
			versionChanged?.();
			generation++;
			recovery++;
			record = undefined;
			channel.removeEventListener('message', received);
		}
	});
}
