/**
 * The organization routes of web API v1 (docs/design/web-api-v1.md): create, the progress
 * lookup of a create, the list and the instance's name. Transport, session and the clock are
 * supplied by the caller; replies are classified here and never trusted undecoded.
 */
import { NotDispatchedError } from '../http/errors';
import { readInstance, readOperationId, readResponse } from '../http/wire';
import {
	readErrorBody,
	readOrganizationsList,
	readOutcomeBody,
	type CreateRefusal,
	type InstanceName,
	type OrganizationList,
	type OrganizationView
} from './readers';

type Reply = { status: number; body: unknown };
/** The session's transport: GET, and one POST that is never re-sent on its own. */
export interface OrganizationsHttp {
	read(path: string, signal?: AbortSignal): Promise<Reply>;
	submit(path: string, body: unknown, signal?: AbortSignal): Promise<Reply>;
}
export interface CreateParams {
	name: string;
	slug: string;
	/** The remote organization repository's URL (https:// or ssh://). */
	repository: string;
	stateBranch: string;
}

export type CreateReply =
	/** The host stored the create and is carrying it out: follow it through the lookup. */
	| { kind: 'accepted' }
	| { kind: 'completed'; organization: OrganizationView }
	/** A stored or pre-dispatch refusal: nothing was created under this operation. */
	| { kind: 'rejected'; error: CreateRefusal }
	/** 504 or a lost or unreadable reply: the outcome is not known. Look the same ID up. */
	| { kind: 'unknown' }
	/** 503: the host could not be asked. The same ID may be sent again. */
	| { kind: 'unavailable'; code: string }
	| { kind: 'expired' }
	| { kind: 'session' }
	/** Proven not to have left this browser. */
	| { kind: 'not_sent' };

export type LookupReply =
	| { kind: 'accepted' }
	| { kind: 'completed'; organization: OrganizationView }
	| { kind: 'rejected'; error: CreateRefusal }
	/** HQ has no record of the operation (or it belongs to another account). */
	| { kind: 'missing' }
	| { kind: 'unreachable' }
	| { kind: 'expired' }
	| { kind: 'session' };

export type ListReply =
	{ kind: 'listed'; list: OrganizationList } | { kind: 'unreachable' } | { kind: 'session' };

/** The lookup's waits: 1 s, then 1.5 times longer each time, to at most 10 s. */
export const POLL_FIRST_MS = 1_000;
export const POLL_FACTOR = 1.5;
export const POLL_MAX_MS = 10_000;
export const nextPollDelay = (previousMs?: number): number =>
	previousMs === undefined
		? POLL_FIRST_MS
		: Math.min(POLL_MAX_MS, Math.round(previousMs * POLL_FACTOR));
/** Consecutive lookups without an answer before the page says HQ can't be reached. */
export const UNREACHABLE_LIMIT = 3;

const errorCode = (body: unknown): string => {
	try {
		return readErrorBody(body).code;
	} catch {
		return 'unknown';
	}
};

export function createOrganizationsApi({
	http,
	instanceId,
	newId
}: {
	http: OrganizationsHttp;
	/** The signed-in session's instance; requests are scoped to it. */
	instanceId: () => string;
	/** A canonical lowercase UUIDv7 from the server-corrected clock. */
	newId: () => string;
}) {
	async function lookup(operationId: string, signal?: AbortSignal): Promise<LookupReply> {
		const id = readOperationId(operationId);
		let reply: Reply;
		try {
			reply = await http.read(
				`/api/v1/operations/host/${id}?instance_id=${encodeURIComponent(instanceId())}`,
				signal
			);
		} catch {
			return { kind: 'unreachable' };
		}
		if (reply.status === 401) return { kind: 'session' };
		if (reply.status === 404) return { kind: 'missing' };
		if (reply.status === 410) return { kind: 'expired' };
		if (reply.status !== 200) return { kind: 'unreachable' };
		try {
			const body = readResponse(reply.body, (result) => readOutcomeBody(result, id));
			if ('error' in body) return { kind: 'unreachable' };
			const outcome = body.result;
			if (outcome.outcome === 'accepted') return { kind: 'accepted' };
			if (outcome.outcome === 'completed')
				return { kind: 'completed', organization: outcome.organization };
			if (outcome.outcome === 'rejected') return { kind: 'rejected', error: outcome.error };
			return { kind: 'unreachable' };
		} catch {
			return { kind: 'unreachable' };
		}
	}
	return Object.freeze({
		newOperationId: (): string => readOperationId(newId()),
		/** One POST. The caller owns the operation ID and decides whether to send it again. */
		async create(
			operationId: string,
			params: CreateParams,
			signal?: AbortSignal
		): Promise<CreateReply> {
			const id = readOperationId(operationId);
			let body: unknown;
			try {
				body = {
					operation_id: id,
					scope: { kind: 'instance', instance_id: instanceId() },
					params: {
						name: params.name,
						slug: params.slug,
						organization_repository: { kind: 'remote', remote_url: params.repository },
						state_branch: params.stateBranch
					}
				};
			} catch {
				// The request could not be built (the session went away), so nothing left this browser.
				return { kind: 'not_sent' };
			}
			let reply: Reply;
			try {
				reply = await http.submit('/api/v1/organizations', body, signal);
			} catch (error) {
				return error instanceof NotDispatchedError ? { kind: 'not_sent' } : { kind: 'unknown' };
			}
			const { status } = reply;
			if (status === 401) return { kind: 'session' };
			if (status === 410) return { kind: 'expired' };
			if (status === 503) return { kind: 'unavailable', code: errorCode(reply.body) };
			try {
				if (status === 200 || status === 202 || status === 504) {
					const outcome = readOutcomeBody(reply.body, id);
					if (status === 202 && outcome.outcome === 'accepted') return { kind: 'accepted' };
					if (status === 200 && outcome.outcome === 'completed')
						return { kind: 'completed', organization: outcome.organization };
					return { kind: 'unknown' };
				}
				if (status === 400 || status === 403 || status === 409) {
					const fields = reply.body as Record<string, unknown> | null;
					// The host's refusal is an outcome; the web process's own is a plain `{error}`.
					if (fields && typeof fields === 'object' && Object.hasOwn(fields, 'outcome')) {
						const outcome = readOutcomeBody(reply.body, id);
						return outcome.outcome === 'rejected'
							? { kind: 'rejected', error: outcome.error }
							: { kind: 'unknown' };
					}
					return { kind: 'rejected', error: readErrorBody(reply.body) };
				}
			} catch {
				// An unreadable reply after dispatch says nothing about whether it was stored.
			}
			return { kind: 'unknown' };
		},
		lookup,
		async list(signal?: AbortSignal): Promise<ListReply> {
			try {
				const reply = await http.read('/api/v1/organizations', signal);
				if (reply.status === 401) return { kind: 'session' };
				if (reply.status !== 200) return { kind: 'unreachable' };
				const body = readResponse(reply.body, readOrganizationsList);
				return 'result' in body ? { kind: 'listed', list: body.result } : { kind: 'unreachable' };
			} catch {
				return { kind: 'unreachable' };
			}
		},
		/** The instance that will store a new organization; undefined when it can't be read. */
		async instance(signal?: AbortSignal): Promise<InstanceName | undefined> {
			try {
				const reply = await http.read('/api/v1/instance', signal);
				if (reply.status !== 200) return undefined;
				const body = readResponse(reply.body, readInstance);
				if (!('result' in body)) return undefined;
				const { instance_id, name, server_version, server_time_ms } = body.result;
				return { instance_id, name, server_version, server_time_ms };
			} catch {
				return undefined;
			}
		},
		/**
		 * Follows one operation until it is no longer in progress: the stored outcome, no record,
		 * too old, a session that ended, or HQ unreachable UNREACHABLE_LIMIT times in a row.
		 * It only ever reads; it never sends the create again.
		 */
		async follow(
			operationId: string,
			{
				sleep,
				signal,
				waiting
			}: {
				sleep: (ms: number, signal?: AbortSignal) => Promise<void>;
				signal?: AbortSignal;
				/** Called before each wait, with the answer that keeps it waiting. */
				waiting?: (reply: 'accepted' | 'unreachable') => void;
			}
		): Promise<Exclude<LookupReply, { kind: 'accepted' }> | { kind: 'stopped' }> {
			let delay: number | undefined;
			let unreachable = 0;
			for (;;) {
				if (signal?.aborted) return { kind: 'stopped' };
				const reply = await lookup(operationId, signal);
				if (signal?.aborted) return { kind: 'stopped' };
				if (reply.kind === 'unreachable') {
					if (++unreachable >= UNREACHABLE_LIMIT) return reply;
				} else if (reply.kind !== 'accepted') return reply;
				else unreachable = 0;
				waiting?.(reply.kind);
				delay = nextPollDelay(delay);
				try {
					await sleep(delay, signal);
				} catch {
					return { kind: 'stopped' };
				}
			}
		}
	});
}
export type OrganizationsApi = ReturnType<typeof createOrganizationsApi>;
