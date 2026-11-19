// The cost screens' words, one function per fact, so the copy is testable (costs-and-insights-ux §0, §1, §4, §5).
import { dayKey, formatDay, formatDayKey, formatDayTime, isDayKey } from './dates';
import { formatBudgetAmount, formatCost } from './money';
import type {
	AttributionBasis,
	BudgetPeriodState,
	Coverage,
	CostBasis,
	PeriodKind,
	PriceVersion,
	ResolvedPeriod,
	UsageRecord,
	UsageTotals
} from './types';

export const PERIODS: { kind: PeriodKind; label: string }[] = [
	{ kind: 'this_week', label: 'This week' },
	{ kind: 'this_month', label: 'This month' },
	{ kind: 'last_month', label: 'Last month' },
	{ kind: 'custom', label: 'Custom' },
	{ kind: 'all_time', label: 'All time' }
];

export const periodLabel = (kind: PeriodKind) =>
	PERIODS.find((p) => p.kind === kind)?.label ?? 'This month';

const plural = (n: number, one: string, many = `${one}s`) =>
	`${new Intl.NumberFormat('en-US').format(n)} ${n === 1 ? one : many}`;

/** "1 Nov 2026 – now", "1 Oct – 31 Oct 2026", "Everything recorded"; the zone is said by the caller. */
export function periodRange(period: ResolvedPeriod): string {
	const zone = period.timeZone;
	const end = period.toNow
		? 'now'
		: formatDay(new Date(new Date(period.endsAt).getTime() - 1), zone);
	if (!period.startsAt) return period.toNow ? 'Everything recorded up to now' : `Up to ${end}`;
	const start = formatDay(new Date(period.startsAt), zone);
	if (end === start) return start;
	// One year written once when both ends share it: "1 Oct – 31 Oct 2026".
	const startYear = start.slice(-4);
	if (!period.toNow && end.endsWith(startYear)) return `${start.slice(0, -5)} – ${end}`;
	return `${start} – ${end}`;
}

/**
 * The change from the previous period of the same length, in words: "18% more than 1 Oct – 19 Oct 2026". No
 * comparison when either side is unknown or the previous period had no known spend.
 */
export function changeText(
	current: number | null,
	previous: { period: ResolvedPeriod; knownCostMicroUsd: number | null } | undefined
): string | undefined {
	if (current === null || !previous || previous.knownCostMicroUsd === null) return undefined;
	const range = periodRange(previous.period);
	const before = previous.knownCostMicroUsd;
	if (before === 0) return current === 0 ? `No spend in ${range} either` : `No spend in ${range}`;
	const percent = Math.round(((current - before) * 100) / before);
	if (percent === 0) return `About the same as ${range}`;
	return percent > 0
		? `${percent}% more than ${range}`
		: `${Math.abs(percent)}% less than ${range}`;
}

/**
 * The qualification every affected total repeats (decision 2, COST-4, INS-4): "Usage incomplete: excludes 5 unpriced
 * records · 3 records with partial usage". Undefined when the totals are complete.
 */
export function incompleteText(totals: UsageTotals, coverage?: Coverage): string | undefined {
	const parts: string[] = [];
	if (totals.unpricedRecords > 0)
		parts.push(`excludes ${plural(totals.unpricedRecords, 'unpriced record')}`);
	if (totals.unknownCostRecords > 0)
		parts.push(`${plural(totals.unknownCostRecords, 'record')} with unknown cost`);
	const partialCost =
		totals.costIncompleteRecords - totals.unpricedRecords - totals.unknownCostRecords;
	if (partialCost > 0) parts.push(`${plural(partialCost, 'record')} with a partial cost`);
	if (totals.tokenIncompleteRecords > 0)
		parts.push(`${plural(totals.tokenIncompleteRecords, 'record')} without full token counts`);
	if (coverage && coverage.gaps.length > 0)
		parts.push(`${plural(coverage.gaps.length, 'usage source')} not fully reported`);
	return parts.length ? `Usage incomplete: ${parts.join(' · ')}` : undefined;
}

/** Why a known cost is missing: an all-unknown sum stays "—" and says so (decision 2). */
export function unknownCostReason(totals: UsageTotals): string {
	if (totals.records === 0) return 'No usage records';
	if (totals.unpricedRecords === totals.records)
		return `No known dollars: ${plural(totals.records, 'record')}, none priced`;
	if (totals.unknownCostRecords === totals.records)
		return `Cost unknown: the runtime reported no cost for ${plural(totals.records, 'record')}`;
	return `No known dollars: ${totals.unpricedRecords} unpriced, ${totals.unknownCostRecords} with unknown cost`;
}

/** "74% cache reads" when the share is known. */
export function cacheShareText(totals: UsageTotals): string | undefined {
	if (!totals.knownTokens || totals.cacheReadTokens === null) return undefined;
	return `${Math.round((totals.cacheReadTokens * 100) / totals.knownTokens)}% cache reads`;
}

/** A row's share of the period's known cost, as a whole percent; undefined when either is unknown or zero. */
export function shareOf(part: number | null, whole: number | null): number | undefined {
	if (part === null || whole === null || whole <= 0) return undefined;
	return Math.round((part * 100) / whole);
}

/** The cost tile's value with what it includes: "≈ $412.00" (decision 3). */
export function costValue(totals: UsageTotals): string {
	return formatCost(totals.knownCostMicroUsd);
}

/**
 * Not on a task's two kinds of overhead, kept apart (decision 4): "Team overhead: ≈ $3.10 · organization overhead:
 * none". A bucket with no records is "none", never "—" (which would read as unknown).
 */
export function overheadText(overhead: { team: UsageTotals; organization: UsageTotals }): string {
	const part = (t: UsageTotals) => (t.records === 0 ? 'none' : formatCost(t.knownCostMicroUsd));
	return `Team overhead: ${part(overhead.team)} · organization overhead: ${part(overhead.organization)}`;
}

export const API_EQUIVALENT_NOTE = 'Includes API-equivalent cost for subscription plans';

// ---- Usage records ------------------------------------------------------------------------------------------------

const BASIS: Record<CostBasis, string> = {
	runtime_estimate: 'Runtime estimate',
	token_estimate: 'Token estimate',
	unpriced: 'Unpriced',
	unknown: 'Unknown'
};

/** "Token estimate · API-equivalent". */
export function basisText(record: Pick<UsageRecord, 'costBasis' | 'apiEquivalent'>): string {
	return `${BASIS[record.costBasis]}${record.apiEquivalent ? ' · API-equivalent' : ''}`;
}

/** Why a record shows "—" for its cost, for assistive technology and the record's detail line. */
export function costDashReason(record: Pick<UsageRecord, 'costBasis'>): string {
	return record.costBasis === 'unpriced'
		? 'Unpriced: no rate for this model, so it adds no known dollars'
		: 'Cost unknown: the runtime reported no cost';
}

const ATTRIBUTION: Record<AttributionBasis, string> = {
	work_for: 'Work for',
	current_task: 'Current task',
	project_overhead: 'Project overhead',
	team_overhead: 'Team overhead',
	organization_overhead: 'Organization overhead'
};

/** Where a record's cost went and how it was attributed: "T-12 · work for", "Atlas · project overhead, not on a task". */
export function attributionText(a: UsageRecord['attribution']): string {
	switch (a.basis) {
		case 'work_for':
		case 'current_task':
			return `${a.task ? `${a.task.ref} ${a.task.title}` : 'A task'} · ${ATTRIBUTION[a.basis].toLowerCase()}`;
		case 'project_overhead':
			return `${a.project?.name ?? 'A project'} · project overhead, not on a task`;
		case 'team_overhead':
			return `${a.team?.name ?? 'A team'} · team overhead, no project`;
		case 'organization_overhead':
			return 'Organization overhead · no project or team recorded';
	}
}

const STATUS: Record<UsageRecord['tokenStatus'], string> = {
	complete: 'complete',
	partial: 'partial',
	unavailable: 'unavailable'
};

/** "Tokens complete · cost partial". */
export function completenessText(record: Pick<UsageRecord, 'tokenStatus' | 'costStatus'>): string {
	return `Tokens ${STATUS[record.tokenStatus]} · cost ${STATUS[record.costStatus]}`;
}

// ---- Budgets ------------------------------------------------------------------------------------------------------

export type BudgetTone = 'quiet' | 'owner';

/** The budget's state in words (document 17): reached waits on the owner, so it is the only amber state. */
export function budgetState(b: BudgetPeriodState): { word: string; tone: BudgetTone } | undefined {
	if (b.hold === 'stop_pending') return { word: 'Stopping · Budget reached', tone: 'owner' };
	if (b.hold === 'held') return { word: 'Paused · Budget reached', tone: 'owner' };
	if (b.spent >= b.limit) return { word: 'Budget reached', tone: 'owner' };
	if (b.spent * 5 >= b.limit * 4) return { word: '80% reached', tone: 'quiet' };
	return undefined;
}

/** Whole percent of the limit, floored, so 79.9% never reads as 80%. */
export const budgetPercent = (b: Pick<BudgetPeriodState, 'spent' | 'limit'>) =>
	Math.floor((b.spent * 100) / b.limit);

/** "≈ $7.87 of $9.00 · 87%", or for tokens "18.1 M of 25 M tokens · 72%". */
export function budgetAmountText(b: BudgetPeriodState): string {
	const spent =
		b.unit === 'usd'
			? `${b.complete ? '≈ ' : 'at least ≈ '}${formatBudgetAmount('usd', b.spent)}`
			: `${b.complete ? '' : 'at least '}${formatBudgetAmount('tokens', b.spent).replace(' tokens', '')}`;
	return `${spent} of ${formatBudgetAmount(b.unit, b.limit)} · ${budgetPercent(b)}%`;
}

/** "Today (America/New_York)" or "This month (America/New_York)": the budget's own zone, not the viewer's. */
export function budgetPeriodLabel(kind: 'daily' | 'monthly', zone: string): string {
	return `${kind === 'daily' ? 'Today' : 'This month'} (${zone})`;
}

/** "19 Nov 2026, 00:00 up to 20 Nov 2026, 00:00" in the budget zone (named in the label): inclusive start, exclusive end. */
export function budgetBoundsText(b: BudgetPeriodState, zone: string): string {
	return `${formatDayTime(new Date(b.period.startsAt), zone)} up to ${formatDayTime(new Date(b.period.endsAt), zone)}`;
}

/** "limit $20.00 + $4.00 = $24.00": the base and the one extension (document 17: L + floor(L/5)). */
export function extensionText(b: BudgetPeriodState): string {
	const base = b.baseLimit ?? b.limit;
	return `limit ${formatBudgetAmount(b.unit, base)} + ${formatBudgetAmount(b.unit, b.limit - base)} = ${formatBudgetAmount(b.unit, b.limit)}`;
}

export const NO_USAGE_RUNTIME =
	'This runtime does not report usage, so this budget cannot stop its unmeasured work.';

export const zoneSourceText = (source: 'owner_account' | 'hq') =>
	source === 'owner_account' ? "the owner's time zone" : "HQ's time zone";

// ---- Prices -------------------------------------------------------------------------------------------------------

export interface PriceGroup {
	provider: string;
	model: string;
	/** The version in effect now; absent when every version starts later. */
	current?: PriceVersion;
	/** Versions that start later, soonest first. */
	scheduled: PriceVersion[];
	/** Versions that were replaced, newest first. */
	earlier: PriceVersion[];
}

/** Price versions grouped by provider and model, A–Z, each split into current, scheduled and earlier at `now`. */
export function groupPrices(prices: PriceVersion[], now: Date): PriceGroup[] {
	const groups = new Map<string, PriceVersion[]>();
	for (const price of prices) {
		const key = `${price.provider}\u0000${price.model}`;
		groups.set(key, [...(groups.get(key) ?? []), price]);
	}
	return [...groups.values()]
		.map((versions) => {
			const sorted = versions.toSorted((a, b) => a.effectiveFrom.localeCompare(b.effectiveFrom));
			const started = sorted.filter((v) => new Date(v.effectiveFrom) <= now);
			const current = started.at(-1);
			return {
				provider: sorted[0].provider,
				model: sorted[0].model,
				...(current ? { current } : {}),
				scheduled: sorted.filter((v) => new Date(v.effectiveFrom) > now),
				earlier: started.slice(0, -1).reverse()
			};
		})
		.toSorted((a, b) => a.provider.localeCompare(b.provider) || a.model.localeCompare(b.model));
}

export const NO_PRICES = 'No prices set: unpriced records add no known dollars.';
export const UNKNOWN_RATE = 'No rate: records with these tokens are unpriced';

/** "From 1 Sep 2026" in the viewer's zone. */
export const effectiveText = (price: PriceVersion, zone: string) =>
	`From ${formatDay(new Date(price.effectiveFrom), zone)}`;

// ---- Custom period ------------------------------------------------------------------------------------------------

export type CustomErrors = { from?: string; to?: string };

/** Checks a custom range of whole dates (§3: From without To runs to now; To without From is refused). */
export function checkCustomRange(from: string, to: string, today: string): CustomErrors {
	const errors: CustomErrors = {};
	if (from === '') errors.from = 'Choose the first day.';
	else if (!isDayKey(from)) errors.from = 'Enter a date, such as 2026-11-01.';
	else if (from > today) errors.from = "The first day can't be after today.";
	if (to !== '') {
		if (!isDayKey(to)) errors.to = 'Enter a date, such as 2026-11-19.';
		else if (to > today) errors.to = "The last day can't be after today.";
		else if (!errors.from && to < from) errors.to = "The last day can't be before the first day.";
	}
	return errors;
}

/** Today's date key in the viewer's zone. */
export const todayKey = (zone: string, now = new Date()) => dayKey(now, zone);

/** "1 Nov 2026 – 19 Nov 2026" for the custom range a person chose. */
export const customRangeText = (from: string, to: string) =>
	to ? `${formatDayKey(from)} – ${formatDayKey(to)}` : `${formatDayKey(from)} – now`;
