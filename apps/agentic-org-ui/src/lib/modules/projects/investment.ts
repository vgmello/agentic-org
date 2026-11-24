// A project's investment (costs-and-insights-ux decision 6, §1 "Projects list"): its total cost to date with this
// month beside it, read through the costs module's project-level read from the same effective ledger as every other
// cost figure. Amounts reach only the viewers the spec shows them to (§1 "Who sees amounts"): the owner sees every
// project, a project manager the projects they lead, anyone else none — HQ scopes the rows and the screens show no
// amount where none came. Unknown stays "—" with its reason; every incomplete total says so, this month's included.
import {
	API_EQUIVALENT_NOTE,
	costsApi,
	formatCost,
	incompleteText,
	scoped,
	unknownCostReason,
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
	/** Only the projects whose amounts the viewer may see. */
	byProject: Map<string, Investment>;
	/** `managed`: a project manager, who sees some projects' amounts and not others'. */
	scope: 'organization' | 'managed';
	coverage: Coverage;
}

/** One read. Refused (`allowed: false`) for viewers who may see no project's amounts. */
export async function readInvestment(
	organizationId: string,
	timeZone: string
): Promise<Scoped<InvestmentByProject>> {
	return scoped(async () => {
		const read = await costsApi.projectInvestment(organizationId, { timeZone });
		return {
			byProject: new Map(
				read.projects.map((p) => [p.projectId, { toDate: p.toDate, thisMonth: p.thisMonth }])
			),
			scope: read.scope,
			coverage: read.coverage
		};
	});
}

export interface InvestmentWords {
	/** "≈ $1,240.00", "—", "No usage yet" or "Not shown". */
	toDate: string;
	/** "≈ $412.00 this month", "— this month"; absent when nothing was recorded this month. */
	thisMonth?: string;
	/** Why the total is "—", or "Usage incomplete" (short, for a row). */
	note?: string;
	/** What an incomplete total leaves out, or why an amount isn't shown, in full. */
	detail?: string;
}

export const NOT_SHOWN = "Only the organization's owner and the project's manager see its cost.";

/** Words for a project's investment; `undefined` with `visible` false means the viewer may not see this one. */
export function investmentWords(
	i: Investment | undefined,
	coverage?: Coverage,
	visible = true
): InvestmentWords {
	if (!visible) return { toDate: 'Not shown', detail: NOT_SHOWN };
	if (!i || i.toDate.records === 0) return { toDate: 'No usage yet' };
	const month = i.thisMonth && i.thisMonth.records > 0 ? i.thisMonth : undefined;
	const monthIncomplete = month && monthQualifier(month, coverage);
	const total = qualifier(i.toDate, coverage);
	return {
		toDate: formatCost(i.toDate.knownCostMicroUsd),
		thisMonth: month
			? `${formatCost(month.knownCostMicroUsd)} this month${monthIncomplete ? `, ${monthIncomplete}` : ''}`
			: undefined,
		...total
	};
}

function sumNullable(values: (number | null)[]): number | null {
	let sum = 0;
	let known = false;
	for (const v of values) {
		if (v !== null) {
			sum += v;
			known = true;
		}
	}
	return known ? sum : null;
}

function sumUsageTotals(list: UsageTotals[]): UsageTotals {
	return {
		records: list.reduce((acc, t) => acc + t.records, 0),
		knownCostMicroUsd: sumNullable(list.map((t) => t.knownCostMicroUsd)),
		knownTokens: sumNullable(list.map((t) => t.knownTokens)),
		cacheReadTokens: sumNullable(list.map((t) => t.cacheReadTokens)),
		tokenIncompleteRecords: list.reduce((acc, t) => acc + t.tokenIncompleteRecords, 0),
		costIncompleteRecords: list.reduce((acc, t) => acc + t.costIncompleteRecords, 0),
		unpricedRecords: list.reduce((acc, t) => acc + t.unpricedRecords, 0),
		unknownCostRecords: list.reduce((acc, t) => acc + t.unknownCostRecords, 0),
		includesApiEquivalent: list.some((t) => t.includesApiEquivalent)
	};
}

export interface PortfolioPeriodWords {
	/** "≈ $1,240.50", "—" or "No usage yet". */
	amount: string;
	/** Exact period-specific qualification (unknown cost reason or incomplete counts + coverage gaps). */
	qualification?: string;
	/** Canonical API-equivalent note when subscription-derived usage is included in this period. */
	apiEquivalent?: string;
}

export interface PortfolioInvestmentWords {
	toDate: PortfolioPeriodWords;
	/** Absent when nothing was recorded in the current month. */
	thisMonth?: PortfolioPeriodWords;
}

const EMPTY_TOTALS: UsageTotals = {
	records: 0,
	knownCostMicroUsd: null,
	knownTokens: null,
	cacheReadTokens: null,
	tokenIncompleteRecords: 0,
	costIncompleteRecords: 0,
	unpricedRecords: 0,
	unknownCostRecords: 0,
	includesApiEquivalent: false
};

function periodWords(totals: UsageTotals, coverage?: Coverage): PortfolioPeriodWords {
	if (totals.records === 0) {
		const gapQualification = incompleteText(totals, coverage);
		return {
			amount: 'No usage yet',
			...(gapQualification ? { qualification: gapQualification } : {})
		};
	}
	let qualification: string | undefined;
	if (totals.knownCostMicroUsd === null) {
		const extraIncomplete = incompleteText(
			{
				...totals,
				unpricedRecords: 0,
				unknownCostRecords: 0,
				costIncompleteRecords: 0
			},
			coverage
		);
		qualification = [unknownCostReason(totals), extraIncomplete].filter(Boolean).join(' · ');
	} else {
		qualification = incompleteText(totals, coverage);
	}
	return {
		amount: formatCost(totals.knownCostMicroUsd),
		...(qualification ? { qualification } : {}),
		...(totals.includesApiEquivalent ? { apiEquivalent: API_EQUIVALENT_NOTE } : {})
	};
}

/** Combined organization-wide project investment summary (only when viewer has organization scope). */
export function portfolioInvestmentWords(
	amounts: InvestmentByProject | undefined
): PortfolioInvestmentWords | undefined {
	if (!amounts || amounts.scope !== 'organization') return undefined;
	const entries = [...amounts.byProject.values()];
	if (entries.length === 0) return { toDate: periodWords(EMPTY_TOTALS, amounts.coverage) };
	const toDate = sumUsageTotals(entries.map((inv) => inv.toDate));
	const monthTotals = entries.map((inv) => inv.thisMonth).filter((m): m is UsageTotals => !!m);
	const thisMonth = monthTotals.length > 0 ? sumUsageTotals(monthTotals) : undefined;
	return {
		toDate: periodWords(toDate, amounts.coverage),
		...(thisMonth && thisMonth.records > 0
			? { thisMonth: periodWords(thisMonth, amounts.coverage) }
			: {})
	};
}

/** A total's qualification (decision 2): an unknown total says why; a known incomplete one says what it leaves out. */
function qualifier(t: UsageTotals, coverage?: Coverage): Pick<InvestmentWords, 'note' | 'detail'> {
	if (t.knownCostMicroUsd === null) return { note: unknownCostReason(t) };
	const text = incompleteText(t, coverage);
	return text ? { note: 'Usage incomplete', detail: text } : {};
}

/** This month's qualification, short, repeated beside its figure. */
function monthQualifier(t: UsageTotals, coverage?: Coverage): string | undefined {
	if (t.knownCostMicroUsd === null) return unknownCostReason(t).toLowerCase();
	return incompleteText(t, coverage) ? 'usage incomplete' : undefined;
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
