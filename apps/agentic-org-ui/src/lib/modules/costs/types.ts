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

// ---- Project Insights, Organization Insights, and Member Cost (Step 7; costs-and-insights-ux §1, §3, §4) ----------

/** One bucket in a spend trend series (day, week, or month per §3 "By-period output"). */
export interface TrendBucket {
	key: string;
	/** Short readable axis label: "14 Nov", "W46", "Oct 2026". */
	label: string;
	/** RFC 3339 inclusive start. */
	startsAt: string;
	/** RFC 3339 exclusive end. */
	endsAt: string;
	/** Edge bucket clipped by period start/end or current time. */
	partial?: boolean;
	totals: UsageTotals;
}

/** Actionable insight sentence pointing at an authorized underlying item (decision 7, INS-3). */
export interface InsightNotice {
	id: string;
	kind: 'outlier' | 'waiting' | 'concentration' | 'speed';
	/** Amber ('owner') is reserved strictly for items waiting on or needing owner attention. */
	tone: 'owner' | 'neutral';
	text: string;
	actionLabel: string;
	href: string;
	/** Explicit sample size or coverage limitation. */
	limitation?: string;
}

/** Project Insights breakdown dimensions (INS-2). */
export type ProjectDimension = 'team' | 'consumer' | 'model' | 'task_type' | 'work_type';

/** Filter key identifying one Project Insights breakdown row for drill-down. */
export type ProjectBreakdownKey =
	| { dimension: 'team'; teamId: string }
	| { dimension: 'team'; notOnTask: true }
	| { dimension: 'consumer'; memberId: string }
	| { dimension: 'consumer'; oneOff: true }
	| { dimension: 'model'; provider: string | null; model: string | null }
	| { dimension: 'task_type'; taskType: string }
	| { dimension: 'work_type'; workType: 'build' | 'run' | 'not_recorded' }
	| { all: true };

export interface InsightBreakdownRow {
	id: string;
	key: ProjectBreakdownKey;
	label: string;
	subtitle?: string;
	memberId?: string;
	teamId?: string;
	archived?: boolean;
	overhead?: boolean;
	totals: UsageTotals;
	/** Completed work items in this row during the period; null when not applicable (e.g., overhead). */
	itemsDone: number | null;
	/** Known cost divided by items done; null when unknown or itemsDone === 0. */
	costPerTaskMicroUsd: number | null;
	/** Median lead time in seconds; null when unavailable or fewer than 5 items. */
	medianLeadSeconds: number | null;
	/** Median cycle time in seconds; null when unavailable or fewer than 5 items. */
	medianCycleSeconds: number | null;
	/** Completed sample count behind the medians. */
	sampleCount: number;
}

/** Release lifetime cap summary on Project Insights (§1, document 17). Release caps warn only, never hold. */
export interface ReleaseCapRow {
	releaseId: string;
	number: number;
	title: string;
	versionLabel?: string;
	unit: BudgetUnit;
	/** Cap limit in micro-USD or tokens; undefined when no cap is set. */
	capLimit?: number;
	/** Known spend attributed to this release (micro-USD or tokens matching `unit`). */
	spent: number;
	complete: boolean;
	/** RFC 3339 when released (succeeded production deployment). */
	releasedAt?: string;
	/** Retained checkpoint spend at release time; never recomputed from today's membership. */
	spentAtRelease?: number;
}

/** Speed summary row by work item kind (TIME-2: medians only, working time not tracked yet). */
export interface SpeedKindRow {
	kind: string;
	label: string;
	sampleCount: number;
	medianLeadSeconds: number | null;
	medianCycleSeconds: number | null;
	medianWaitingSeconds: number | null;
}

/** Project › Insights payload (costs-and-insights-ux §1, §3, INS-1..5). */
export interface ProjectInsights {
	project: { id: string; name: string; number: number; slug: string };
	period: ResolvedPeriod;
	previous?: { period: ResolvedPeriod; knownCostMicroUsd: number | null };
	/** Total cost to date leading the Cost section (decision 6, INS-5). */
	investment: {
		toDate: UsageTotals;
		sinceAt: string | null;
		tasksDone: number;
		bugsDone: number;
	};
	totals: UsageTotals;
	work: {
		tasksDone: number;
		bugsDone: number;
		requestsScoped: number;
		waitingOnYouSeconds: number | null;
		cancelledCostMicroUsd: number | null;
		cancelledRecords: number;
	};
	trend: TrendBucket[];
	insights: InsightNotice[];
	breakdowns: {
		byTeam: InsightBreakdownRow[];
		byConsumer: InsightBreakdownRow[];
		byModel: InsightBreakdownRow[];
		byTaskType: InsightBreakdownRow[];
		byWorkType: InsightBreakdownRow[];
	};
	releaseCaps: ReleaseCapRow[];
	speed: SpeedKindRow[];
	coverage: Coverage;
	viewer: { scope: 'organization' | 'managed'; canExport: boolean };
}

/** One project row on Organization Insights › Projects (§1, §3). */
export interface OrgInsightProjectRow {
	projectId: string;
	projectNumber: number;
	name: string;
	archived?: boolean;
	accountableTeam?: string;
	toDate: UsageTotals;
	thisMonth: UsageTotals;
	lastMonth: UsageTotals;
	periodTotals: UsageTotals;
	/** Percent change from last month to this month; null when either unknown or last month had no spend. */
	changePercent: number | null;
	itemsDone: number;
	medianLeadSeconds: number | null;
	sampleCount: number;
}

/** Organization Insights `/insights` payload (costs-and-insights-ux §1, §3). */
export interface OrganizationInsights {
	period: ResolvedPeriod;
	previous?: { period: ResolvedPeriod; knownCostMicroUsd: number | null };
	totals: UsageTotals;
	work: {
		tasksDone: number;
		bugsDone: number;
		requestsScoped: number;
		waitingOnYouSeconds: number | null;
		cancelledCostMicroUsd: number | null;
		cancelledRecords: number;
	};
	trend: TrendBucket[];
	insights: InsightNotice[];
	projects: OrgInsightProjectRow[];
	/** Owner-only org-level Not on a task bucket; absent for project managers. */
	notOnTask?: UsageTotals;
	coverage: Coverage;
	viewer: { scope: 'organization' | 'managed'; canExport: boolean };
}

/** Member profile › Cost breakdown by project (COST-7). */
export interface MemberProjectSpendRow {
	id: string;
	projectId?: string;
	label: string;
	archived?: boolean;
	/** True for a project's own Not on a task bucket, or the org-wide Not on a task bucket (owner only). */
	notOnTask?: boolean;
	totals: UsageTotals;
}

/** Member profile › Cost top tasks by cost (COST-7). */
export interface MemberTaskSpendRow {
	taskRef: string;
	title: string;
	projectId?: string;
	projectName?: string;
	totals: UsageTotals;
}

export type MemberRecordsFilter =
	| { kind: 'all' }
	| { kind: 'period'; period: 'today' | 'this_week' | 'this_month' | 'last_month' }
	| { kind: 'project'; projectId: string; notOnTask?: boolean }
	| { kind: 'org_not_on_task' }
	| { kind: 'task'; taskRef: string };

/** Member profile › Cost tab payload (costs-and-insights-ux §1 Agent profile › Cost, COST-7, §4). */
export interface MemberCostOverview {
	memberId: string;
	/** `organization` for owner; `managed` for a project manager (labelled "in your projects"). */
	scope: 'organization' | 'managed';
	timeZone: string;
	periods: {
		today: UsageTotals;
		thisWeek: UsageTotals;
		thisMonth: UsageTotals;
		lastMonth: UsageTotals;
	};
	byProject: MemberProjectSpendRow[];
	topTasks: MemberTaskSpendRow[];
	/** Owner only: daily and monthly calendar budgets in the budget zone. Omitted for project managers (COST-7). */
	budgets?: {
		zone: { name: string | null; source: 'owner_account' | 'hq' };
		daily?: BudgetPeriodState;
		monthly?: BudgetPeriodState;
		canManage: boolean;
		/** Neutral note after a hold releases on period reset; never promises execution resumed. */
		resetNote?: string;
	};
	coverage: Coverage;
}

/** Set or raise an agent's daily or monthly calendar budget (owner only, §4). */
export interface AgentBudgetDraft {
	periodKind: 'daily' | 'monthly';
	unit: BudgetUnit;
	/** Positive integer micro-USD or tokens matching `unit`. */
	limit: number;
	operationId: string;
}

/** Clear, extend once (+20%), or keep paused on a reached agent calendar budget (owner only, §4). */
export interface AgentBudgetAction {
	periodKind: 'daily' | 'monthly';
	action: 'clear' | 'allow_once' | 'keep_paused';
	reason?: string;
	operationId: string;
}

export type AgentBudgetOperationOutcome =
	{ state: 'applied' } | { state: 'not_applied'; reason: string } | { state: 'pending' };

/** Export payload returned for CSV generation, obeying the same viewer scope as the screen. */
export interface UsageExportResult {
	period: ResolvedPeriod;
	records: UsageRecord[];
	coverage: Coverage;
}
