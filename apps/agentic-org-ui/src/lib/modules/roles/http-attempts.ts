import type { AppError } from '$lib/proto';
import { NotDispatchedError } from '$lib/platform/http/errors';
import type { GatewayHttpTransport, GatewayReply } from '$lib/platform/http/gateway-errors';
import { readJsonObject, readResponse } from '$lib/platform/http/wire';
import { readOrganizationsList, type OrganizationView } from '$lib/platform/organizations/readers';
import type { Role } from './types';
import {
	deleteIfCurrent,
	SESSION_CHANGED_ERROR,
	toNotDispatchedAppError,
	UNCERTAIN_OUTCOME_MESSAGE,
	type SessionToken
} from './http-mappers';

const MAX_RESOLVED_ATTEMPTS = 64;

type SettledEntry = {
	attemptId?: string;
	committedRevision?: number;
	hasUnresolvedFollowUp?: boolean;
	detailsEvicted?: boolean;
	resolved?: Role;
};

function isInSettledWindow(entry: SettledEntry): boolean {
	return (
		entry.committedRevision !== undefined && !entry.hasUnresolvedFollowUp && !entry.detailsEvicted
	);
}

function isReclaimableEvicted(
	entry: SettledEntry,
	isAttemptRetained?: (attemptId: string) => boolean
): boolean {
	if (!isAttemptRetained || !entry.detailsEvicted || entry.committedRevision === undefined) {
		return false;
	}
	if (entry.hasUnresolvedFollowUp) return false;
	return !entry.attemptId || !isAttemptRetained(entry.attemptId);
}

export function pruneResolved<T extends SettledEntry>(
	map: Map<string, T>,
	activeKeys: ReadonlyMap<string, Promise<unknown>>,
	isAttemptRetained?: (attemptId: string) => boolean
): void {
	let settled = 0;
	for (const [key, entry] of map) {
		if (activeKeys.has(key)) continue;
		if (isInSettledWindow(entry)) {
			settled++;
		} else if (isReclaimableEvicted(entry, isAttemptRetained)) {
			map.delete(key);
		}
	}
	if (settled <= MAX_RESOLVED_ATTEMPTS) return;
	for (const [key, entry] of map) {
		if (settled <= MAX_RESOLVED_ATTEMPTS) break;
		if (activeKeys.has(key) || !isInSettledWindow(entry)) continue;
		entry.resolved = undefined;
		entry.detailsEvicted = true;
		settled--;
		if (isReclaimableEvicted(entry, isAttemptRetained)) {
			map.delete(key);
		}
	}
}

export function createAttemptKey(
	caller: string,
	organizationId: string,
	attemptId: string | undefined
): string | undefined {
	return attemptId ? JSON.stringify(['create', caller, organizationId, attemptId]) : undefined;
}

export function updateAttemptKey(
	caller: string,
	organizationId: string,
	roleId: string,
	expectedRevision: number,
	attemptId: string | undefined
): string | undefined {
	return attemptId
		? JSON.stringify(['update', caller, organizationId, roleId, expectedRevision, attemptId])
		: undefined;
}

export function createRoleGatewayHelper(deps: {
	http: GatewayHttpTransport;
	inFlightKeys: Map<string, Promise<unknown>>;
	sessionScope?: () => string | undefined;
	sessionGeneration?: () => number;
	isAttemptRetained?: (attemptId: string) => boolean;
}) {
	function currentSessionToken(): SessionToken {
		return {
			caller: deps.sessionScope?.() ?? '',
			generation: deps.sessionGeneration?.() ?? 0
		};
	}

	function assertSameSession(session: SessionToken): void {
		const same =
			(deps.sessionGeneration?.() ?? 0) === session.generation &&
			(deps.sessionScope?.() ?? '') === session.caller;
		if (!same) throw SESSION_CHANGED_ERROR;
	}

	async function withAttemptLock<T extends { attemptId?: string; resolved?: Role }, R>(
		attemptKey: string | undefined,
		map: Map<string, T>,
		session: SessionToken,
		fn: () => Promise<R>
	): Promise<R> {
		if (!attemptKey) return fn();
		const prev = deps.inFlightKeys.get(attemptKey);
		let release!: () => void;
		const gate = new Promise<void>((resolve) => {
			release = resolve;
		});
		const tail = prev
			? prev.then(
					() => gate,
					() => gate
				)
			: gate;
		deps.inFlightKeys.set(attemptKey, tail);
		try {
			if (prev) {
				await prev.catch(() => {});
				assertSameSession(session);
			}
			return await fn();
		} finally {
			if (deps.inFlightKeys.get(attemptKey) === tail) deps.inFlightKeys.delete(attemptKey);
			pruneResolved(map, deps.inFlightKeys, deps.isAttemptRetained);
			release();
		}
	}

	async function readGateway(path: string, fallbackMessage: string): Promise<GatewayReply> {
		try {
			return await deps.http.read(path);
		} catch (err) {
			if (err instanceof Error && err.message === 'Session changed') throw SESSION_CHANGED_ERROR;
			throw { source: 'api', code: 'unavailable', message: fallbackMessage } satisfies AppError;
		}
	}

	async function submitGateway(
		path: string,
		body: unknown,
		fallbackMessage: string
	): Promise<GatewayReply> {
		try {
			return await deps.http.submit(path, body);
		} catch (err) {
			if (err instanceof NotDispatchedError) throw err;
			if (err instanceof Error && err.message === 'Session changed') throw SESSION_CHANGED_ERROR;
			throw { source: 'api', code: 'unavailable', message: fallbackMessage } satisfies AppError;
		}
	}

	async function dispatchAttempt<
		T extends { dispatched?: boolean; mayHaveCommitted?: boolean; committedRevision?: number }
	>(
		path: string,
		body: unknown,
		pending: Map<string, T>,
		attempt: T,
		attemptKey: string | undefined,
		session: SessionToken
	): Promise<GatewayReply> {
		assertSameSession(session);
		try {
			const reply = await submitGateway(path, body, UNCERTAIN_OUTCOME_MESSAGE);
			attempt.dispatched = true;
			assertSameSession(session);
			return reply;
		} catch (err) {
			if (err instanceof NotDispatchedError) {
				if (!attempt.dispatched && attempt.committedRevision === undefined) {
					deleteIfCurrent(pending, attemptKey, attempt);
				}
				throw toNotDispatchedAppError(err);
			}
			if (err !== SESSION_CHANGED_ERROR) {
				attempt.dispatched = true;
				attempt.mayHaveCommitted = true;
			}
			assertSameSession(session);
			throw err;
		}
	}

	async function readOrgView(organizationId: string): Promise<OrganizationView | undefined> {
		try {
			const reply = await deps.http.read('/api/v1/organizations');
			if (reply.status !== 200) return undefined;
			const envelope = readResponse(reply.body, readJsonObject);
			if ('error' in envelope) return undefined;
			return readOrganizationsList(envelope.result).organizations.find(
				(o) => o.id === organizationId
			);
		} catch {
			return undefined;
		}
	}

	return {
		currentSessionToken,
		assertSameSession,
		withAttemptLock,
		readGateway,
		dispatchAttempt,
		readOrgView
	};
}
