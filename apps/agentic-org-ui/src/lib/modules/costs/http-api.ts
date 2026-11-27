import type { OrgApi } from '$lib/modules/organization/api';
import {
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import type { AppError } from '$lib/proto';
import type {
	AgentBudget,
	AgentBudgets,
	BudgetPeriodState,
	OrganizationCost,
	PeriodRequest,
	ResolvedPeriod,
	UsageTotals
} from './types';

import { asNum, asRec, asStr, type Rec } from '$lib/platform/http/decode';

const asNullableNum = (v: unknown): number | null => {
	if (typeof v === 'number' && Number.isFinite(v)) return v;
	return null;
};

function decodeSummaryTotals(raw: unknown): UsageTotals {
	const r = asRec(raw);
	if (!r) {
		throw {
			source: 'api',
			code: 'unavailable',
			message: 'The cost summary could not be read.'
		} satisfies AppError;
	}
	const incomplete = asNum(r.incomplete_records) ?? 0;
	return {
		records: asNum(r.record_count) ?? 0,
		knownCostMicroUsd: asNullableNum(r.known_cost_micro_usd),
		knownTokens: asNullableNum(r.known_tokens),
		cacheReadTokens: asNullableNum(r.cache_read_tokens),
		tokenIncompleteRecords: incomplete,
		costIncompleteRecords: incomplete,
		unpricedRecords: asNum(r.unpriced_records) ?? 0,
		unknownCostRecords: 0,
		includesApiEquivalent: r.includes_api_equivalent === true
	};
}

function decodeBudgetUnit(raw: unknown): BudgetPeriodState['unit'] | undefined {
	if (raw === 'tokens') return 'tokens';
	if (raw === 'micro_usd') return 'usd';
	return undefined;
}

function decodeBudgetFlags(
	r: Rec,
	limit: number
): Partial<Pick<BudgetPeriodState, 'hold' | 'extended' | 'baseLimit' | 'warned'>> {
	const flags: Partial<Pick<BudgetPeriodState, 'hold' | 'extended' | 'baseLimit' | 'warned'>> = {};
	const rawHold = asStr(r.hold_state);
	if (rawHold === 'stop_pending' || rawHold === 'held') flags.hold = rawHold;
	if (r.extension_used === true) {
		flags.extended = true;
		const baseLimit = asNum(r.limit_amount);
		if (baseLimit !== undefined && baseLimit !== limit) flags.baseLimit = baseLimit;
	}
	if (asStr(r.state_summary) === 'warning80') flags.warned = true;
	return flags;
}

function decodeBudgetPeriod(r: Rec): BudgetPeriodState | undefined {
	if (r.state !== 'active' || r.amounts_visible !== true) return undefined;
	const limit = asNum(r.effective_limit) ?? asNum(r.limit_amount);
	const unit = decodeBudgetUnit(asStr(r.unit));
	if (limit === undefined || limit <= 0 || !unit) return undefined;

	return {
		unit,
		limit,
		spent: asNum(r.used_amount) ?? 0,
		complete: asStr(r.usage_status) === 'complete' || r.usage_status === undefined,
		period: {
			key: asStr(r.period_key) ?? '',
			startsAt: asStr(r.starts_at) ?? '',
			endsAt: asStr(r.ends_at) ?? ''
		},
		...decodeBudgetFlags(r, limit)
	};
}

async function loadMemberLookup(
	orgApi: Pick<OrgApi, 'view'>,
	organizationId: string,
	signal?: AbortSignal
): Promise<Map<string, { name: string; former: boolean }>> {
	const memberLookup = new Map<string, { name: string; former: boolean }>();
	try {
		const { snapshot } = await orgApi.view(organizationId, signal);
		for (const m of snapshot.members) {
			memberLookup.set(m.id, {
				name: m.name,
				former: m.agent?.status === 'offboarded'
			});
		}
	} catch (err) {
		if (signal?.aborted) throw err;
		// Fall back to agent IDs if the org snapshot cannot be resolved alongside budgets.
	}
	return memberLookup;
}

function groupAgentBudgets(
	rawBudgets: readonly Rec[],
	memberLookup: ReadonlyMap<string, { name: string; former: boolean }>
): AgentBudget[] {
	const byAgent = new Map<string, AgentBudget>();
	for (const r of rawBudgets) {
		const agentId = asStr(r.agent_id);
		const periodState = decodeBudgetPeriod(r);
		const periodKind = asStr(r.period_kind);
		if (!agentId || !periodState || (periodKind !== 'day' && periodKind !== 'month')) continue;

		const info = memberLookup.get(agentId);
		const entry = byAgent.get(agentId) ?? {
			memberId: agentId,
			name: info?.name ?? agentId,
			...(info?.former ? { former: true } : {})
		};
		if (periodKind === 'day') entry.daily = periodState;
		else entry.monthly = periodState;
		byAgent.set(agentId, entry);
	}
	return [...byAgent.values()].sort((a, b) => a.name.localeCompare(b.name));
}

export interface HttpCostsReadApi {
	organizationCost(
		organizationId: string,
		period: PeriodRequest,
		signal?: AbortSignal
	): Promise<OrganizationCost>;
	agentBudgets(organizationId: string, signal?: AbortSignal): Promise<AgentBudgets>;
}

/**
 * Production HTTP read adapter for `POST /api/v1/organizations/{org}/costs/query`
 * and `GET /api/v1/organizations/{org}/budgets?scope_kind=agent`.
 */
export function createHttpCostsReadApi({
	http,
	orgApi,
	now = () => new Date().toISOString()
}: {
	http: GatewayHttpTransport;
	orgApi: Pick<OrgApi, 'view'>;
	now?: () => string;
}): HttpCostsReadApi {
	return Object.freeze({
		async organizationCost(
			organizationId: string,
			period: PeriodRequest,
			signal?: AbortSignal
		): Promise<OrganizationCost> {
			const path = `/api/v1/organizations/${encodeURIComponent(organizationId)}/costs/query`;
			const body: Record<string, unknown> = {
				period: period.kind,
				time_zone: period.timeZone
			};
			if (period.from) body.from_date = period.from;
			if (period.to) body.to_date = period.to;

			let reply: GatewayReply;
			try {
				reply = await (signal ? http.submit(path, body, signal) : http.submit(path, body));
			} catch (err) {
				if (signal?.aborted) throw err;
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'Organization costs could not be reached.'
				} satisfies AppError;
			}

			const result = unwrapGatewayResult(reply, 'Organization costs could not be loaded.');
			const totals = decodeSummaryTotals(result.summary);
			const summaryRec = asRec(result.summary);
			const endsAt = asStr(result.ends_at) ?? now();
			const startsAt = asStr(result.starts_at);
			const resolvedPeriod: ResolvedPeriod = {
				kind: period.kind,
				...(startsAt ? { startsAt } : {}),
				endsAt,
				timeZone: asStr(result.time_zone) ?? period.timeZone,
				toNow:
					period.kind === 'this_week' || period.kind === 'this_month' || period.kind === 'all_time'
			};

			return {
				period: resolvedPeriod,
				totals,
				byProject: [],
				byConsumer: [],
				byWorkType: [],
				coverage: {
					checkpointAt: asStr(summaryRec?.coverage_as_of) ?? endsAt,
					gaps: []
				}
			};
		},

		async agentBudgets(organizationId: string, signal?: AbortSignal): Promise<AgentBudgets> {
			const path = `/api/v1/organizations/${encodeURIComponent(organizationId)}/budgets?scope_kind=agent`;
			let reply: GatewayReply;
			try {
				reply = await (signal ? http.read(path, signal) : http.read(path));
			} catch (err) {
				if (signal?.aborted) throw err;
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'Agent budgets could not be reached.'
				} satisfies AppError;
			}

			const result = unwrapGatewayResult(reply, 'Agent budgets could not be loaded.');
			if (!Array.isArray(result.budgets)) {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'The agent budgets response could not be read.'
				} satisfies AppError;
			}

			const rawBudgets = result.budgets
				.map(asRec)
				.filter((r): r is Rec => r !== undefined && r.scope_kind === 'agent');
			if (rawBudgets.length === 0) {
				return { zone: { name: null, source: 'hq' }, agents: [] };
			}

			const memberLookup = await loadMemberLookup(orgApi, organizationId, signal);
			const first = rawBudgets[0];
			const zoneName = asStr(first.time_zone) ?? null;
			const zoneSource = asStr(first.time_zone_source) === 'owner_person' ? 'owner_account' : 'hq';

			return {
				zone: { name: zoneName, source: zoneSource },
				agents: groupAgentBudgets(rawBudgets, memberLookup)
			};
		}
	});
}
