// A project's investment (costs-and-insights-ux decision 6, §1 "Projects list"): its total cost to date with this
// month beside it, read through the costs module's public API from the same effective ledger as every other cost
// figure. Amounts reach only the viewers the spec shows them to: anyone else is refused, and the screens then show no
// amount at all (§1 "Who sees amounts"). Unknown stays "—" with its reason; incomplete totals say so.
import {
	costsApi,
	formatCost,
	incompleteText,
	scoped,
	unknownCostReason,
	type BreakdownRow,
	type Coverage,
	type Scoped,
	type UsageTotals
} from '$lib/modules/costs';
import { Resource } from '$lib/components/resource.svelte';
import { accountZone } from '$lib/modules/preferences/account-zone.svelte';

export interface Investment {
	toDate: UsageTotals;
	thisMonth?: UsageTotals;
}

export interface InvestmentByProject {
	byProject: Map<string, Investment>;
	coverage: Coverage;
}

const projectRows = (rows: BreakdownRow[]) =>
	new Map(rows.flatMap((r) => (r.projectId ? [[r.projectId, r.totals] as const] : [])));

/** Both periods in one read. Refused (`allowed: false`) for viewers who may not see amounts. */
export async function readInvestment(
	organizationId: string,
	timeZone: string
): Promise<Scoped<InvestmentByProject>> {
	return scoped(async () => {
		const [all, month] = await Promise.all([
			costsApi.organizationCost(organizationId, { kind: 'all_time', timeZone }),
			costsApi.organizationCost(organizationId, { kind: 'this_month', timeZone })
		]);
		const monthly = projectRows(month.byProject);
		const byProject = new Map<string, Investment>();
		for (const [id, toDate] of projectRows(all.byProject))
			byProject.set(id, { toDate, thisMonth: monthly.get(id) });
		return { byProject, coverage: all.coverage };
	});
}

export interface InvestmentWords {
	/** "≈ $1,240.00", "—" or "No usage yet". */
	toDate: string;
	/** "≈ $412.00 this month", absent when nothing was recorded this month. */
	thisMonth?: string;
	/** Why the figure is "—", or "Usage incomplete" (short, for a row). */
	note?: string;
	/** What an incomplete total leaves out, in full (for assistive technology and the Overview). */
	detail?: string;
}

const incomplete = (text: string | undefined) =>
	text ? { note: 'Usage incomplete', detail: text } : {};

/** A project with no records has spent nothing known: "No usage yet", never "$0.00" or a guess. */
export function investmentWords(i: Investment | undefined, coverage?: Coverage): InvestmentWords {
	if (!i || i.toDate.records === 0) return { toDate: 'No usage yet' };
	const unknown = i.toDate.knownCostMicroUsd === null;
	const month = i.thisMonth && i.thisMonth.records > 0 ? i.thisMonth : undefined;
	return {
		toDate: formatCost(i.toDate.knownCostMicroUsd),
		thisMonth: month ? `${formatCost(month.knownCostMicroUsd)} this month` : undefined,
		...(unknown
			? { note: unknownCostReason(i.toDate) }
			: incomplete(incompleteText(i.toDate, coverage)))
	};
}

/**
 * The investment read a Projects screen holds. Its states are its own: the list and the Overview stay usable while it
 * loads or fails. Periods resolve in the viewing account's zone.
 */
export function investmentResource(organizationId: () => string) {
	return new Resource(async () => {
		await accountZone.ensure(organizationId());
		return readInvestment(organizationId(), accountZone.zone);
	});
}
