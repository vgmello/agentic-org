import type { OrgApi } from '$lib/modules/organization/api';
import { asRec, asStr, type Rec } from '$lib/platform/http/decode';
import {
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import { readJsonObject, readResponse } from '$lib/platform/http/wire';
import { readOrganizationsList } from '$lib/platform/organizations/readers';
import type { AppError, OrgSnapshot } from '$lib/proto';
import type {
	AgentBudgetAction,
	AgentBudgetDraft,
	AgentBudgetOperationOutcome,
	BreakdownKey,
	BudgetPeriodState,
	Coverage,
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
	ResolvedPeriod,
	UsageExportResult,
	UsageRecordPage
} from './types';

export interface HttpCostsFullApi {
	prices(organizationId: string, signal?: AbortSignal): Promise<PricesOverview>;
	addPrice(organizationId: string, draft: PriceDraft): Promise<PriceVersion>;
	priceOperation(organizationId: string, operationId: string): Promise<OperationOutcome>;
	organizationCost(
		organizationId: string,
		period: PeriodRequest,
		signal?: AbortSignal
	): Promise<OrganizationCost>;
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

export function createCostsFullTransport(deps: {
	http: GatewayHttpTransport;
	orgApi: Pick<OrgApi, 'view'>;
	session: () => { accountAdmin: boolean } | undefined;
	now: () => string;
}) {
	async function loadSnapshot(
		organizationId: string,
		signal?: AbortSignal
	): Promise<OrgSnapshot | undefined> {
		try {
			return (await deps.orgApi.view(organizationId, signal)).snapshot;
		} catch (err) {
			if (signal?.aborted) throw err;
			return undefined;
		}
	}

	async function isOwnerOrAdmin(
		organizationId: string,
		snapshot: OrgSnapshot | undefined,
		signal?: AbortSignal
	): Promise<boolean> {
		if (deps.session()?.accountAdmin) return true;
		try {
			const reply = await (signal
				? deps.http.read('/api/v1/organizations', signal)
				: deps.http.read('/api/v1/organizations'));
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
		} catch (err) {
			if (signal?.aborted) throw err;
			return false;
		}
	}

	async function queryCosts(
		organizationId: string,
		body: Record<string, unknown>,
		signal?: AbortSignal
	): Promise<Rec> {
		const path = `/api/v1/organizations/${encodeURIComponent(organizationId)}/costs/query`;
		let reply: GatewayReply;
		try {
			reply = await (signal ? deps.http.submit(path, body, signal) : deps.http.submit(path, body));
		} catch (err) {
			if (signal?.aborted) throw err;
			throw {
				source: 'api',
				code: 'unavailable',
				message: 'Cost data could not be reached.'
			} satisfies AppError;
		}
		return unwrapGatewayResult(reply, 'Cost data could not be loaded.');
	}

	function toResolvedPeriod(result: Rec, period: PeriodRequest): ResolvedPeriod {
		const endsAt = asStr(result.ends_at) ?? deps.now();
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
			checkpointAt: asStr(summary?.coverage_as_of) ?? asStr(result.ends_at) ?? deps.now(),
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

	return { loadSnapshot, isOwnerOrAdmin, queryCosts, toResolvedPeriod, toCoverage, queryBody };
}
