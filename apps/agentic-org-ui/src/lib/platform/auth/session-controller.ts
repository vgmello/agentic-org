import type { createEventStreamClient, EventStreamHandlers, Subscription } from '../http/events';
import { createEventStreamManager } from './event-stream-manager';
import type { createAuthlessJsonHttp, createJsonHttp } from '../http/json-http';
import {
	readJsonObject,
	readResponse,
	readSafeInteger,
	SESSION_PROOF_HEADER,
	versionCompatible
} from '../http/wire';
import {
	SessionRequestError,
	type AccessView,
	type CliApprovalDecision,
	type EstablishedSession,
	type SessionGateway,
	type SessionView,
	type TlsEnableResult
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
			/** Present only when local HQ serves plain HTTP without authentication (ADR-0034). */
			authless?: true;
	  };
type ControllerGateway = Pick<
	SessionGateway,
	'version' | 'signIn' | 'register' | 'get' | 'refresh' | 'signOut'
> &
	Partial<
		Pick<
			SessionGateway,
			| 'access'
			| 'getLocal'
			| 'enableAuth'
			| 'enableTls'
			| 'decideCliApproval'
			| 'registerSignIn'
			| 'inspectLink'
			| 'setPassword'
			| 'changePassword'
		>
	>;
type Options = {
	store: SessionStore;
	gateway: ControllerGateway;
	http: ReturnType<typeof createJsonHttp>;
	authlessHttp?: ReturnType<typeof createAuthlessJsonHttp>;
	eventStream?: ReturnType<typeof createEventStreamClient>;
	locks: Pick<LockManager, 'request'>;
	channel: Pick<BroadcastChannel, 'postMessage' | 'addEventListener' | 'removeEventListener'>;
	now: () => number;
	changed: (state: SessionState) => void;
	accessChanged?: (access: AccessView) => void;
};
const DEFAULT_ACCESS: AccessView = Object.freeze({
	auth_enabled: true,
	listen_scheme: 'unknown'
});
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
	authlessHttp,
	eventStream,
	locks,
	channel,
	now,
	changed,
	accessChanged
}: Options) {
	let record: SessionRecord | undefined;
	let state: SessionState = { kind: 'signed-out' };
	let accessState: AccessView = DEFAULT_ACCESS;
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
	const publishAccess = (next: AccessView) => {
		accessState = Object.freeze({ ...next });
		if (!disposed) accessChanged?.(accessState);
	};
	const probeAccess = async (signal?: AbortSignal): Promise<AccessView> => {
		if (!gateway.access) return DEFAULT_ACCESS;
		try {
			return await gateway.access(signal);
		} catch (error) {
			if (
				error instanceof SessionRequestError &&
				(error.status === 404 || error.status === 405 || error.status === 501)
			) {
				return DEFAULT_ACCESS;
			}
			throw error;
		}
	};
	const active = (token: number) => !disposed && generation === token;
	const ensureOpen = () => {
		if (disposed) throw new Error('Session controller closed');
	};
	const recovering = (token: number, attempt: number) => active(token) && recovery === attempt;
	const ended = (reason: 'ended' | 'signed-out' | 'changed-account', login?: string) => {
		streams.closeAll();
		generation++;
		recovery++;
		record = undefined;
		publish({ kind: 'signed-out', reason, ...(login === undefined ? {} : { login }) });
	};
	const reauth = (reason: 'session_ended' | 'unknown' = 'session_ended') => {
		if (state.kind === 'signed-in') publish({ ...state, reauth: reason });
	};
	const streams = createEventStreamManager(eventStream, {
		isSignedIn: () => state.kind === 'signed-in',
		isDisposed: () => disposed,
		authEnabled: () => accessState.auth_enabled,
		currentRecord: () => record,
		async recoverRotatedProof(expected, sessionReason) {
			const token = generation;
			const stored = await store.read();
			if (!active(token)) return undefined;
			if (!stored || !sameAccount(expected, stored)) {
				ended(stored ? 'changed-account' : 'ended', stored?.login);
				return undefined;
			}
			if (stored.proof !== expected.proof) {
				record = stored;
				return stored.proof;
			}
			reauth(sessionReason);
			return undefined;
		}
	});
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
				if (!accessState.auth_enabled) {
					publishAccess({ ...accessState, auth_enabled: true });
				}
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
	async function tryBootAuthless(
		token: number,
		attempt: number,
		access: AccessView
	): Promise<boolean> {
		if (access.auth_enabled || !gateway.getLocal) return false;
		try {
			const session = await gateway.getLocal();
			if (!recovering(token, attempt)) return true;
			record = undefined;
			publish({ kind: 'signed-in', session, authless: true });
			return true;
		} catch (error) {
			if (!recovering(token, attempt)) return true;
			if (error instanceof SessionRequestError && error.status === 401) {
				publishAccess({ ...access, auth_enabled: true });
				return false;
			}
			throw error;
		}
	}
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
		const access = await probeAccess();
		if (!recovering(token, attempt)) return;
		publishAccess(access);
		if (await tryBootAuthless(token, attempt, access)) return;
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
			if (!accessState.auth_enabled) {
				publishAccess({ ...accessState, auth_enabled: true });
			}
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
	/** Sign-in, registration and opt-in auth enablement create a pair under the cross-tab session lock. */
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
	async function sendAuthless(
		request: { path: string; signal?: AbortSignal } & (
			{ method: 'GET' } | { method: 'POST'; body: unknown }
		)
	) {
		if (accessState.auth_enabled || state.kind !== 'signed-in' || !authlessHttp) {
			throw new Error('Sign in required');
		}
		const token = generation;
		const current = () => {
			if (!active(token)) throw new Error('Session changed');
		};
		const response = await authlessHttp(request);
		current();
		if (response.status !== 401) return response;
		publishAccess({ ...accessState, auth_enabled: true });
		const stored = await store.read();
		current();
		if (stored) {
			record = stored;
			const retried = await http({ ...request, proof: stored.proof });
			current();
			if (retried.status !== 401) {
				void synchronize().catch(() => {
					/* Keep retried response if background sync transiently fails. */
				});
				return retried;
			}
		}
		ended('ended');
		return response;
	}
	async function send(
		request: { path: string; signal?: AbortSignal } & (
			{ method: 'GET' } | { method: 'POST'; body: unknown }
		)
	) {
		ensureOpen();
		const expected = record;
		if (!expected) return sendAuthless(request);
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
	async function callAuthless<T>(call: (proof: string | undefined) => Promise<T>): Promise<T> {
		if (accessState.auth_enabled || state.kind !== 'signed-in') {
			throw new Error('Sign in required');
		}
		try {
			return await call(undefined);
		} catch (error) {
			if (error instanceof SessionRequestError && error.status === 401) {
				publishAccess({ ...accessState, auth_enabled: true });
				ended('ended');
			}
			throw error;
		}
	}
	async function recoverUnauthorizedCall<T>(
		expected: SessionRecord,
		token: number,
		caught: SessionRequestError,
		call: (proof: string | undefined) => Promise<T>
	): Promise<T> {
		const stored = await store.read();
		if (!active(token)) throw new Error('Session changed');
		if (!stored || !sameAccount(expected, stored)) {
			ended(stored ? 'changed-account' : 'ended', stored?.login);
			throw new Error('Session changed');
		}
		if (stored.proof !== expected.proof) {
			record = stored;
			return call(stored.proof);
		}
		reauth(caught.sessionReason);
		throw caught;
	}
	async function withCallerAuth<T>(call: (proof: string | undefined) => Promise<T>): Promise<T> {
		ensureOpen();
		const expected = record;
		if (!expected) return callAuthless(call);
		const token = generation;
		let caught: unknown;
		try {
			return await call(expected.proof);
		} catch (error) {
			caught = error;
		}
		if (!active(token) || !(caught instanceof SessionRequestError) || caught.status !== 401) {
			throw caught;
		}
		return recoverUnauthorizedCall(expected, token, caught, call);
	}
	async function recoverAuthEnabledRace() {
		publishAccess({ ...accessState, auth_enabled: true });
		if (record) return;
		const stored = await store.read();
		if (stored) {
			await synchronize().catch(() => {
				/* Keep recovered access flag if validation transiently fails. */
			});
			return;
		}
		ended('ended');
	}
	return Object.freeze({
		boot,
		access: () => accessState,
		async refreshAccess(signal?: AbortSignal): Promise<AccessView> {
			ensureOpen();
			const next = await probeAccess(signal);
			publishAccess(next);
			return next;
		},
		signIn: (credentials: unknown, signal?: AbortSignal) =>
			establish((signal) => gateway.signIn(credentials, signal), signal),
		/** Redeems a registration link; the new account is signed in exactly as by sign-in. */
		register: (request: unknown, signal?: AbortSignal) =>
			establish((signal) => gateway.register(request, signal), signal),
		/** Attaches an organization invite link to an existing account by signing in. */
		registerSignIn: (request: unknown, signal?: AbortSignal) => {
			if (!gateway.registerSignIn) throw new Error('Invite sign-in unavailable');
			const fn = gateway.registerSignIn;
			return establish((sig) => fn(request, sig), signal);
		},
		/** Inspects a registration or set-password link without redeeming it. */
		inspectLink: (
			request: { link: string; kind: 'registration' | 'set_password' },
			signal?: AbortSignal
		) => {
			if (!gateway.inspectLink) throw new Error('Link inspection unavailable');
			return gateway.inspectLink(request, signal);
		},
		/** Redeems a one-time set-password link. */
		setPassword: (request: { link: string; password: string }, signal?: AbortSignal) => {
			if (!gateway.setPassword) throw new Error('Password reset unavailable');
			return gateway.setPassword(request, signal);
		},
		/** Opt-in authentication enablement from local authless HTTP mode (ADR-0034). */
		async enableAuth(credentials: unknown, signal?: AbortSignal) {
			if (!gateway.enableAuth) throw new Error('Authentication enablement unavailable');
			const enable = gateway.enableAuth;
			try {
				await establish((sig) => enable(credentials, sig), signal);
			} catch (error) {
				if (
					error instanceof SessionRequestError &&
					(error.status === 409 || error.status === 401)
				) {
					await recoverAuthEnabledRace();
				}
				throw error;
			}
		},
		/** Opt-in HTTPS enablement or live TLS certificate rotation (ADR-0033 / TASK-62). */
		async enableTls(request: unknown, signal?: AbortSignal): Promise<TlsEnableResult> {
			if (!gateway.enableTls) throw new Error('TLS configuration unavailable');
			const enable = gateway.enableTls;
			const result = await withCallerAuth((proof) => enable(request, proof, signal));
			if (!result.restart_required) {
				publishAccess({ ...accessState, listen_scheme: result.scheme });
			}
			return result;
		},
		/** Approves or declines a pending CLI sign-in approval link (SIGN-40 / FC-173). */
		async decideCliApproval(request: unknown, signal?: AbortSignal): Promise<CliApprovalDecision> {
			if (!gateway.decideCliApproval) throw new Error('CLI approval unavailable');
			const decide = gateway.decideCliApproval;
			return withCallerAuth((proof) => decide(request, proof, signal));
		},
		/** Changes the signed-in account's password; ends every other session (§4a.2). */
		async changePassword(
			request: { current_password: string; new_password: string },
			signal?: AbortSignal
		): Promise<void> {
			if (!gateway.changePassword) throw new Error('Password change unavailable');
			const change = gateway.changePassword;
			await withCallerAuth((proof) => {
				if (proof === undefined) throw new Error('Sign in required');
				return change(request, proof, signal);
			});
		},
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
				streams.closeAll();
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
		/** Opens a same-origin SSE event stream using the active session proof without exposing it. */
		subscribeEvents: (subscriptions: readonly Subscription[], handlers: EventStreamHandlers) =>
			streams.subscribe(subscriptions, handlers),
		/** Opens a same-origin GET SSE stream with active proof + 401 rotation without exposing proof. */
		openSseStream: (path: string, signal?: AbortSignal) => {
			if (!path.startsWith('/api/v1/') || path.includes('..') || path.includes('\\')) {
				return Promise.reject(new Error('Invalid API stream path'));
			}
			const token = generation;
			return withCallerAuth(async (proof) => {
				const headers = new Headers({ Accept: 'text/event-stream' });
				if (proof) headers.set(SESSION_PROOF_HEADER, proof);
				const res = await globalThis.fetch(path, {
					method: 'GET',
					headers,
					mode: 'same-origin',
					credentials: 'same-origin',
					redirect: 'error',
					cache: 'no-store',
					signal
				});
				if (!active(token)) {
					await res.body?.cancel().catch(() => {});
					throw new Error('Session changed');
				}
				if (res.status === 401) {
					let sessionReason: 'session_ended' | 'unknown' | undefined;
					try {
						const body = readResponse(await res.json(), readJsonObject);
						if ('error' in body) sessionReason = body.error.session_reason;
					} catch {
						/* Untrusted error payload. */
					}
					throw new SessionRequestError(401, {
						code: 'unauthenticated',
						message: '',
						session_reason: sessionReason
					});
				}
				return res;
			});
		},
		dispose() {
			disposed = true;
			streams.closeAll();
			versionChanged?.();
			generation++;
			recovery++;
			record = undefined;
			channel.removeEventListener('message', received);
		}
	});
}
