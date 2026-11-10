import { NotDispatchedError } from './errors';

/** Local adapter contracts, NOT the pending /api/v1 wire DTOs or route names. */
export type OperationKind = 'org' | 'work' | 'host' | 'msg';
export interface OperationScope {
	readonly instanceId: string;
	readonly accountId: string;
	readonly organizationId?: string;
	readonly officeId?: string;
}
export interface Session {
	readonly instanceId: string;
	readonly accountId: string;
	readonly proof: string;
}
export interface OperationRef {
	readonly kind: OperationKind;
	readonly id: string;
	readonly scope: OperationScope;
}
export interface OperationRequest extends OperationRef {
	readonly method: string;
	readonly params: Readonly<Record<string, unknown>>;
}
/** Adapter-local results; B1-02 owns the actual HTTP field names and wire decoders. */
export type AdapterOutcome = { readonly operationId: string } & (
	| { readonly status: 'accepted' | 'unknown' }
	| { readonly status: 'completed'; readonly result: unknown }
	| { readonly status: 'rejected'; readonly error: { readonly code: string; readonly message: string } }
);
/** A missing ledger entry is `missing`; a stored rule-level not_found remains a rejected outcome. */
export type AdapterLookup = AdapterOutcome | { readonly operationId: string; readonly status: 'missing' | 'expired' };
/**
 * B1-02's adapter maps validated wire data to AdapterOutcome/AdapterLookup. Inputs remain
 * unknown at this runtime boundary so a stale or malformed adapter result cannot imply success.
 */
export interface OperationTransport {
	submit(request: OperationRequest, session: Session): Promise<unknown>;
	lookup(operation: OperationRef, session: Session): Promise<unknown>;
}
export interface Refusal {
	readonly code: string;
	readonly message: string;
	readonly category: 'conflict' | 'authorization' | 'version' | 'clock' | 'unavailable' | 'other';
}
export type OperationState<T> =
	| { readonly status: 'not_submitted'; readonly error?: Refusal }
	| { readonly status: 'accepted' }
	| { readonly status: 'completed'; readonly result: T }
	| { readonly status: 'rejected'; readonly error: Refusal }
	| { readonly status: 'unresolved'; readonly reason: 'unknown' | 'missing' | 'transport' | 'invalid_response' | 'session_changed' | 'authorization' | 'clock' }
	| { readonly status: 'expired' }
	| { readonly status: 'unsupported' };
export interface Operation<T> extends OperationRef {
	readonly state: OperationState<T>;
	/** Sends once after possible dispatch. A proven local refusal permits explicit retry; unknown outcomes only use lookup. */
	submit(): Promise<OperationState<T>>;
	lookup(): Promise<OperationState<T>>;
}

const ID_FIELD = { org: 'command_id', work: 'change_id', host: 'operation_id', msg: 'id' } as const;
const ULID = /^[0-7][0-9A-HJKMNP-TV-Z]{25}$/;
const object = (value: unknown): value is Record<string, unknown> =>
	!!value && typeof value === 'object' && !Array.isArray(value);
function terminal<T>(state: OperationState<T>): boolean {
	switch (state.status) {
		case 'completed': case 'rejected': case 'expired': return true;
		case 'not_submitted': case 'accepted': case 'unresolved': case 'unsupported': return false;
		default: { const unreachable: never = state; return unreachable; }
	}
}
const sameAccount = (session: Session | undefined, scope: OperationScope) =>
	session?.instanceId === scope.instanceId && session.accountId === scope.accountId;

/** Capture an immutable JSON intent before the first dispatch, including expected versions. */
function capture(value: unknown, depth = 0): unknown {
	if (depth > 50) throw new Error('Operation params are too deeply nested');
	if (value === null || typeof value === 'string' || typeof value === 'boolean') return value;
	if (typeof value === 'number' && Number.isFinite(value)) return value;
	if (Array.isArray(value)) return Object.freeze(value.map((item) => capture(item, depth + 1)));
	if (object(value) && [Object.prototype, null].includes(Object.getPrototypeOf(value)))
		return Object.freeze(Object.fromEntries(Object.entries(value).filter(([, v]) => v !== undefined).map(([k, v]) => [k, capture(v, depth + 1)])));
	throw new Error('Operation params must be JSON values');
}

function refusal(value: unknown): Refusal {
	if (!object(value) || typeof value.code !== 'string' || typeof value.message !== 'string') throw new Error('Invalid refusal');
	const categories: Record<string, Refusal['category']> = {
		operation_reused: 'conflict', conflict: 'conflict', not_authorized: 'authorization',
		unsupported_version: 'version', clock_skew: 'clock', unavailable: 'unavailable', not_ready: 'unavailable'
	};
	return Object.freeze({ code: value.code, message: value.message, category: Object.hasOwn(categories, value.code) ? categories[value.code] : 'other' });
}

/**
 * No routing, browser storage, automatic retry, or domain reducer lives here. A handle owns one
 * intent and deduplicates concurrent callers. Submitted work outlives this in-memory view.
 */
export function createOperationClient({ transport, session, clock }: {
	transport: OperationTransport;
	session: () => Session | undefined;
	clock: { newId(): string };
}) {
	const readSession = () => { try { return session(); } catch { return undefined; } };
	function handle<T>(ref: OperationRef, decode: (result: unknown) => T, request?: OperationRequest): Operation<T> {
		if (!ULID.test(ref.id)) throw new Error('Invalid operation ID');
		const scope = Object.freeze({ ...ref.scope });
		if (!scope.instanceId || !scope.accountId) throw new Error('Operation scope requires instance and account');
		const reference = Object.freeze({ ...ref, scope });
		let state: OperationState<T> = Object.freeze(request ? { status: 'not_submitted' } : { status: 'unresolved', reason: 'unknown' });
		let sent = !request;
		let pending: Promise<OperationState<T>> | undefined;
		const update = (next: OperationState<T>) => (state = Object.freeze(next));
		function readReply(reply: unknown, lookup: boolean): OperationState<T> {
			if (!object(reply) || reply.operationId !== reference.id || typeof reply.status !== 'string') throw new Error('Invalid operation reply');
			const result = Object.hasOwn(reply, 'result');
			const error = Object.hasOwn(reply, 'error');
			switch (reply.status) {
				case 'completed':
					if (!result || error) throw new Error('Invalid completed result');
					return { status: 'completed', result: decode(reply.result) };
				case 'rejected':
					if (!error || result) throw new Error('Invalid rejection');
					{
						const failure = refusal(reply.error);
						// X-02 admission/auth failures are never stored business rejections.
						if (failure.category === 'authorization') return { status: 'unresolved', reason: 'authorization' };
						if (failure.category === 'version') return { status: 'unsupported' };
						if (failure.category === 'clock') return { status: 'unresolved', reason: 'clock' };
						if (failure.code === 'unknown') return { status: 'unresolved', reason: 'unknown' };
						if (failure.code === 'expired') return { status: 'expired' };
						return { status: 'rejected', error: failure };
					}
				case 'accepted':
				case 'unknown':
				case 'missing':
				case 'expired':
					if (result || error || (!lookup && ['missing', 'expired'].includes(reply.status))) throw new Error('Invalid operation state');
					if (reply.status === 'accepted') return { status: 'accepted' };
					if (reply.status === 'expired') return { status: 'expired' };
					return { status: 'unresolved', reason: reply.status === 'missing' ? 'missing' : 'unknown' };
				default: return { status: 'unsupported' };
			}
		}
		function run(lookup: boolean): Promise<OperationState<T>> {
			const current = readSession();
			if (typeof current?.proof !== 'string' || !current.proof || !sameAccount(current, scope)) return Promise.reject(new Error('Reauthenticate the original account before continuing this operation'));
			if (pending) return pending;
			if (terminal(state) || (!lookup && sent) || (lookup && !sent)) return Promise.resolve(state);
			// A new session may resolve the same account's old ID, but never implicitly resubmits it.
			const auth = Object.freeze({ ...current });
			if (!lookup) sent = true;
			pending = (async () => {
				let reply: unknown;
				try {
					reply = await (lookup ? transport.lookup(reference, auth) : transport.submit(request!, auth));
				} catch (error) {
					if (!lookup && error instanceof NotDispatchedError) {
						sent = false;
						return update({ status: 'not_submitted', error: Object.freeze({ code: error.code, message: error.message, category: 'other' }) });
					}
					return update({ status: 'unresolved', reason: 'transport' });
				}
				const latest = readSession();
				if (!sameAccount(latest, scope) || latest?.proof !== auth.proof)
					return update({ status: 'unresolved', reason: 'session_changed' });
				try { return update(readReply(reply, lookup)); }
				catch { return update({ status: 'unresolved', reason: 'invalid_response' }); }
			})().finally(() => { pending = undefined; });
			return pending;
		}
		return Object.freeze({
			...reference,
			get state(): OperationState<T> {
				const current = readSession();
				if (state.status !== 'not_submitted' && (!current?.proof || !sameAccount(current, scope)))
					return Object.freeze({ status: 'unresolved', reason: 'session_changed' });
				return state;
			},
			submit: () => run(false), lookup: () => run(true)
		});
	}
	return {
		prepare<T>({ kind, method, scope, params, decode }: {
			kind: OperationKind; method: string; scope: OperationScope; params: Record<string, unknown>; decode: (result: unknown) => T;
		}): Operation<T> {
			if (!method) throw new Error('An operation method is required');
			if (!Object.hasOwn(ID_FIELD, kind)) throw new Error('Invalid operation kind');
			const field = ID_FIELD[kind];
			const existing = params[field];
			if (existing !== undefined && typeof existing !== 'string') throw new Error('Invalid operation ID');
			const id = existing ?? clock.newId();
			const captured = capture({ ...params, [field]: id }) as Readonly<Record<string, unknown>>;
			const ref = { kind, id, scope: Object.freeze({ ...scope }) };
			return handle(ref, decode, Object.freeze({ ...ref, method, params: captured }));
		},
		/** Reconnect with a known ID without retaining or recreating a request body/outbox. */
		resume<T>(ref: OperationRef, decode: (result: unknown) => T): Operation<T> { return handle(ref, decode); }
	};
}
