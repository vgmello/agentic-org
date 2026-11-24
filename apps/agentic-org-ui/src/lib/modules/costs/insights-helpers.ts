// Pure sorting, filtering, and presentation helpers for Project Insights and Organization Insights (Step 7).
import { formatDay } from './dates';
import { WORKING_TIME_NOT_TRACKED } from './duration';
import { formatBudgetAmount, formatCost, formatCount } from './money';
import type {
	InsightBreakdownRow,
	OrgInsightProjectRow,
	ProjectInsights,
	ReleaseCapRow
} from './types';

export type SortDir = 'asc' | 'desc';

export type BreakdownSortKey =
	'label' | 'cost' | 'tokens' | 'itemsDone' | 'costPerTask' | 'medianLead';

export type OrgProjectSortKey =
	'name' | 'toDate' | 'thisMonth' | 'lastMonth' | 'change' | 'itemsDone';

function compareNullable(a: number | null, b: number | null, dir: SortDir): number {
	if (a === null && b === null) return 0;
	// Always sort unknown/null values last regardless of direction so real numbers lead.
	if (a === null) return 1;
	if (b === null) return -1;
	return dir === 'asc' ? a - b : b - a;
}

/** Filters and sorts Project Insights breakdown rows, keeping overhead ("Not on a task") pinned last unless filtered. */
export function filterAndSortBreakdowns(
	rows: readonly InsightBreakdownRow[],
	query: string,
	sortKey: BreakdownSortKey,
	dir: SortDir
): InsightBreakdownRow[] {
	const needle = query.trim().toLowerCase();
	const filtered = needle
		? rows.filter(
				(r) =>
					r.label.toLowerCase().includes(needle) ||
					(r.subtitle?.toLowerCase().includes(needle) ?? false)
			)
		: [...rows];
	return filtered.toSorted((a, b) => {
		if (!!a.overhead !== !!b.overhead) return a.overhead ? 1 : -1;
		let cmp = 0;
		switch (sortKey) {
			case 'label':
				cmp =
					dir === 'asc'
						? a.label.localeCompare(b.label, 'en', { sensitivity: 'base' })
						: b.label.localeCompare(a.label, 'en', { sensitivity: 'base' });
				break;
			case 'cost':
				cmp = compareNullable(a.totals.knownCostMicroUsd, b.totals.knownCostMicroUsd, dir);
				break;
			case 'tokens':
				cmp = compareNullable(a.totals.knownTokens, b.totals.knownTokens, dir);
				break;
			case 'itemsDone':
				cmp = compareNullable(a.itemsDone, b.itemsDone, dir);
				break;
			case 'costPerTask':
				cmp = compareNullable(a.costPerTaskMicroUsd, b.costPerTaskMicroUsd, dir);
				break;
			case 'medianLead':
				cmp = compareNullable(a.medianLeadSeconds, b.medianLeadSeconds, dir);
				break;
		}
		return cmp || a.label.localeCompare(b.label, 'en', { sensitivity: 'base' });
	});
}

/** Filters and sorts Organization Insights project rows. */
export function filterAndSortOrgProjects(
	rows: readonly OrgInsightProjectRow[],
	query: string,
	includeArchived: boolean,
	sortKey: OrgProjectSortKey,
	dir: SortDir
): OrgInsightProjectRow[] {
	const needle = query.trim().toLowerCase();
	const filtered = rows.filter((r) => {
		if (!includeArchived && r.archived) return false;
		if (!needle) return true;
		return (
			r.name.toLowerCase().includes(needle) ||
			(r.accountableTeam?.toLowerCase().includes(needle) ?? false) ||
			`p-${r.projectNumber}`.includes(needle)
		);
	});
	return filtered.toSorted((a, b) => {
		let cmp = 0;
		switch (sortKey) {
			case 'name':
				cmp =
					dir === 'asc'
						? a.name.localeCompare(b.name, 'en', { sensitivity: 'base' })
						: b.name.localeCompare(a.name, 'en', { sensitivity: 'base' });
				break;
			case 'toDate':
				cmp = compareNullable(a.toDate.knownCostMicroUsd, b.toDate.knownCostMicroUsd, dir);
				break;
			case 'thisMonth':
				cmp = compareNullable(a.thisMonth.knownCostMicroUsd, b.thisMonth.knownCostMicroUsd, dir);
				break;
			case 'lastMonth':
				cmp = compareNullable(a.lastMonth.knownCostMicroUsd, b.lastMonth.knownCostMicroUsd, dir);
				break;
			case 'change':
				cmp = compareNullable(a.changePercent, b.changePercent, dir);
				break;
			case 'itemsDone':
				cmp = dir === 'asc' ? a.itemsDone - b.itemsDone : b.itemsDone - a.itemsDone;
				break;
		}
		return cmp || a.name.localeCompare(b.name, 'en', { sensitivity: 'base' });
	});
}

/**
 * Investment lead sentence on Project › Insights (doc-51 §3 "Flow · Investment and export"):
 * "Atlas has cost ≈ $1,240.00 so far (since 7 Nov 2026) · agent work: — (not tracked yet) · 18 tasks and 4 bugs done"
 */
export function investmentLeadText(insights: ProjectInsights): string {
	const { project, investment, period } = insights;
	const since = investment.sinceAt
		? ` (since ${formatDay(new Date(investment.sinceAt), period.timeZone)})`
		: '';
	const tasksWord = investment.tasksDone === 1 ? 'task' : 'tasks';
	const bugsWord = investment.bugsDone === 1 ? 'bug' : 'bugs';
	return `${project.name} has cost ${formatCost(investment.toDate.knownCostMicroUsd)} so far${since} · agent work: — (${WORKING_TIME_NOT_TRACKED}) · ${formatCount(investment.tasksDone)} ${tasksWord} and ${formatCount(investment.bugsDone)} ${bugsWord} done`;
}

/** Release cap line (doc-51 §1 Release header): neutral styling even when over cap. */
export function releaseCapText(row: ReleaseCapRow): string {
	const spentStr =
		row.unit === 'usd'
			? `≈ ${formatBudgetAmount('usd', row.spent)}`
			: formatBudgetAmount('tokens', row.spent);
	const base =
		row.capLimit !== undefined
			? `Spent ${spentStr} of ${formatBudgetAmount(row.unit, row.capLimit)} cap`
			: `Spent ${spentStr}`;
	if (row.releasedAt && row.spentAtRelease !== undefined) {
		const atRelease =
			row.unit === 'usd'
				? `≈ ${formatBudgetAmount('usd', row.spentAtRelease)}`
				: formatBudgetAmount('tokens', row.spentAtRelease);
		return `${base} · Spent at release ${atRelease}`;
	}
	return base;
}

/** Formatted change cell ("+18%", "−6%", "0%", or "—" when unknown). */
export function formatChangePercent(pct: number | null): string {
	if (pct === null) return '—';
	if (pct > 0) return `+${pct}%`;
	if (pct < 0) return `−${Math.abs(pct)}%`;
	return '0%';
}
