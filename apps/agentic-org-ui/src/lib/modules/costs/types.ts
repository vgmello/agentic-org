// What the cost screens read and change. Shapes follow the accounting contract
// (docs/design/organization-data/10-usage-and-cost-accounting.md: `usage_records`, `model_prices`, the effective-ledger
// totals) and the budget contract (17-budgets-and-controls.md), in the browser's camelCase. Money is integer micro-USD
// (0.000001 USD) everywhere; the screens never do float arithmetic on stored amounts. `null` means unknown, never zero.

/** How a usage record's cost was valued (`cost_basis`). */
export type CostBasis = 'runtime_estimate' | 'token_estimate' | 'unpriced' | 'unknown';

/** Token and cost completeness are independent (`token_status`, `cost_status`). */
export type Completeness = 'complete' | 'partial' | 'unavailable';

/** The four disjoint token categories, plus reasoning as a subset of output (never another charge). */
export interface TokenCounts {
	input: number | null;
	output: number | null;
	cacheRead: number | null;
	cacheWrite: number | null;
	/** A subset of `output`. */
	reasoning: number | null;
}

/**
 * Derived totals over a set of effective usage records (one ledger query, COST-4). Sums stay `null` when every value is
 * unknown; a partly known sum is a lower bound and the counts say what is missing.
 */
export interface UsageTotals {
	/** Effective records counted. */
	records: number;
	/** Integer micro-USD; null when no record in the set has a known cost. */
	knownCostMicroUsd: number | null;
	/** Input + output + cache read + cache write, once; null when no record has known tokens. */
	knownTokens: number | null;
	cacheReadTokens: number | null;
	/** Records whose token counts are partial or unavailable. */
	tokenIncompleteRecords: number;
	/** Records whose cost is partial or unavailable (unpriced and unknown included). */
	costIncompleteRecords: number;
	/** Records valued `unpriced`: no rate for their model, so they add no known dollars. Adding a price can fix them. */
	unpricedRecords: number;
	/** Records valued `unknown`: the runtime reported no cost. A price cannot fix them. */
	unknownCostRecords: number;
	/** Some records are valued at API-equivalent prices for a subscription plan. */
	includesApiEquivalent: boolean;
}

/** A usage stream the ledger has not heard from in full: totals are incomplete until it reports. */
export interface CoverageGap {
	/** "Studio Mac office" */
	source: string;
	/** Why: "not reported since 13:40", "Antigravity doesn't report usage". */
	reason: string;
	/** RFC 3339: the last checkpoint received from it, when known. */
	lastCheckpointAt?: string;
}

/** What the totals were computed against: the verified ledger checkpoint and the streams missing from it. */
export interface Coverage {
	/** RFC 3339. */
	checkpointAt: string;
	gaps: CoverageGap[];
}

/** The UI's cost periods (costs-and-insights-ux §1). This month is the initial one. */
export type PeriodKind = 'this_week' | 'this_month' | 'last_month' | 'custom' | 'all_time';

/** A period as asked for: calendar periods resolve in `timeZone`; custom takes inclusive whole dates. */
export interface PeriodRequest {
	kind: PeriodKind;
	/** `YYYY-MM-DD`, custom only, inclusive. */
	from?: string;
	/** `YYYY-MM-DD`, custom only, inclusive; missing means up to now. */
	to?: string;
	/** The viewing account's IANA zone. */
	timeZone: string;
}

/** A period as the server resolved it: half-open instant bounds in the zone it was asked in. */
export interface ResolvedPeriod {
	kind: PeriodKind;
	/** RFC 3339, inclusive; absent for All time. */
	startsAt?: string;
	/** RFC 3339, exclusive: now for a period still running. */
	endsAt: string;
	timeZone: string;
	/** The period still runs: its end is the captured now. */
	toNow: boolean;
}

/** How a breakdown groups the organization's records. */
export type Dimension = 'project' | 'consumer' | 'work_type';

/** What a breakdown row stands for; the records sheet asks for exactly these records. */
export type BreakdownKey =
	| { dimension: 'project'; projectId: string }
	/** The organization's Not on a task line: records with no project, team overhead and organization overhead. */
	| { dimension: 'project'; notOnTask: true }
	| { dimension: 'consumer'; memberId: string }
	| { dimension: 'consumer'; oneOff: true }
	| { dimension: 'work_type'; workType: 'build' | 'run' | 'not_recorded' };

export interface BreakdownRow {
	/** Stable within the breakdown. */
	id: string;
	key: BreakdownKey;
	label: string;
	/** Set for a project or an agent: the screens link it. */
	projectId?: string;
	memberId?: string;
	/** The record is an archived project or a former employee: still counted, named as it was. */
	archived?: boolean;
	totals: UsageTotals;
	/** Not on a task only: the classification kept apart (decision 4). */
	overhead?: { team: UsageTotals; organization: UsageTotals };
}

/** Organization › Cost for one period (owner only). */
export interface OrganizationCost {
	period: ResolvedPeriod;
	totals: UsageTotals;
	/** The previous period of the same length; absent for All time. */
	previous?: { period: ResolvedPeriod; knownCostMicroUsd: number | null };
	byProject: BreakdownRow[];
	byConsumer: BreakdownRow[];
	byWorkType: BreakdownRow[];
	coverage: Coverage;
}

/** One project's investment (decision 6): what was spent on it to date and in the current month. */
export interface ProjectInvestmentRow {
	projectId: string;
	/** All time: every effective record attributed to the project, overhead included. */
	toDate: UsageTotals;
	thisMonth: UsageTotals;
}

/** The projects whose amounts the viewer may see, and nothing else (§1 "Who sees amounts"). */
export interface ProjectInvestment {
	/** `organization` for the owner (every project); `managed` for a project manager (the projects they lead). */
	scope: 'organization' | 'managed';
	/** The current month as resolved in the zone asked for. */
	thisMonth: ResolvedPeriod;
	projects: ProjectInvestmentRow[];
	coverage: Coverage;
}

/** How a record's work was attributed when it was consumed (decision 4). */
export type AttributionBasis =
	'work_for' | 'current_task' | 'project_overhead' | 'team_overhead' | 'organization_overhead';

/** One effective usage record (COST-1). */
export interface UsageRecord {
	id: string;
	/** RFC 3339. */
	occurredAt: string;
	consumer:
		| { kind: 'agent'; memberId: string; name: string; former?: boolean }
		| { kind: 'one_off'; label: string };
	runtime: string;
	provider: string | null;
	model: string | null;
	tokens: TokenCounts | null;
	tokenStatus: Completeness;
	costMicroUsd: number | null;
	costBasis: CostBasis;
	costStatus: Completeness;
	/** Valued at API-equivalent prices for a subscription plan. */
	apiEquivalent: boolean;
	/** The exact price version used, when there was one. */
	price?: { id: string; label: string };
	attribution: {
		basis: AttributionBasis;
		task?: { ref: string; title: string };
		project?: { id: string; name: string };
		team?: { id: string; name: string };
	};
	workType: 'build' | 'run' | null;
	/** Why the record is incomplete, in words, when it is. */
	issue?: string;
	/** This record replaced another (an attribution correction): who, when and why. */
	correction?: { reason: string; byName: string; at: string };
}

export interface UsageRecordPage {
	records: UsageRecord[];
	/** All effective records behind the figure. */
	total: number;
	/** Pass back to read the next page; absent on the last page. */
	nextCursor?: string;
}

/** One price version (`model_prices`): integer micro-USD per million tokens; null is an unknown rate, not free. */
export interface PriceVersion {
	id: string;
	provider: string;
	model: string;
	/** RFC 3339: the instant the rate takes effect. */
	effectiveFrom: string;
	inputMicroUsdPerMillion: number | null;
	outputMicroUsdPerMillion: number | null;
	cacheReadMicroUsdPerMillion: number | null;
	cacheWriteMicroUsdPerMillion: number | null;
	/** Usage records reference it: it can never change or be removed. */
	used: boolean;
	created: { at: string; byName?: string };
}

export interface PricesOverview {
	prices: PriceVersion[];
	/** The owner manages prices; everyone else reads them. The server authorizes every change regardless. */
	viewer: { canManage: boolean };
}

/** A new price version. The date is a whole day in `timeZone`; the server stores the instant it starts. */
export interface PriceDraft {
	provider: string;
	model: string;
	inputMicroUsdPerMillion: number | null;
	outputMicroUsdPerMillion: number | null;
	cacheReadMicroUsdPerMillion: number | null;
	cacheWriteMicroUsdPerMillion: number | null;
	/** `YYYY-MM-DD`, today or later in `timeZone`. */
	effectiveFrom: string;
	timeZone: string;
	/** One per attempt: a retry after a verified failure reuses it; an unknown outcome is checked with it. */
	operationId: string;
}

/** Where an add-a-rate operation stands, when its outcome was not confirmed. */
export type OperationOutcome =
	| { state: 'applied'; price: PriceVersion }
	| { state: 'not_applied'; reason: string }
	| { state: 'pending' };

/** A refusal tied to one field of Add a rate. */
export type PriceField = 'provider' | 'model' | 'rates' | 'effectiveFrom';

export type BudgetUnit = 'usd' | 'tokens';

/** One agent calendar budget in its current period (document 17). */
export interface BudgetPeriodState {
	unit: BudgetUnit;
	/** The effective limit in integer micro-USD or tokens, as `unit` says. Positive. */
	limit: number;
	/** Known spend in the period; a lower bound when `complete` is false. */
	spent: number;
	complete: boolean;
	/** Why spend is incomplete, in words: "Antigravity doesn't report usage". */
	incompleteReason?: string;
	/** The agent runs on a runtime that reports no usage: this budget cannot stop its unmeasured work. */
	unmeasured?: boolean;
	/** Key and bounds in the budget zone: `2026-11-19` or `2026-11`. */
	period: { key: string; startsAt: string; endsAt: string };
	/** Reached: stop_pending until stopping is verified, then held. */
	hold?: 'stop_pending' | 'held';
	/** The one 20% extension was used in this period: `limit` is then the effective limit, base + floor(base / 5). */
	extended?: boolean;
	/** The policy's own limit, set when it differs from `limit` (after the extension). */
	baseLimit?: number;
	/** The 80% notice went out in this period. */
	warned?: boolean;
}

export interface AgentBudget {
	memberId: string;
	name: string;
	former?: boolean;
	daily?: BudgetPeriodState;
	monthly?: BudgetPeriodState;
}

export interface AgentBudgets {
	/** The zone agent budgets run in, and where it comes from; `name` is null while it is not known. */
	zone: { name: string | null; source: 'owner_account' | 'hq' };
	agents: AgentBudget[];
}

/** What the records sheet lists: one breakdown figure, or every record in the period. */
export interface RecordsTarget {
	key: BreakdownKey | { all: true };
	/** "Atlas", "Sam Okafor", "Build": named in the sheet's title. */
	label: string;
}
