import { asNum, asRec, asStr, type Rec } from '$lib/platform/http/decode';
import type { OrgSnapshot } from '$lib/proto';
import { emptyUsageTotals } from './insights-http-api';
import type {
	AttributionBasis,
	BudgetPeriodState,
	Completeness,
	CostBasis,
	PriceVersion,
	UsageRecord,
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

export function decodeSummary(raw: unknown): UsageTotals {
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

export function decodePriceVersion(
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

export function decodeUsageRecord(
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

export function sumRecords(records: readonly UsageRecord[]): UsageTotals {
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
		if (rec.costMicroUsd !== null) knownCost = (knownCost ?? 0) + rec.costMicroUsd;
		if (rec.tokens) {
			const parts = [
				rec.tokens.input,
				rec.tokens.output,
				rec.tokens.cacheRead,
				rec.tokens.cacheWrite
			].filter((n): n is number => n !== null);
			if (parts.length > 0) knownTokens = (knownTokens ?? 0) + parts.reduce((a, b) => a + b, 0);
			if (rec.tokens.cacheRead !== null) cacheRead = (cacheRead ?? 0) + rec.tokens.cacheRead;
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

export function decodeBudgetState(raw: unknown): BudgetPeriodState {
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
