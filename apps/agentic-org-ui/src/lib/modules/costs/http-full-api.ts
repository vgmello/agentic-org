import type { OrgApi } from '$lib/modules/organization/api';
import {
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import { readJsonObject, readResponse } from '$lib/platform/http/wire';
import { asNum, asRec, asStr, type Rec } from '$lib/platform/http/decode';
import { readOrganizationsList } from '$lib/platform/organizations/readers';
import type { AppError, OrgSnapshot } from '$lib/proto';
import type { HttpCostsReadApi } from './http-api';
import { emptyUsageTotals } from './insights-http-api';
import type {
	AgentBudgetAction,
	AgentBudgetDraft,
	AgentBudgetOperationOutcome,
	AttributionBasis,
	BreakdownKey,
	BreakdownRow,
	BudgetPeriodState,
	Completeness,
	CostBasis,
	Coverage,
	MemberCostOverview,
	MemberProjectSpendRow,
	MemberRecordsFilter,
	MemberTaskSpendRow,
	OperationOutcome,
	OrganizationCost,
	PeriodRequest,
	PriceDraft,
	PriceVersion,
	PricesOverview,
	ProjectBreakdownKey,
	ProjectInvestment,
	ProjectInvestmentRow,
	ResolvedPeriod,
	UsageExportResult,
	UsageRecord,
	UsageRecordPage,
	UsageTotals
} from './types';

const asNullableNum = (v: unknown): number | null =>
	typeof v === 'number' && Number.isFinite(v) ? v : null;

const VALID_COMPLETENESS: ReadonlySet<string> = new Set(['complete', 'partial', 'unavailable']);
const VALID_COST_BASIS: ReadonlySet<string> = new Set([
	'runtime_estimate',
	'token_estimate',
	'unpriced',
	'unknown'
]);
const VALID_ATTR_BASIS: ReadonlySet<string> = new Set([
	'work_for',
	'current_task',
	'project_overhead',
	'team_overhead',
	'organization_overhead'
]);

function decodeSummary(raw: unknown): UsageTotals {
	const r = asRec(raw);
	if (!r) return emptyUsageTotals();
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

function decodeCompleteness(raw: unknown): Completeness {
	const s = asStr(raw);
	return s && VALID_COMPLETENESS.has(s) ? (s as Completeness) : 'unavailable';
}

function decodeCostBasis(raw: unknown): CostBasis {
	const s = asStr(raw);
	return s && VALID_COST_BASIS.has(s) ? (s as CostBasis) : 'unknown';
}

function decodeAttributionBasis(raw: unknown): AttributionBasis {
	const s = asStr(raw);
	return s && VALID_ATTR_BASIS.has(s) ? (s as AttributionBasis) : 'organization_overhead';
}

function decodePriceVersion(
	raw: unknown,
	membersById: ReadonlyMap<string, string>
): PriceVersion | undefined {
	const r = asRec(raw);
	const id = asStr(r?.id);
	const provider = asStr(r?.provider);
	const model = asStr(r?.model);
	if (!r || !id || !provider || !model) return undefined;
	const createdById = asStr(r.created_by_id);
	return {
		id,
		provider,
		model,
		effectiveFrom: asStr(r.effective_from) ?? new Date(0).toISOString(),
		inputMicroUsdPerMillion: asNullableNum(r.input_micro_usd_per_million),
		outputMicroUsdPerMillion: asNullableNum(r.output_micro_usd_per_million),
		cacheReadMicroUsdPerMillion: asNullableNum(r.cache_read_micro_usd_per_million),
		cacheWriteMicroUsdPerMillion: asNullableNum(r.cache_write_micro_usd_per_million),
		used: r.used === true,
		created: {
			at: asStr(r.created_at) ?? new Date(0).toISOString(),
			...(createdById ? { byName: membersById.get(createdById) ?? createdById } : {})
		}
	};
}

function decodeUsageTokens(r: Rec): UsageRecord['tokens'] {
	const tokens = {
		input: asNullableNum(r.input_tokens),
		output: asNullableNum(r.output_tokens),
		cacheRead: asNullableNum(r.cache_read_tokens),
		cacheWrite: asNullableNum(r.cache_write_tokens),
		reasoning: asNullableNum(r.reasoning_tokens)
	};
	const hasAny = Object.values(tokens).some((v) => v !== null);
	return hasAny ? tokens : null;
}

function decodeUsageConsumer(r: Rec, snapshot: OrgSnapshot | undefined): UsageRecord['consumer'] {
	const employeeId = asStr(r.employee_id);
	if (!employeeId || asStr(r.actor_kind) === 'one_off') {
		return { kind: 'one_off', label: asStr(r.producer_id) ?? 'One-off run' };
	}
	const member = snapshot?.members.find((m) => m.id === employeeId);
	return {
		kind: 'agent',
		memberId: employeeId,
		name: member?.name ?? employeeId,
		...(member?.agent?.status === 'offboarded' ? { former: true } : {})
	};
}

function decodeUsageAttribution(
	r: Rec,
	snapshot: OrgSnapshot | undefined
): UsageRecord['attribution'] {
	const projectId = asStr(r.project_id);
	const project = projectId ? snapshot?.projects?.find((p) => p.id === projectId) : undefined;
	const teamId = asStr(r.team_id);
	const team = teamId ? snapshot?.teams.find((t) => t.id === teamId) : undefined;
	const taskId = asStr(r.task_id);
	return {
		basis: decodeAttributionBasis(r.attribution_basis),
		...(taskId ? { task: { ref: taskId, title: taskId } } : {}),
		...(projectId ? { project: { id: projectId, name: project?.name ?? projectId } } : {}),
		...(teamId ? { team: { id: teamId, name: team?.name ?? teamId } } : {})
	};
}

function decodeUsageCorrection(r: Rec): UsageRecord['correction'] | undefined {
	const reason = asStr(r.correction_reason);
	if (!reason) return undefined;
	return {
		reason,
		byName: asStr(r.recorded_by_id) ?? 'System',
		at: asStr(r.recorded_at) ?? new Date(0).toISOString()
	};
}

function decodeUsageRecord(
	raw: unknown,
	snapshot: OrgSnapshot | undefined
): UsageRecord | undefined {
	const r = asRec(raw);
	const id = asStr(r?.id);
	if (!r || !id) return undefined;

	const rawWorkType = asStr(r.work_type);
	const workType = rawWorkType === 'build' || rawWorkType === 'run' ? rawWorkType : null;
	const issue = asStr(r.cost_issue) ?? asStr(r.token_issue);
	const correction = decodeUsageCorrection(r);

	return {
		id,
		occurredAt: asStr(r.occurred_at) ?? new Date(0).toISOString(),
		consumer: decodeUsageConsumer(r, snapshot),
		runtime: asStr(r.runtime) ?? 'unknown',
		provider: asStr(r.provider) ?? null,
		model: asStr(r.model) ?? null,
		tokens: decodeUsageTokens(r),
		tokenStatus: decodeCompleteness(r.token_status),
		costMicroUsd: asNullableNum(r.cost_micro_usd),
		costBasis: decodeCostBasis(r.cost_basis),
		costStatus: decodeCompleteness(r.cost_status),
		apiEquivalent: r.api_equivalent === true || r.is_subscription === true,
		attribution: decodeUsageAttribution(r, snapshot),
		workType,
		...(issue ? { issue } : {}),
		...(correction ? { correction } : {})
	};
}

function sumRecords(records: readonly UsageRecord[]): UsageTotals {
	if (records.length === 0) return emptyUsageTotals();
	let knownCost: number | null = null;
	let knownTokens: number | null = null;
	let cacheRead: number | null = null;
	let tokenIncomplete = 0;
	let costIncomplete = 0;
	let unpriced = 0;
	let unknownCost = 0;
	let includesApiEquivalent = false;

	for (const rec of records) {
		if (rec.costMicroUsd !== null) {
			knownCost = (knownCost ?? 0) + rec.costMicroUsd;
		}
		if (rec.tokens) {
			const parts = [
				rec.tokens.input,
				rec.tokens.output,
				rec.tokens.cacheRead,
				rec.tokens.cacheWrite
			].filter((n): n is number => n !== null);
			if (parts.length > 0) {
				knownTokens = (knownTokens ?? 0) + parts.reduce((a, b) => a + b, 0);
			}
			if (rec.tokens.cacheRead !== null) {
				cacheRead = (cacheRead ?? 0) + rec.tokens.cacheRead;
			}
		}
		if (rec.tokenStatus !== 'complete') tokenIncomplete += 1;
		if (rec.costStatus !== 'complete') costIncomplete += 1;
		if (rec.costBasis === 'unpriced') unpriced += 1;
		if (rec.costBasis === 'unknown') unknownCost += 1;
		if (rec.apiEquivalent) includesApiEquivalent = true;
	}

	return {
		records: records.length,
		knownCostMicroUsd: knownCost,
		knownTokens,
		cacheReadTokens: cacheRead,
		tokenIncompleteRecords: tokenIncomplete,
		costIncompleteRecords: costIncomplete,
		unpricedRecords: unpriced,
		unknownCostRecords: unknownCost,
		includesApiEquivalent
	};
}

function buildOrgBreakdowns(
	records: readonly UsageRecord[],
	snapshot: OrgSnapshot | undefined
): {
	byProject: BreakdownRow[];
	byConsumer: BreakdownRow[];
	byWorkType: BreakdownRow[];
} {
	const projectGroups = new Map<
		string,
		{ label: string; archived?: boolean; items: UsageRecord[] }
	>();
	const notOnTask: UsageRecord[] = [];
	const consumerGroups = new Map<
		string,
		{ label: string; memberId?: string; archived?: boolean; oneOff?: boolean; items: UsageRecord[] }
	>();
	const workTypeGroups = new Map<'build' | 'run' | 'not_recorded', UsageRecord[]>();

	for (const rec of records) {
		if (rec.attribution.project) {
			const pid = rec.attribution.project.id;
			const proj = snapshot?.projects?.find((p) => p.id === pid);
			const g = projectGroups.get(pid) ?? {
				label: proj?.name ?? rec.attribution.project.name,
				archived: proj?.archived,
				items: []
			};
			g.items.push(rec);
			projectGroups.set(pid, g);
		} else {
			notOnTask.push(rec);
		}

		if (rec.consumer.kind === 'agent') {
			const mid = rec.consumer.memberId;
			const g = consumerGroups.get(mid) ?? {
				label: rec.consumer.name,
				memberId: mid,
				archived: rec.consumer.former,
				items: []
			};
			g.items.push(rec);
			consumerGroups.set(mid, g);
		} else {
			const g = consumerGroups.get('__one_off__') ?? {
				label: 'One-off runs',
				oneOff: true,
				items: []
			};
			g.items.push(rec);
			consumerGroups.set('__one_off__', g);
		}

		const wt = rec.workType ?? 'not_recorded';
		const list = workTypeGroups.get(wt) ?? [];
		list.push(rec);
		workTypeGroups.set(wt, list);
	}

	const byProject: BreakdownRow[] = [...projectGroups.entries()].map(([projectId, g]) => ({
		id: `project:${projectId}`,
		key: { dimension: 'project', projectId },
		label: g.label,
		projectId,
		...(g.archived ? { archived: true } : {}),
		totals: sumRecords(g.items)
	}));
	if (notOnTask.length > 0) {
		byProject.push({
			id: 'project:not-on-task',
			key: { dimension: 'project', notOnTask: true },
			label: 'Not on a task',
			totals: sumRecords(notOnTask)
		});
	}

	const byConsumer: BreakdownRow[] = [...consumerGroups.entries()].map(([k, g]) => ({
		id: g.oneOff ? 'consumer:one-off' : `consumer:${k}`,
		key: g.oneOff
			? { dimension: 'consumer', oneOff: true }
			: { dimension: 'consumer', memberId: k },
		label: g.label,
		...(g.memberId ? { memberId: g.memberId } : {}),
		...(g.archived ? { archived: true } : {}),
		totals: sumRecords(g.items)
	}));

	const wtLabels: Record<'build' | 'run' | 'not_recorded', string> = {
		build: 'Build (R&D)',
		run: 'Run',
		not_recorded: 'Not recorded'
	};
	const byWorkType: BreakdownRow[] = (['build', 'run', 'not_recorded'] as const)
		.filter((wt) => (workTypeGroups.get(wt)?.length ?? 0) > 0)
		.map((wt) => ({
			id: `work-type:${wt}`,
			key: { dimension: 'work_type', workType: wt },
			label: wtLabels[wt],
			totals: sumRecords(workTypeGroups.get(wt) ?? [])
		}));

	return { byProject, byConsumer, byWorkType };
}

function decodeBudgetState(raw: unknown): BudgetPeriodState {
	const r = asRec(raw);
	const unit = asStr(r?.unit) === 'tokens' ? 'tokens' : 'usd';
	const limit = asNum(r?.effective_limit) ?? asNum(r?.limit_amount) ?? 0;
	const rawHold = asStr(r?.hold_state);
	return {
		unit,
		limit,
		spent: asNum(r?.used_amount) ?? 0,
		complete: asStr(r?.usage_status) === 'complete' || r?.usage_status === undefined,
		...(rawHold === 'stop_pending' || rawHold === 'held' ? { hold: rawHold } : {}),
		...(r?.extension_used === true ? { extended: true } : {}),
		...(asStr(r?.state_summary) === 'warning80' ? { warned: true } : {}),
		period: {
			key: asStr(r?.period_key) ?? '',
			startsAt: asStr(r?.starts_at) ?? '',
			endsAt: asStr(r?.ends_at) ?? ''
		}
	};
}

function matchesBreakdownKey(rec: UsageRecord, key: BreakdownKey | { all: true }): boolean {
	if ('all' in key) return true;
	if (key.dimension === 'project') {
		if ('notOnTask' in key) return !rec.attribution.project;
		return rec.attribution.project?.id === key.projectId;
	}
	if (key.dimension === 'consumer') {
		if ('oneOff' in key) return rec.consumer.kind === 'one_off';
		return rec.consumer.kind === 'agent' && rec.consumer.memberId === key.memberId;
	}
	return (rec.workType ?? 'not_recorded') === key.workType;
}

function matchesProjectKey(rec: UsageRecord, key: ProjectBreakdownKey): boolean {
	if ('all' in key) return true;
	if (key.dimension === 'consumer') {
		if ('oneOff' in key) return rec.consumer.kind === 'one_off';
		return rec.consumer.kind === 'agent' && rec.consumer.memberId === key.memberId;
	}
	if (key.dimension === 'work_type') {
		return (rec.workType ?? 'not_recorded') === key.workType;
	}
	if (key.dimension === 'model') {
		return (rec.model ?? null) === key.model && (rec.provider ?? null) === key.provider;
	}
	if (key.dimension === 'team') {
		if ('notOnTask' in key) return !rec.attribution.team;
		return rec.attribution.team?.id === key.teamId;
	}
	return true;
}

export interface HttpCostsFullApi {
	prices(organizationId: string): Promise<PricesOverview>;
	addPrice(organizationId: string, draft: PriceDraft): Promise<PriceVersion>;
	priceOperation(organizationId: string, operationId: string): Promise<OperationOutcome>;
	organizationCost(organizationId: string, period: PeriodRequest): Promise<OrganizationCost>;
	records(
		organizationId: string,
		period: PeriodRequest,
		key: BreakdownKey | { all: true },
		cursor?: string
	): Promise<UsageRecordPage>;
	projectInvestment(
		organizationId: string,
		request: { timeZone: string }
	): Promise<ProjectInvestment>;
	projectRecords(
		organizationId: string,
		projectId: string,
		period: PeriodRequest,
		key: ProjectBreakdownKey,
		cursor?: string
	): Promise<UsageRecordPage>;
	exportRecords(
		organizationId: string,
		scope: { kind: 'organization' } | { kind: 'project'; projectId: string },
		period: PeriodRequest
	): Promise<UsageExportResult>;
	memberCost(
		organizationId: string,
		memberId: string,
		request: { timeZone: string }
	): Promise<MemberCostOverview>;
	memberRecords(
		organizationId: string,
		memberId: string,
		request: { timeZone: string; filter: MemberRecordsFilter },
		cursor?: string
	): Promise<UsageRecordPage>;
	setAgentBudget(
		organizationId: string,
		memberId: string,
		draft: AgentBudgetDraft
	): Promise<BudgetPeriodState>;
	agentBudgetAction(
		organizationId: string,
		memberId: string,
		action: AgentBudgetAction
	): Promise<{ message: string }>;
	agentBudgetOperation(
		organizationId: string,
		operationId: string
	): Promise<AgentBudgetOperationOutcome>;
}

export function createHttpCostsFullApi(deps: {
	http: GatewayHttpTransport;
	orgApi: Pick<OrgApi, 'view'>;
	costsReadApi: HttpCostsReadApi;
	session: () => { accountAdmin: boolean } | undefined;
	now?: () => string;
}): HttpCostsFullApi {
	const now = deps.now ?? (() => new Date().toISOString());

	async function loadSnapshot(organizationId: string): Promise<OrgSnapshot | undefined> {
		try {
			return (await deps.orgApi.view(organizationId)).snapshot;
		} catch {
			return undefined;
		}
	}

	async function isOwnerOrAdmin(
		organizationId: string,
		snapshot: OrgSnapshot | undefined
	): Promise<boolean> {
		if (deps.session()?.accountAdmin) return true;
		try {
			const reply = await deps.http.read('/api/v1/organizations');
			if (reply.status !== 200) return false;
			const env = readResponse(reply.body, readJsonObject);
			if ('error' in env) return false;
			const org = readOrganizationsList(env.result).organizations.find(
				(o) => o.id === organizationId
			);
			const viewer = org?.employee_id
				? snapshot?.members.find((m) => m.id === org.employee_id)
				: undefined;
			return Boolean(viewer?.owner);
		} catch {
			return false;
		}
	}

	async function queryCosts(organizationId: string, body: Record<string, unknown>): Promise<Rec> {
		const path = `/api/v1/organizations/${encodeURIComponent(organizationId)}/costs/query`;
		let reply: GatewayReply;
		try {
			reply = await deps.http.submit(path, body);
		} catch {
			throw {
				source: 'api',
				code: 'unavailable',
				message: 'Cost data could not be reached.'
			} satisfies AppError;
		}
		return unwrapGatewayResult(reply, 'Cost data could not be loaded.');
	}

	function toResolvedPeriod(result: Rec, period: PeriodRequest): ResolvedPeriod {
		const endsAt = asStr(result.ends_at) ?? now();
		const startsAt = asStr(result.starts_at);
		return {
			kind: period.kind,
			...(startsAt ? { startsAt } : {}),
			endsAt,
			timeZone: asStr(result.time_zone) ?? period.timeZone,
			toNow:
				period.kind === 'this_week' || period.kind === 'this_month' || period.kind === 'all_time'
		};
	}

	function toCoverage(result: Rec): Coverage {
		const summary = asRec(result.summary);
		return {
			checkpointAt: asStr(summary?.coverage_as_of) ?? asStr(result.ends_at) ?? now(),
			gaps: []
		};
	}

	function queryBody(
		period: PeriodRequest,
		extra: Record<string, unknown> = {}
	): Record<string, unknown> {
		return {
			period: period.kind,
			time_zone: period.timeZone,
			...(period.from ? { from_date: period.from } : {}),
			...(period.to ? { to_date: period.to } : {}),
			...extra
		};
	}

	async function readPrices(organizationId: string): Promise<PricesOverview> {
		const [reply, snapshot] = await Promise.all([
			deps.http
				.read(`/api/v1/organizations/${encodeURIComponent(organizationId)}/costs/prices`)
				.catch(() => undefined),
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
			period: PeriodRequest
		): Promise<OrganizationCost> {
			const [result, snapshot] = await Promise.all([
				queryCosts(organizationId, queryBody(period, { include_records: true, limit: 500 })),
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

			const projMap = new Map<
				string,
				{ label: string; archived?: boolean; items: UsageRecord[] }
			>();
			const notOnTask: UsageRecord[] = [];
			const taskMap = new Map<
				string,
				{ title: string; project?: { id: string; name: string }; items: UsageRecord[] }
			>();

			for (const rec of monthRecords) {
				if (rec.attribution.project) {
					const pid = rec.attribution.project.id;
					const proj = snapshot?.projects?.find((p) => p.id === pid);
					const g = projMap.get(pid) ?? {
						label: proj?.name ?? rec.attribution.project.name,
						archived: proj?.archived,
						items: []
					};
					g.items.push(rec);
					projMap.set(pid, g);
				} else {
					notOnTask.push(rec);
				}
				if (rec.attribution.task) {
					const ref = rec.attribution.task.ref;
					const g = taskMap.get(ref) ?? {
						title: rec.attribution.task.title,
						project: rec.attribution.project,
						items: []
					};
					g.items.push(rec);
					taskMap.set(ref, g);
				}
			}

			const byProject: MemberProjectSpendRow[] = [...projMap.entries()].map(([projectId, g]) => ({
				id: `project:${projectId}`,
				projectId,
				label: g.label,
				...(g.archived ? { archived: true } : {}),
				totals: sumRecords(g.items)
			}));
			if (notOnTask.length > 0) {
				byProject.push({
					id: 'project:not-on-task',
					label: 'Not on a task',
					notOnTask: true,
					totals: sumRecords(notOnTask)
				});
			}

			const topTasks: MemberTaskSpendRow[] = [...taskMap.entries()]
				.map(([taskRef, g]) => ({
					taskRef,
					title: g.title,
					...(g.project ? { projectId: g.project.id, projectName: g.project.name } : {}),
					totals: sumRecords(g.items)
				}))
				.sort((a, b) => (b.totals.knownCostMicroUsd ?? 0) - (a.totals.knownCostMicroUsd ?? 0))
				.slice(0, 10);

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
