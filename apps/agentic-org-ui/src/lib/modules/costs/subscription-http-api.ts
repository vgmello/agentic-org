import {
	asApiErrorCode,
	gatewayStatusCode,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import type { AppError } from '$lib/proto';
import type {
	SubscriptionCollectResult,
	SubscriptionCredentialDraft,
	SubscriptionModelUsage,
	SubscriptionUsageAuthSource,
	SubscriptionUsageCredential,
	SubscriptionUsageOverview,
	SubscriptionUsageSnapshot,
	SubscriptionUsageStatus,
	SubscriptionUsageWindow
} from './types';

import { asNum, asRec, asStr, type Rec } from '$lib/platform/http/decode';

const MAX_PAGES = 20;
const STATUSES: ReadonlySet<string> = new Set([
	'ok',
	'auth_required',
	'unavailable',
	'unsupported',
	'never_collected',
	'unknown'
]);
const AUTH_SOURCES: ReadonlySet<string> = new Set([
	'none',
	'dedicated_usage_credential',
	'enrolled_subscription_credential',
	'unknown'
]);

const asNullableNum = (v: unknown): number | null | undefined => {
	if (v === null) return null;
	return typeof v === 'number' && Number.isFinite(v) ? v : undefined;
};

function unwrapReply(reply: GatewayReply, fallbackMessage: string): Rec {
	const root = asRec(reply.body);
	if (reply.status !== 200 || root?.ok === false || root?.error) {
		const err = asRec(root?.error);
		throw {
			source: 'api',
			code: err ? asApiErrorCode(err.code) : gatewayStatusCode(reply.status),
			message: asStr(err?.message) || fallbackMessage
		} satisfies AppError;
	}
	const result = asRec(root?.result);
	if (!result) {
		throw { source: 'api', code: 'unavailable', message: fallbackMessage } satisfies AppError;
	}
	return result;
}

function decodeWindow(raw: unknown): SubscriptionUsageWindow {
	const r = asRec(raw);
	if (!r) return {};
	const win: SubscriptionUsageWindow = {};
	const startsAt = asStr(r.starts_at);
	const endsAt = asStr(r.ends_at);
	const resetsAt = asStr(r.resets_at);
	const windowSeconds = asNum(r.window_seconds);
	if (startsAt !== undefined) win.startsAt = startsAt;
	if (endsAt !== undefined) win.endsAt = endsAt;
	if (resetsAt !== undefined) win.resetsAt = resetsAt;
	if (windowSeconds !== undefined) win.windowSeconds = windowSeconds;
	const util = asNullableNum(r.utilization_bps);
	const reqs = asNullableNum(r.requests);
	const inp = asNullableNum(r.input_tokens);
	const out = asNullableNum(r.output_tokens);
	const cr = asNullableNum(r.cache_read_tokens);
	const cw = asNullableNum(r.cache_write_tokens);
	const tot = asNullableNum(r.total_tokens);
	if (util !== undefined) win.utilizationBps = util;
	if (reqs !== undefined) win.requests = reqs;
	if (inp !== undefined) win.inputTokens = inp;
	if (out !== undefined) win.outputTokens = out;
	if (cr !== undefined) win.cacheReadTokens = cr;
	if (cw !== undefined) win.cacheWriteTokens = cw;
	if (tot !== undefined) win.totalTokens = tot;
	return win;
}

function decodeModelUsage(raw: unknown): SubscriptionModelUsage | undefined {
	const r = asRec(raw);
	const model = asStr(r?.model);
	if (!r || !model) return undefined;
	return {
		model,
		total: decodeWindow(r.total),
		daily: decodeWindow(r.daily),
		weekly: decodeWindow(r.weekly),
		...(r.short_term !== undefined && r.short_term !== null
			? { shortTerm: decodeWindow(r.short_term) }
			: {})
	};
}

function decodeSnapshot(raw: unknown): SubscriptionUsageSnapshot | undefined {
	const r = asRec(raw);
	const connectionId = asStr(r?.connection_id);
	const connectionKey = asStr(r?.connection_key);
	const connectionName = asStr(r?.connection_name);
	const providerType = asStr(r?.provider_type);
	const officeId = asStr(r?.office_id);
	if (!r || !connectionId || !connectionKey || !connectionName || !providerType || !officeId) {
		return undefined;
	}
	const rawStatus = asStr(r.status) ?? 'unknown';
	const status: SubscriptionUsageStatus = STATUSES.has(rawStatus)
		? (rawStatus as SubscriptionUsageStatus)
		: 'unknown';
	const rawAuth = asStr(r.auth_source) ?? 'unknown';
	const authSource: SubscriptionUsageAuthSource = AUTH_SOURCES.has(rawAuth)
		? (rawAuth as SubscriptionUsageAuthSource)
		: 'unknown';
	const models = Array.isArray(r.models)
		? r.models.map(decodeModelUsage).filter((m): m is SubscriptionModelUsage => m !== undefined)
		: [];
	return {
		connectionId,
		connectionKey,
		connectionName,
		providerType,
		...(asStr(r.subscription_identity)
			? { subscriptionIdentity: asStr(r.subscription_identity) }
			: {}),
		officeId,
		status,
		authSource,
		hasUsageCredential: r.has_usage_credential === true,
		hasEnrolledCredential: r.has_enrolled_credential === true,
		...(asStr(r.account_hint) ? { accountHint: asStr(r.account_hint) } : {}),
		...(asStr(r.collected_at) ? { collectedAt: asStr(r.collected_at) } : {}),
		...(asStr(r.last_ok_collected_at) ? { lastOkCollectedAt: asStr(r.last_ok_collected_at) } : {}),
		...(asStr(r.fresh_until) ? { freshUntil: asStr(r.fresh_until) } : {}),
		stale: r.stale === true,
		...(asStr(r.issue_code) ? { issueCode: asStr(r.issue_code) } : {}),
		...(asStr(r.message) ? { message: asStr(r.message) } : {}),
		isApiEquivalentEstimate: false,
		models
	};
}

function decodeCredential(raw: unknown): SubscriptionUsageCredential {
	const r = asRec(raw);
	const connectionId = asStr(r?.connection_id);
	const officeId = asStr(r?.office_id);
	if (!r || !connectionId || !officeId) {
		throw {
			source: 'api',
			code: 'unavailable',
			message: 'Invalid subscription credential response.'
		} satisfies AppError;
	}
	return {
		connectionId,
		officeId,
		configured: r.configured === true,
		...(asStr(r.configured_at) ? { configuredAt: asStr(r.configured_at) } : {}),
		...(asStr(r.expires_at) ? { expiresAt: asStr(r.expires_at) } : {}),
		...(asStr(r.account_hint) ? { accountHint: asStr(r.account_hint) } : {})
	};
}

export interface HttpSubscriptionUsageApi {
	subscriptionUsage(organizationId: string): Promise<SubscriptionUsageOverview>;
	collectSubscriptionUsage(
		organizationId: string,
		options?: { connectionId?: string; timeZone?: string }
	): Promise<SubscriptionCollectResult>;
	setSubscriptionUsageCredential(
		organizationId: string,
		draft: SubscriptionCredentialDraft
	): Promise<SubscriptionUsageCredential>;
	revokeSubscriptionUsageCredential(
		organizationId: string,
		connectionId: string
	): Promise<SubscriptionUsageCredential>;
}

/**
 * Production HTTP adapter for subscription model usage (`TASK-79`, `cost_routes.rs`).
 * Fails closed on gateway errors and preserves nullable provider counters without fabricating zeros.
 */
export function createHttpSubscriptionUsageApi({
	http
}: {
	http: GatewayHttpTransport;
}): HttpSubscriptionUsageApi {
	return Object.freeze({
		async subscriptionUsage(organizationId: string): Promise<SubscriptionUsageOverview> {
			const base = `/api/v1/organizations/${encodeURIComponent(organizationId)}/costs/subscriptions/usage`;
			const snapshots: SubscriptionUsageSnapshot[] = [];
			const totalsByModel: SubscriptionModelUsage[] = [];
			const seenSnapshots = new Set<string>();
			const seenModels = new Set<string>();
			let cursor: string | undefined;
			let totalsCursor: string | undefined;
			let snapshotsDone = false;
			let totalsDone = false;
			let collectedAsOf: string | undefined;
			let partialTotals = false;
			let excludedSnapshotCount = 0;

			for (let page = 0; page < MAX_PAGES; page += 1) {
				const params = new URLSearchParams({ limit: '100' });
				if (!snapshotsDone && cursor) params.set('cursor', cursor);
				if (!totalsDone && totalsCursor) params.set('totals_cursor', totalsCursor);
				let reply: GatewayReply;
				try {
					reply = await http.read(`${base}?${params.toString()}`);
				} catch {
					throw {
						source: 'api',
						code: 'unavailable',
						message: 'Subscription usage could not be reached.'
					} satisfies AppError;
				}
				const result = unwrapReply(reply, 'Subscription usage could not be loaded.');
				if (!snapshotsDone) {
					const pageSnaps = Array.isArray(result.snapshots)
						? result.snapshots
								.map(decodeSnapshot)
								.filter((s): s is SubscriptionUsageSnapshot => s !== undefined)
						: [];
					for (const snap of pageSnaps) {
						if (!seenSnapshots.has(snap.connectionId)) {
							seenSnapshots.add(snap.connectionId);
							snapshots.push(snap);
						}
					}
					const nextCursor = asStr(result.next_cursor);
					if (!nextCursor || nextCursor === cursor) {
						snapshotsDone = true;
						cursor = undefined;
					} else {
						cursor = nextCursor;
					}
				}
				if (!totalsDone) {
					const pageTotals = Array.isArray(result.totals_by_model)
						? result.totals_by_model
								.map(decodeModelUsage)
								.filter((m): m is SubscriptionModelUsage => m !== undefined)
						: [];
					for (const m of pageTotals) {
						if (!seenModels.has(m.model)) {
							seenModels.add(m.model);
							totalsByModel.push(m);
						}
					}
					const nextTotalsCursor = asStr(result.next_totals_cursor);
					if (!nextTotalsCursor || nextTotalsCursor === totalsCursor) {
						totalsDone = true;
						totalsCursor = undefined;
					} else {
						totalsCursor = nextTotalsCursor;
					}
				}
				if (page === 0) {
					collectedAsOf = asStr(result.collected_as_of);
					partialTotals = result.partial_totals === true;
					excludedSnapshotCount = asNum(result.excluded_snapshot_count) ?? 0;
				}
				if (snapshotsDone && totalsDone) break;
			}

			return {
				snapshots,
				totalsByModel,
				...(collectedAsOf ? { collectedAsOf } : {}),
				...(!snapshotsDone && cursor ? { nextCursor: cursor } : {}),
				...(!totalsDone && totalsCursor ? { nextTotalsCursor: totalsCursor } : {}),
				partialTotals,
				excludedSnapshotCount,
				isApiEquivalentEstimate: false
			};
		},

		async collectSubscriptionUsage(
			organizationId: string,
			options?: { connectionId?: string; timeZone?: string }
		): Promise<SubscriptionCollectResult> {
			const path = `/api/v1/organizations/${encodeURIComponent(organizationId)}/costs/subscriptions/usage/collect`;
			const body: Record<string, string> = {};
			if (options?.connectionId) body.connection_id = options.connectionId;
			if (options?.timeZone) body.time_zone = options.timeZone;
			let reply: GatewayReply;
			try {
				reply = await http.submit(path, body);
			} catch {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'Could not reach the office to collect subscription usage.'
				} satisfies AppError;
			}
			const result = unwrapReply(reply, 'Could not collect subscription usage.');
			const snapshots = Array.isArray(result.snapshots)
				? result.snapshots
						.map(decodeSnapshot)
						.filter((s): s is SubscriptionUsageSnapshot => s !== undefined)
				: [];
			const deferred = Array.isArray(result.deferred_connection_ids)
				? result.deferred_connection_ids.filter((id): id is string => typeof id === 'string')
				: undefined;
			return {
				snapshots,
				collectedAt: asStr(result.collected_at) ?? new Date().toISOString(),
				...(deferred && deferred.length > 0 ? { deferredConnectionIds: deferred } : {})
			};
		},

		async setSubscriptionUsageCredential(
			organizationId: string,
			draft: SubscriptionCredentialDraft
		): Promise<SubscriptionUsageCredential> {
			const path = `/api/v1/organizations/${encodeURIComponent(organizationId)}/costs/subscriptions/usage/credentials`;
			const body: Record<string, string> = {
				connection_id: draft.connectionId,
				secret: draft.secret
			};
			if (draft.expiresAt) body.expires_at = draft.expiresAt;
			if (draft.accountHint) body.account_hint = draft.accountHint;
			if (draft.expectedOrigin) body.expected_origin = draft.expectedOrigin;
			let reply: GatewayReply;
			try {
				reply = await http.submit(path, body);
			} catch {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'Could not reach the office to save the usage credential.'
				} satisfies AppError;
			}
			const result = unwrapReply(reply, 'Could not save the usage credential.');
			return decodeCredential(result.credential);
		},

		async revokeSubscriptionUsageCredential(
			organizationId: string,
			connectionId: string
		): Promise<SubscriptionUsageCredential> {
			const path = `/api/v1/organizations/${encodeURIComponent(organizationId)}/costs/subscriptions/usage/credentials/revoke`;
			let reply: GatewayReply;
			try {
				reply = await http.submit(path, { connection_id: connectionId });
			} catch {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'Could not reach the office to revoke the usage credential.'
				} satisfies AppError;
			}
			const result = unwrapReply(reply, 'Could not revoke the usage credential.');
			return decodeCredential(result.credential);
		}
	});
}
