import { unavailableCall } from '$lib/platform/unavailable-api';
import type {
	AgentBudgets,
	BreakdownKey,
	OperationOutcome,
	OrganizationCost,
	PeriodRequest,
	PriceDraft,
	PriceVersion,
	PricesOverview,
	ProjectInvestment,
	UsageRecordPage
} from './types';

/**
 * What the cost screens read and change (costs-and-insights-ux §1, §3, §5). Reads use the same effective-ledger query
 * and scoped authorization as every other cost reader; amounts are refused with `not_authorized` to anyone the spec
 * does not show them to. Refusals reject with an AppError; a refusal about one Add a rate field also carries `field`
 * (a `PriceField`). A write whose outcome is unknown (the connection dropped after sending) rejects with the code
 * `OUTCOME_UNKNOWN` (./outcome.ts) and is reconciled with `priceOperation` before anything is repeated.
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
}

// The web API does not serve costs yet: every call refuses, and the page says so.
// Under `bun run dev:mock`, e2e/fakes/modules/costs.ts stands in for this file.
export const costsApi: CostsApi = {
	prices: unavailableCall,
	addPrice: unavailableCall,
	priceOperation: unavailableCall,
	organizationCost: unavailableCall,
	records: unavailableCall,
	projectInvestment: unavailableCall,
	agentBudgets: unavailableCall
};
