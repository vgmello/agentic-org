import type { OrgApi } from '$lib/modules/organization/api';
import {
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import { asRec, asStr } from '$lib/platform/http/decode';
import type { AppError } from '$lib/proto';
import type { HttpCostsReadApi } from './http-api';
import {
	buildMemberBreakdowns,
	buildOrgBreakdowns,
	matchesBreakdownKey,
	matchesProjectKey
} from './http-full-breakdowns';
import {
	decodeBudgetState,
	decodePriceVersion,
	decodeSummary,
	decodeUsageRecord,
	sumRecords
} from './http-full-decode';
import { createCostsFullTransport, type HttpCostsFullApi } from './http-full-transport';
import type {
	AgentBudgetAction,
	AgentBudgetDraft,
	AgentBudgetOperationOutcome,
	BreakdownKey,
	BudgetPeriodState,
	MemberCostOverview,
	MemberRecordsFilter,
	OperationOutcome,
	OrganizationCost,
	PeriodRequest,
	PriceDraft,
	PriceVersion,
	PricesOverview,
	ProjectBreakdownKey,
	ProjectInvestment,
	ProjectInvestmentRow,
	UsageExportResult,
	UsageRecord,
	UsageRecordPage
} from './types';

export type { HttpCostsFullApi } from './http-full-transport';

export function createHttpCostsFullApi(deps: {
	http: GatewayHttpTransport;
	orgApi: Pick<OrgApi, 'view'>;
	costsReadApi: HttpCostsReadApi;
	session: () => { accountAdmin: boolean } | undefined;
	now?: () => string;
}): HttpCostsFullApi {
	const now = deps.now ?? (() => new Date().toISOString());
	const { loadSnapshot, isOwnerOrAdmin, queryCosts, toResolvedPeriod, toCoverage, queryBody } =
		createCostsFullTransport({
			http: deps.http,
			orgApi: deps.orgApi,
			session: deps.session,
			now
		});

	async function readPrices(organizationId: string, signal?: AbortSignal): Promise<PricesOverview> {
		const pricesPath = `/api/v1/organizations/${encodeURIComponent(organizationId)}/costs/prices`;
		const [reply, snapshot] = await Promise.all([
			(signal ? deps.http.read(pricesPath, signal) : deps.http.read(pricesPath)).catch((err) => {
				if (signal?.aborted) throw err;
				return undefined;
			}),
			loadSnapshot(organizationId)
		]);
		if (!reply) {
			throw {
				source: 'api',
				code: 'unavailable',
				message: 'Model prices could not be reached.'
			} satisfies AppError;
		}
		const result = unwrapGatewayResult(reply, 'Model prices could not be loaded.');
		const membersById = new Map((snapshot?.members ?? []).map((m) => [m.id, m.name]));
		const rawPrices = Array.isArray(result.prices) ? result.prices : [];
		const prices = rawPrices
			.map((p) => decodePriceVersion(p, membersById))
			.filter((p): p is PriceVersion => p !== undefined);
		const canManage = await isOwnerOrAdmin(organizationId, snapshot);
		return { prices, viewer: { canManage } };
	}

	return Object.freeze<HttpCostsFullApi>({
		prices: readPrices,

		async addPrice(organizationId: string, draft: PriceDraft): Promise<PriceVersion> {
			const effectiveIso = draft.effectiveFrom.includes('T')
				? draft.effectiveFrom
				: `${draft.effectiveFrom}T00:00:00Z`;
			let reply: GatewayReply;
			try {
				reply = await deps.http.submit(
					`/api/v1/organizations/${encodeURIComponent(organizationId)}/costs/prices`,
					{
						id: draft.operationId,
						provider: draft.provider,
						model: draft.model,
						effective_from: effectiveIso,
						...(draft.inputMicroUsdPerMillion !== null
							? { input_micro_usd_per_million: draft.inputMicroUsdPerMillion }
							: {}),
						...(draft.outputMicroUsdPerMillion !== null
							? { output_micro_usd_per_million: draft.outputMicroUsdPerMillion }
							: {}),
						...(draft.cacheReadMicroUsdPerMillion !== null
							? { cache_read_micro_usd_per_million: draft.cacheReadMicroUsdPerMillion }
							: {}),
						...(draft.cacheWriteMicroUsdPerMillion !== null
							? { cache_write_micro_usd_per_million: draft.cacheWriteMicroUsdPerMillion }
							: {})
					}
				);
			} catch {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'Model price could not be saved.'
				} satisfies AppError;
			}
			const result = unwrapGatewayResult(reply, 'Model price was refused.');
			const snapshot = await loadSnapshot(organizationId);
			const membersById = new Map((snapshot?.members ?? []).map((m) => [m.id, m.name]));
			const price = decodePriceVersion(result.price ?? result, membersById);
			if (!price) {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'Saved price could not be read.'
				} satisfies AppError;
			}
			return price;
		},

		async priceOperation(organizationId: string, operationId: string): Promise<OperationOutcome> {
			const overview = await readPrices(organizationId);
			const found = overview.prices.find((p) => p.id === operationId);
			return found
				? { state: 'applied', price: found }
				: { state: 'not_applied', reason: 'That rate was not recorded.' };
		},

		async organizationCost(
			organizationId: string,
			period: PeriodRequest,
			signal?: AbortSignal
		): Promise<OrganizationCost> {
			const [result, snapshot] = await Promise.all([
				queryCosts(
					organizationId,
					queryBody(period, { include_records: true, limit: 500 }),
					signal
				),
				loadSnapshot(organizationId)
			]);
			const rawRecords = Array.isArray(result.records) ? result.records : [];
			const records = rawRecords
				.map((r) => decodeUsageRecord(r, snapshot))
				.filter((r): r is UsageRecord => r !== undefined);
			const breakdowns = buildOrgBreakdowns(records, snapshot);
			return {
				period: toResolvedPeriod(result, period),
				totals: decodeSummary(result.summary),
				...breakdowns,
				coverage: toCoverage(result)
			};
		},

		async records(
			organizationId: string,
			period: PeriodRequest,
			key: BreakdownKey | { all: true }
		): Promise<UsageRecordPage> {
			const [result, snapshot] = await Promise.all([
				queryCosts(organizationId, queryBody(period, { include_records: true, limit: 500 })),
				loadSnapshot(organizationId)
			]);
			const rawRecords = Array.isArray(result.records) ? result.records : [];
			const all = rawRecords
				.map((r) => decodeUsageRecord(r, snapshot))
				.filter((r): r is UsageRecord => r !== undefined)
				.filter((r) => matchesBreakdownKey(r, key));
			return { records: all, total: all.length };
		},

		async projectInvestment(
			organizationId: string,
			request: { timeZone: string }
		): Promise<ProjectInvestment> {
			const [allRes, monthRes, snapshot] = await Promise.all([
				queryCosts(organizationId, {
					period: 'all_time',
					time_zone: request.timeZone,
					include_records: true,
					limit: 500
				}),
				queryCosts(organizationId, {
					period: 'this_month',
					time_zone: request.timeZone,
					include_records: true,
					limit: 500
				}),
				loadSnapshot(organizationId)
			]);

			const allRecs = (Array.isArray(allRes.records) ? allRes.records : [])
				.map((r) => decodeUsageRecord(r, snapshot))
				.filter((r): r is UsageRecord => r !== undefined);
			const monthRecs = (Array.isArray(monthRes.records) ? monthRes.records : [])
				.map((r) => decodeUsageRecord(r, snapshot))
				.filter((r): r is UsageRecord => r !== undefined);

			const projectIds = new Set<string>();
			for (const p of snapshot?.projects ?? []) projectIds.add(p.id);
			for (const r of [...allRecs, ...monthRecs]) {
				if (r.attribution.project) projectIds.add(r.attribution.project.id);
			}

			const projects: ProjectInvestmentRow[] = [...projectIds].map((projectId) => ({
				projectId,
				toDate: sumRecords(allRecs.filter((r) => r.attribution.project?.id === projectId)),
				thisMonth: sumRecords(monthRecs.filter((r) => r.attribution.project?.id === projectId))
			}));

			return {
				scope: monthRes.scoped_to_projects === true ? 'managed' : 'organization',
				thisMonth: toResolvedPeriod(monthRes, { kind: 'this_month', timeZone: request.timeZone }),
				projects,
				coverage: toCoverage(monthRes)
			};
		},

		async projectRecords(
			organizationId: string,
			projectId: string,
			period: PeriodRequest,
			key: ProjectBreakdownKey
		): Promise<UsageRecordPage> {
			const [result, snapshot] = await Promise.all([
				queryCosts(
					organizationId,
					queryBody(period, { project_id: projectId, include_records: true, limit: 500 })
				),
				loadSnapshot(organizationId)
			]);
			const rawRecords = Array.isArray(result.records) ? result.records : [];
			const filtered = rawRecords
				.map((r) => decodeUsageRecord(r, snapshot))
				.filter((r): r is UsageRecord => r !== undefined)
				.filter((r) => matchesProjectKey(r, key));
			return { records: filtered, total: filtered.length };
		},

		async exportRecords(
			organizationId: string,
			scope: { kind: 'organization' } | { kind: 'project'; projectId: string },
			period: PeriodRequest
		): Promise<UsageExportResult> {
			const [result, snapshot] = await Promise.all([
				queryCosts(
					organizationId,
					queryBody(period, {
						...(scope.kind === 'project' ? { project_id: scope.projectId } : {}),
						include_records: true,
						limit: 1000
					})
				),
				loadSnapshot(organizationId)
			]);
			const records = (Array.isArray(result.records) ? result.records : [])
				.map((r) => decodeUsageRecord(r, snapshot))
				.filter((r): r is UsageRecord => r !== undefined);
			return {
				period: toResolvedPeriod(result, period),
				records,
				coverage: toCoverage(result)
			};
		},

		async memberCost(
			organizationId: string,
			memberId: string,
			request: { timeZone: string }
		): Promise<MemberCostOverview> {
			const [weekRes, monthRes, lastMonthRes, budgetsRes, snapshot] = await Promise.all([
				queryCosts(organizationId, {
					agent_id: memberId,
					period: 'this_week',
					time_zone: request.timeZone,
					include_records: true,
					limit: 500
				}),
				queryCosts(organizationId, {
					agent_id: memberId,
					period: 'this_month',
					time_zone: request.timeZone,
					include_records: true,
					limit: 500
				}),
				queryCosts(organizationId, {
					agent_id: memberId,
					period: 'last_month',
					time_zone: request.timeZone
				}),
				deps.costsReadApi.agentBudgets(organizationId).catch(() => undefined),
				loadSnapshot(organizationId)
			]);

			const monthRecords = (Array.isArray(monthRes.records) ? monthRes.records : [])
				.map((r) => decodeUsageRecord(r, snapshot))
				.filter((r): r is UsageRecord => r !== undefined);
			const todayPrefix = now().slice(0, 10);
			const todayRecords = monthRecords.filter((r) => r.occurredAt.slice(0, 10) === todayPrefix);
			const { byProject, topTasks } = buildMemberBreakdowns(monthRecords, snapshot);

			const scoped = monthRes.scoped_to_projects === true;
			const agentBudget = budgetsRes?.agents.find((a) => a.memberId === memberId);
			const canManage = await isOwnerOrAdmin(organizationId, snapshot);

			return {
				memberId,
				scope: scoped ? 'managed' : 'organization',
				timeZone: request.timeZone,
				periods: {
					today: sumRecords(todayRecords),
					thisWeek: decodeSummary(weekRes.summary),
					thisMonth: decodeSummary(monthRes.summary),
					lastMonth: decodeSummary(lastMonthRes.summary)
				},
				byProject,
				topTasks,
				...(!scoped && budgetsRes
					? {
							budgets: {
								zone: budgetsRes.zone,
								...(agentBudget?.daily ? { daily: agentBudget.daily } : {}),
								...(agentBudget?.monthly ? { monthly: agentBudget.monthly } : {}),
								canManage
							}
						}
					: {}),
				coverage: toCoverage(monthRes)
			};
		},

		async memberRecords(
			organizationId: string,
			memberId: string,
			request: { timeZone: string; filter: MemberRecordsFilter }
		): Promise<UsageRecordPage> {
			const period =
				request.filter.kind === 'period'
					? request.filter.period === 'today'
						? 'this_week'
						: request.filter.period
					: 'this_month';
			const [result, snapshot] = await Promise.all([
				queryCosts(organizationId, {
					agent_id: memberId,
					period,
					time_zone: request.timeZone,
					include_records: true,
					limit: 500
				}),
				loadSnapshot(organizationId)
			]);
			let records = (Array.isArray(result.records) ? result.records : [])
				.map((r) => decodeUsageRecord(r, snapshot))
				.filter((r): r is UsageRecord => r !== undefined);

			const f = request.filter;
			if (f.kind === 'period' && f.period === 'today') {
				const today = now().slice(0, 10);
				records = records.filter((r) => r.occurredAt.slice(0, 10) === today);
			} else if (f.kind === 'project') {
				records = records.filter((r) => r.attribution.project?.id === f.projectId);
			} else if (f.kind === 'org_not_on_task') {
				records = records.filter((r) => !r.attribution.project);
			} else if (f.kind === 'task') {
				records = records.filter((r) => r.attribution.task?.ref === f.taskRef);
			}
			return { records, total: records.length };
		},

		async setAgentBudget(
			organizationId: string,
			memberId: string,
			draft: AgentBudgetDraft
		): Promise<BudgetPeriodState> {
			let reply: GatewayReply;
			try {
				reply = await deps.http.submit(
					`/api/v1/organizations/${encodeURIComponent(organizationId)}/budgets/set`,
					{
						operation_id: draft.operationId,
						scope_kind: 'agent',
						agent_id: memberId,
						period_kind: draft.periodKind === 'daily' ? 'day' : 'month',
						unit: draft.unit === 'usd' ? 'micro_usd' : 'tokens',
						limit_amount: draft.limit
					}
				);
			} catch {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'Agent budget could not be saved.'
				} satisfies AppError;
			}
			const result = unwrapGatewayResult(reply, 'Agent budget update was refused.');
			return decodeBudgetState(result.budget ?? result);
		},

		async agentBudgetAction(
			organizationId: string,
			memberId: string,
			action: AgentBudgetAction
		): Promise<{ message: string }> {
			const listReply = await deps.http.read(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/budgets?scope_kind=agent&agent_id=${encodeURIComponent(memberId)}`
			);
			const listResult = unwrapGatewayResult(listReply, 'Agent budgets could not be loaded.');
			const wirePeriod = action.periodKind === 'daily' ? 'day' : 'month';
			const budgets = Array.isArray(listResult.budgets) ? listResult.budgets : [];
			const target = budgets
				.map(asRec)
				.find((b) => b && b.period_kind === wirePeriod && asStr(b.id));
			const budgetId = asStr(target?.id);
			if (!budgetId) {
				throw {
					source: 'api',
					code: 'not_found',
					message: 'That budget was not found.'
				} satisfies AppError;
			}
			const endpoint =
				action.action === 'clear'
					? 'clear'
					: action.action === 'allow_once'
						? 'extend-once'
						: 'keep-paused';
			const reply = await deps.http.submit(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/budgets/${endpoint}`,
				{
					operation_id: action.operationId,
					budget_id: budgetId,
					...(action.reason ? { reason: action.reason } : {})
				}
			);
			unwrapGatewayResult(reply, 'Budget action was refused.');
			const message =
				action.action === 'clear'
					? 'Budget cleared.'
					: action.action === 'allow_once'
						? 'Extended budget by 20% for this period.'
						: 'Agent will stay paused until the next period.';
			return { message };
		},

		async agentBudgetOperation(): Promise<AgentBudgetOperationOutcome> {
			return { state: 'applied' };
		}
	});
}
