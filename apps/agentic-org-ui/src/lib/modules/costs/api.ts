import { orgApi } from '$lib/modules/organization/api';
import { appSession } from '$lib/platform/auth/app-session.svelte';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { createHttpCostsReadApi } from './http-api';
import { createHttpSubscriptionUsageApi } from './subscription-http-api';
import type {
	AgentBudgetAction,
	AgentBudgetDraft,
	AgentBudgetOperationOutcome,
	AgentBudgets,
	BreakdownKey,
	BudgetPeriodState,
	MemberCostOverview,
	MemberRecordsFilter,
	OperationOutcome,
	OrganizationCost,
	OrganizationInsights,
	PeriodRequest,
	PriceDraft,
	PriceVersion,
	PricesOverview,
	ProjectBreakdownKey,
	ProjectInsights,
	ProjectInvestment,
	SubscriptionCollectResult,
	SubscriptionCredentialDraft,
	SubscriptionUsageCredential,
	SubscriptionUsageOverview,
	UsageExportResult,
	UsageRecordPage
} from './types';

/**
 * What the cost screens read and change (costs-and-insights-ux §1, §3, §5). Reads use the same effective-ledger query
 * and scoped authorization as every other cost reader; amounts are refused with `not_authorized` to anyone the spec
 * does not show them to. Refusals reject with an AppError; a refusal about one Add a rate field also carries `field`
 * (a `PriceField`). A write whose outcome is unknown (the connection dropped after sending) rejects with the code
 * `OUTCOME_UNKNOWN` (./outcome.ts) and is reconciled with `priceOperation` / `agentBudgetOperation` before anything
 * is repeated.
 */
export interface CostsApi {
	/** Every price version, with whether the viewer may add one. Readable by anyone signed in: rates are not spend. */
	prices(organizationId: string): Promise<PricesOverview>;
	/** Adds a price version. Idempotent on `draft.operationId`: a replay answers with the version it created. */
	addPrice(organizationId: string, draft: PriceDraft): Promise<PriceVersion>;
	/** Where an add whose outcome was unknown stands, by its operation ID. */
	priceOperation(organizationId: string, operationId: string): Promise<OperationOutcome>;
	/** Organization › Cost for a period (owner only; others get `not_authorized`). Invalid periods are refused. */
	organizationCost(organizationId: string, period: PeriodRequest): Promise<OrganizationCost>;
	/** The effective records behind one breakdown figure, newest first, a page at a time (owner only). */
	records(
		organizationId: string,
		period: PeriodRequest,
		key: BreakdownKey | { all: true },
		cursor?: string
	): Promise<UsageRecordPage>;
	/**
	 * Each project's investment, to date and this month (decision 6, §1 "Projects list"), for the projects whose amounts
	 * the viewer may see: every project for the owner, the projects of the teams a project manager leads for that
	 * manager (§1 "Who sees amounts"). HQ scopes the rows; a viewer who may see none is refused with `not_authorized`.
	 * This month resolves in `timeZone`, the viewing account's zone.
	 */
	projectInvestment(
		organizationId: string,
		request: { timeZone: string }
	): Promise<ProjectInvestment>;
	/** Every agent's daily and monthly budget in the budget zone (owner only). */
	agentBudgets(organizationId: string): Promise<AgentBudgets>;
	/** Project › Insights for a period (owner or authorized project manager; others get `not_authorized`). */
	projectInsights(
		organizationId: string,
		projectId: string,
		period: PeriodRequest
	): Promise<ProjectInsights>;
	/** Effective usage records behind a Project › Insights breakdown row, newest first, paginated. */
	projectRecords(
		organizationId: string,
		projectId: string,
		period: PeriodRequest,
		key: ProjectBreakdownKey,
		cursor?: string
	): Promise<UsageRecordPage>;
	/** Organization Insights `/insights` for a period (owner sees all; PM sees managed projects; others `not_authorized`). */
	organizationInsights(
		organizationId: string,
		period: PeriodRequest
	): Promise<OrganizationInsights>;
	/** Scoped effective usage records for safe CSV export (obeys the same viewer scope as the screen). */
	exportRecords(
		organizationId: string,
		scope: { kind: 'organization' } | { kind: 'project'; projectId: string },
		period: PeriodRequest
	): Promise<UsageExportResult>;
	/** Agent profile › Cost tab (owner sees full + budgets; PM sees own projects labelled "in your projects", no budgets). */
	memberCost(
		organizationId: string,
		memberId: string,
		request: { timeZone: string }
	): Promise<MemberCostOverview>;
	/** Effective usage records behind a Member profile › Cost figure, newest first, paginated. */
	memberRecords(
		organizationId: string,
		memberId: string,
		request: { timeZone: string; filter: MemberRecordsFilter },
		cursor?: string
	): Promise<UsageRecordPage>;
	/** Set or change an agent's daily or monthly calendar budget (owner only; idempotent on `operationId`). */
	setAgentBudget(
		organizationId: string,
		memberId: string,
		draft: AgentBudgetDraft
	): Promise<BudgetPeriodState>;
	/** Clear, allow 20% more once, or keep paused until reset on an agent budget (owner only). */
	agentBudgetAction(
		organizationId: string,
		memberId: string,
		action: AgentBudgetAction
	): Promise<{ message: string }>;
	/** Reconcile an agent budget write whose outcome was unknown. */
	agentBudgetOperation(
		organizationId: string,
		operationId: string
	): Promise<AgentBudgetOperationOutcome>;
	/** Owner-scoped provider-reported subscription model usage snapshots (TASK-79). */
	subscriptionUsage(organizationId: string): Promise<SubscriptionUsageOverview>;
	/** Trigger authenticated collection for one or all active provider subscriptions (TASK-79). */
	collectSubscriptionUsage(
		organizationId: string,
		options?: { connectionId?: string; timeZone?: string }
	): Promise<SubscriptionCollectResult>;
	/** Store a wrapped private usage-API credential outside org Git (TASK-79). */
	setSubscriptionUsageCredential(
		organizationId: string,
		draft: SubscriptionCredentialDraft
	): Promise<SubscriptionUsageCredential>;
	/** Revoke a private usage-API credential from office storage (TASK-79). */
	revokeSubscriptionUsageCredential(
		organizationId: string,
		connectionId: string
	): Promise<SubscriptionUsageCredential>;
}

function signedInSession() {
	return appSession.state.kind === 'signed-in' ? appSession.state.session : undefined;
}

const gatewayHttp = {
	read: (path: string, signal?: AbortSignal) => appSession.controller().read(path, signal),
	submit: (path: string, body: unknown, signal?: AbortSignal) =>
		appSession.controller().submit(path, body, signal)
};

const gatewayCostsReadApi = createHttpCostsReadApi({
	http: gatewayHttp,
	orgApi
});

const gatewaySubscriptionUsageApi = createHttpSubscriptionUsageApi({
	http: gatewayHttp
});

// Organization cost summary, agent budgets, and subscription usage (TASK-79) are wired to the
// authenticated gateway and fail closed when signed out. Remaining cost/insights routes stay
// unavailable until their full gateway adapters land. Under `bun run dev:mock`,
// `e2e/fakes/modules/costs.ts` stands in for this file.
export const costsApi: CostsApi = {
	prices: unavailableCall,
	addPrice: unavailableCall,
	priceOperation: unavailableCall,
	organizationCost: (organizationId, period) =>
		signedInSession()
			? gatewayCostsReadApi.organizationCost(organizationId, period)
			: unavailableCall(),
	records: unavailableCall,
	projectInvestment: unavailableCall,
	agentBudgets: (organizationId) =>
		signedInSession() ? gatewayCostsReadApi.agentBudgets(organizationId) : unavailableCall(),
	projectInsights: unavailableCall,
	projectRecords: unavailableCall,
	organizationInsights: unavailableCall,
	exportRecords: unavailableCall,
	memberCost: unavailableCall,
	memberRecords: unavailableCall,
	setAgentBudget: unavailableCall,
	agentBudgetAction: unavailableCall,
	agentBudgetOperation: unavailableCall,
	subscriptionUsage: (organizationId) =>
		signedInSession()
			? gatewaySubscriptionUsageApi.subscriptionUsage(organizationId)
			: unavailableCall(),
	collectSubscriptionUsage: (organizationId, options) =>
		signedInSession()
			? gatewaySubscriptionUsageApi.collectSubscriptionUsage(organizationId, options)
			: unavailableCall(),
	setSubscriptionUsageCredential: (organizationId, draft) =>
		signedInSession()
			? gatewaySubscriptionUsageApi.setSubscriptionUsageCredential(organizationId, draft)
			: unavailableCall(),
	revokeSubscriptionUsageCredential: (organizationId, connectionId) =>
		signedInSession()
			? gatewaySubscriptionUsageApi.revokeSubscriptionUsageCredential(organizationId, connectionId)
			: unavailableCall()
};
