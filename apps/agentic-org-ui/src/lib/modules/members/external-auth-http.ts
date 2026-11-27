// Live HTTP adapter for BYOA external-agent sign-in approvals & lease management (`TASK-249`).
// All authenticated calls flow through `appSession.controller()`; signed-out calls fail closed.
import { appSession } from '$lib/platform/auth/app-session.svelte';
import {
	asArr,
	asBool,
	asNonEmptyStr,
	asNum,
	asRec,
	asStr,
	optStrs,
	reqStrs,
	type Rec
} from '$lib/platform/http/decode';
import type { GatewayHttpTransport, GatewayReply } from '$lib/platform/http/gateway-errors';
import { unavailableCall } from '$lib/platform/unavailable-api';
import {
	decodePendingExternalAuth,
	decodePendingList,
	normalizeHandle
} from '$lib/platform/workspace/external-auth';
import { MAX_LEASE_TTL_SECONDS, MIN_LEASE_TTL_SECONDS } from './external-auth-present';
import type {
	ExternalAuthApi,
	ExternalAuthError,
	ExternalAuthErrorCode,
	ExternalHistoryEntry,
	ExternalSessionView,
	MemberExternalSessionsView,
	PendingExternalAuthView
} from './external-auth-types';

export { decodePendingExternalAuth, decodePendingList };

const ERROR_CODES: ReadonlySet<string> = new Set([
	'invalid_request',
	'not_authorized',
	'owner_protected',
	'not_found',
	'conflict',
	'expired',
	'rate_limited',
	'unavailable',
	'unknown'
]);

function statusToErrorCode(status: number): ExternalAuthErrorCode {
	if (status === 401 || status === 403) return 'not_authorized';
	if (status === 400 || status === 422) return 'invalid_request';
	if (status === 404) return 'not_found';
	if (status === 409) return 'conflict';
	if (status === 410) return 'expired';
	if (status === 429) return 'rate_limited';
	return 'unavailable';
}

export function isExternalAuthError(value: unknown): value is ExternalAuthError {
	const rec = asRec(value);
	return Boolean(
		rec &&
		rec.source === 'api' &&
		typeof rec.code === 'string' &&
		ERROR_CODES.has(rec.code) &&
		typeof rec.message === 'string'
	);
}

function errorFromRecord(rec: Rec, fallback: string): ExternalAuthError {
	const code: ExternalAuthErrorCode =
		typeof rec.code === 'string' && ERROR_CODES.has(rec.code)
			? (rec.code as ExternalAuthErrorCode)
			: 'unknown';
	const retry = asNum(rec.retry_after_ms);
	const status = asNum(rec.status);
	return {
		source: 'api',
		code,
		message: asNonEmptyStr(rec.message) ?? fallback,
		...(status !== undefined ? { status } : {}),
		...(retry !== undefined && retry > 0 ? { retry_after_ms: retry } : {})
	};
}

export function asExternalAuthError(
	err: unknown,
	fallback = 'External sign-in request failed.'
): ExternalAuthError {
	if (isExternalAuthError(err)) return err;
	const rec = asRec(err);
	if (rec && typeof rec.message === 'string') return errorFromRecord(rec, fallback);
	return {
		source: 'api',
		code: 'unavailable',
		message: err instanceof Error && err.message ? err.message : fallback
	};
}

export function decodeExternalAuthError(
	reply: GatewayReply,
	fallbackMessage: string
): ExternalAuthError {
	const err = asRec(asRec(reply.body)?.error);
	const rawCode = err?.code;
	const code: ExternalAuthErrorCode =
		typeof rawCode === 'string' && ERROR_CODES.has(rawCode)
			? (rawCode as ExternalAuthErrorCode)
			: statusToErrorCode(reply.status);
	const retryAfterMs = asNum(err?.retry_after_ms);
	return {
		source: 'api',
		code,
		status: reply.status,
		message: asNonEmptyStr(err?.message) ?? fallbackMessage,
		...(retryAfterMs !== undefined && retryAfterMs > 0 ? { retry_after_ms: retryAfterMs } : {})
	};
}

function unwrapExternalAuthResult(reply: GatewayReply, fallbackMessage: string): Rec {
	const root = asRec(reply.body);
	if (reply.status !== 200 || root?.error) {
		throw decodeExternalAuthError(reply, fallbackMessage);
	}
	const result = asRec(root?.result);
	if (!result) {
		throw {
			source: 'api',
			code: 'unavailable',
			status: reply.status,
			message: fallbackMessage
		} satisfies ExternalAuthError;
	}
	return result;
}

export function decodeExternalSession(raw: unknown): ExternalSessionView | undefined {
	const r = asRec(raw);
	if (!r) return undefined;
	const req = reqStrs(r, [
		'session_id',
		'employee_id',
		'workspace_label',
		'pubkey_fingerprint',
		'approved_by',
		'approved_at',
		'expires_at'
	]);
	if (!req) return undefined;
	const handle = normalizeHandle(asStr(r.handle));
	const opt = optStrs(r, ['revoked_at', 'superseded_session_id']);
	return {
		...req,
		...opt,
		...(handle ? { handle } : {}),
		active: asBool(r.active) ?? !opt.revoked_at,
		took_over_managed: asBool(r.took_over_managed) ?? false
	};
}

function decodeHistoryList(raw: unknown): ExternalHistoryEntry[] {
	const seen = new Map<string, number>();
	const out: ExternalHistoryEntry[] = [];
	for (const entry of asArr(raw)) {
		const r = asRec(entry);
		if (!r) continue;
		const req = reqStrs(r, ['at', 'event']);
		if (!req) continue;
		const opt = optStrs(r, [
			'session_id',
			'actor_id',
			'detail',
			'hostname',
			'workspace_path',
			'workspace_label',
			'pubkey_fingerprint'
		]);
		const baseKey = [
			req.at,
			req.event,
			opt.session_id ?? '',
			opt.actor_id ?? '',
			opt.pubkey_fingerprint ?? '',
			opt.workspace_label ?? ''
		].join('|');
		const count = (seen.get(baseKey) ?? 0) + 1;
		seen.set(baseKey, count);
		out.push({ id: count === 1 ? baseKey : `${baseKey}#${count}`, ...req, ...opt });
	}
	return out;
}

const unreachableError = (message: string): ExternalAuthError => ({
	source: 'api',
	code: 'unavailable',
	message
});

function clampTtl(ttl: number | undefined): number | undefined {
	if (ttl === undefined || !Number.isFinite(ttl)) return undefined;
	return Math.max(MIN_LEASE_TTL_SECONDS, Math.min(MAX_LEASE_TTL_SECONDS, Math.trunc(ttl)));
}

function requireSessionResult(
	reply: GatewayReply,
	refusedMsg: string,
	badShapeMsg: string
): ExternalSessionView {
	const session = decodeExternalSession(unwrapExternalAuthResult(reply, refusedMsg));
	if (!session) throw unreachableError(badShapeMsg);
	return session;
}

export interface HttpExternalAuthApiOptions {
	http: GatewayHttpTransport;
	newCommandId?: () => string;
}

export function createHttpExternalAuthApi({
	http,
	newCommandId
}: HttpExternalAuthApiOptions): ExternalAuthApi {
	const orgBase = (orgId: string) => `/api/v1/organizations/${encodeURIComponent(orgId)}`;
	const memberBase = (orgId: string, memberId: string) =>
		`${orgBase(orgId)}/members/${encodeURIComponent(memberId)}/external-sessions`;

	async function readOrThrow(path: string, fallback: string, signal?: AbortSignal) {
		try {
			return await (signal ? http.read(path, signal) : http.read(path));
		} catch (err) {
			if (signal?.aborted) throw err;
			throw unreachableError(fallback);
		}
	}

	async function submitOrThrow(
		path: string,
		body: Record<string, unknown>,
		fallback: string,
		signal?: AbortSignal
	) {
		try {
			return await (signal ? http.submit(path, body, signal) : http.submit(path, body));
		} catch (err) {
			if (signal?.aborted) throw err;
			throw unreachableError(fallback);
		}
	}

	return Object.freeze<ExternalAuthApi>({
		async pending(organizationId, options = {}): Promise<PendingExternalAuthView[]> {
			const query = options.memberId ? `?member_id=${encodeURIComponent(options.memberId)}` : '';
			const reply = await readOrThrow(
				`${orgBase(organizationId)}/external-auth/pending${query}`,
				'Pending external sign-in requests could not be reached.',
				options.signal
			);
			const result = unwrapExternalAuthResult(
				reply,
				'Pending external sign-in requests could not be loaded.'
			);
			return decodePendingList(result.pending);
		},

		async memberSessions(
			organizationId,
			memberId,
			options = {}
		): Promise<MemberExternalSessionsView> {
			const limit =
				options.limit !== undefined && Number.isFinite(options.limit)
					? Math.max(1, Math.min(200, Math.trunc(options.limit)))
					: undefined;
			const reply = await readOrThrow(
				`${memberBase(organizationId, memberId)}${limit !== undefined ? `?limit=${limit}` : ''}`,
				'External sessions could not be reached.',
				options.signal
			);
			const result = unwrapExternalAuthResult(reply, 'External sessions could not be loaded.');
			const active = result.active !== undefined ? decodeExternalSession(result.active) : undefined;
			return {
				employee_id: asNonEmptyStr(result.employee_id) ?? memberId,
				...(active ? { active } : {}),
				pending: decodePendingList(result.pending),
				history: decodeHistoryList(result.history)
			};
		},

		async approve(organizationId, input, signal) {
			const commandId = input.command_id ?? newCommandId?.();
			const ttlSeconds = clampTtl(input.ttl_seconds);
			const body: Record<string, unknown> = {
				code: input.code,
				...(commandId ? { command_id: commandId } : {}),
				...(ttlSeconds !== undefined ? { ttl_seconds: ttlSeconds } : {}),
				...(input.takeover !== undefined ? { takeover: input.takeover } : {})
			};
			const reply = await submitOrThrow(
				`${orgBase(organizationId)}/external-auth/approve`,
				body,
				'Could not reach the gateway to approve external sign-in.',
				signal
			);
			return requireSessionResult(
				reply,
				'External sign-in approval was refused.',
				'Unexpected approval response from the gateway.'
			);
		},

		async renew(organizationId, memberId, input = {}, signal) {
			const commandId = input.command_id ?? newCommandId?.();
			const ttlSeconds = clampTtl(input.ttl_seconds);
			const body: Record<string, unknown> = {
				...(commandId ? { command_id: commandId } : {}),
				...(input.session_id ? { session_id: input.session_id } : {}),
				...(ttlSeconds !== undefined ? { ttl_seconds: ttlSeconds } : {})
			};
			const reply = await submitOrThrow(
				`${memberBase(organizationId, memberId)}/renew`,
				body,
				'Could not reach the gateway to renew the external lease.',
				signal
			);
			return requireSessionResult(
				reply,
				'External lease renewal was refused.',
				'Unexpected renewal response from the gateway.'
			);
		},

		async revoke(organizationId, memberId, input = {}, signal) {
			const commandId = input.command_id ?? newCommandId?.();
			const body: Record<string, unknown> = {
				...(commandId ? { command_id: commandId } : {}),
				...(input.session_id ? { session_id: input.session_id } : {})
			};
			const reply = await submitOrThrow(
				`${memberBase(organizationId, memberId)}/revoke`,
				body,
				'Could not reach the gateway to revoke the external lease.',
				signal
			);
			const result = unwrapExternalAuthResult(reply, 'External lease revocation was refused.');
			const session_id = asNonEmptyStr(result.session_id);
			return {
				employee_id: asNonEmptyStr(result.employee_id) ?? memberId,
				revoked: asBool(result.revoked) ?? false,
				...(session_id ? { session_id } : {})
			};
		}
	});
}

const isSignedIn = () => appSession.state.kind === 'signed-in';

const gatewayExternalAuthApi = createHttpExternalAuthApi({
	http: {
		read: (path, signal) => appSession.controller().read(path, signal),
		submit: (path, body, signal) => appSession.controller().submit(path, body, signal)
	},
	newCommandId: () => appSession.newOperationId()
});

/** Production BYOA external-auth API backed by live `/api/v1` endpoints; fails closed when signed out. */
export const externalAuthApi: ExternalAuthApi = Object.freeze<ExternalAuthApi>({
	pending: (orgId, options) =>
		isSignedIn() ? gatewayExternalAuthApi.pending(orgId, options) : unavailableCall(),
	memberSessions: (orgId, memberId, options) =>
		isSignedIn()
			? gatewayExternalAuthApi.memberSessions(orgId, memberId, options)
			: unavailableCall(),
	approve: (orgId, input, signal) =>
		isSignedIn() ? gatewayExternalAuthApi.approve(orgId, input, signal) : unavailableCall(),
	renew: (orgId, memberId, input, signal) =>
		isSignedIn() ? gatewayExternalAuthApi.renew(orgId, memberId, input, signal) : unavailableCall(),
	revoke: (orgId, memberId, input, signal) =>
		isSignedIn() ? gatewayExternalAuthApi.revoke(orgId, memberId, input, signal) : unavailableCall()
});
