import { describe, expect, it } from 'vitest';
import {
	attributionText,
	basisText,
	budgetAmountText,
	budgetPercent,
	budgetState,
	changeText,
	checkCustomRange,
	groupPrices,
	incompleteText,
	overheadText,
	periodRange,
	shareOf,
	unknownCostReason
} from './present';
import type { BudgetPeriodState, PriceVersion, ResolvedPeriod, UsageTotals } from './types';

const totals = (over: Partial<UsageTotals> = {}): UsageTotals => ({
	records: 10,
	knownCostMicroUsd: 1_000_000,
	knownTokens: 1_000,
	cacheReadTokens: 500,
	tokenIncompleteRecords: 0,
	costIncompleteRecords: 0,
	unpricedRecords: 0,
	includesApiEquivalent: false,
	...over
});
const zone = 'America/New_York';
const october: ResolvedPeriod = {
	kind: 'last_month',
	startsAt: '2026-10-01T04:00:00Z',
	endsAt: '2026-11-01T04:00:00Z',
	timeZone: zone,
	toNow: false
};

describe('periods', () => {
	it('names the range in the zone, with an exclusive end shown as the last whole day', () => {
		expect(periodRange(october)).toBe('1 Oct – 31 Oct 2026');
		expect(
			periodRange({
				...october,
				kind: 'this_month',
				startsAt: '2026-11-01T04:00:00Z',
				endsAt: '2026-11-19T15:00:00Z',
				toNow: true
			})
		).toBe('1 Nov 2026 – now');
		expect(
			periodRange({ kind: 'all_time', endsAt: '2026-11-19T15:00:00Z', timeZone: zone, toNow: true })
		).toBe('Everything recorded up to now');
	});
	it('compares with the previous period in words, and not at all when unknown', () => {
		const previous = { period: october, knownCostMicroUsd: 100_000_000 };
		expect(changeText(118_000_000, previous)).toBe('18% more than 1 Oct – 31 Oct 2026');
		expect(changeText(94_000_000, previous)).toBe('6% less than 1 Oct – 31 Oct 2026');
		expect(changeText(100_200_000, previous)).toBe('About the same as 1 Oct – 31 Oct 2026');
		expect(changeText(null, previous)).toBeUndefined();
		expect(changeText(5, undefined)).toBeUndefined();
		expect(changeText(5, { period: october, knownCostMicroUsd: null })).toBeUndefined();
		expect(changeText(5, { period: october, knownCostMicroUsd: 0 })).toBe(
			'No spend in 1 Oct – 31 Oct 2026'
		);
	});
	it('refuses a custom range that ends before it starts, or has no start (§3)', () => {
		expect(checkCustomRange('', '', '2026-11-19')).toEqual({ from: 'Choose the first day.' });
		expect(checkCustomRange('', '2026-11-02', '2026-11-19').from).toBe('Choose the first day.');
		expect(checkCustomRange('2026-11-10', '2026-11-02', '2026-11-19')).toEqual({
			to: "The last day can't be before the first day."
		});
		expect(checkCustomRange('2026-11-20', '', '2026-11-19').from).toBe(
			"The first day can't be after today."
		);
		expect(checkCustomRange('2026-02-30', '', '2026-11-19').from).toBe(
			'Enter a date, such as 2026-11-01.'
		);
		expect(checkCustomRange('2026-11-01', '', '2026-11-19')).toEqual({});
		expect(checkCustomRange('2026-11-01', '2026-11-01', '2026-11-19')).toEqual({});
	});
});

describe('totals say what is known (decision 2)', () => {
	it('repeats the incomplete qualification with its counts', () => {
		expect(incompleteText(totals())).toBeUndefined();
		expect(
			incompleteText(
				totals({ unpricedRecords: 5, costIncompleteRecords: 8, tokenIncompleteRecords: 1 }),
				{
					checkpointAt: '2026-11-19T14:58:00Z',
					gaps: [{ source: 'Studio Mac office', reason: 'not reported' }]
				}
			)
		).toBe(
			'Usage incomplete: excludes 5 unpriced records · 3 records with a partial cost · 1 record without full token counts · 1 usage source not fully reported'
		);
		expect(incompleteText(totals({ unpricedRecords: 1, costIncompleteRecords: 1 }))).toBe(
			'Usage incomplete: excludes 1 unpriced record'
		);
	});
	it('explains an unknown sum instead of showing zero', () => {
		expect(unknownCostReason(totals({ knownCostMicroUsd: null, unpricedRecords: 10 }))).toBe(
			'No known dollars: 10 records, none priced'
		);
		expect(unknownCostReason(totals({ records: 0, knownCostMicroUsd: null }))).toBe(
			'No usage records'
		);
	});
	it('keeps team and organization overhead apart, saying none rather than a dash', () => {
		expect(
			overheadText({
				team: totals({ records: 0, knownCostMicroUsd: null }),
				organization: totals()
			})
		).toBe('Team overhead: none · organization overhead: ≈ $1.00');
	});
	it('computes a share only from known figures', () => {
		expect(shareOf(25, 100)).toBe(25);
		expect(shareOf(null, 100)).toBeUndefined();
		expect(shareOf(5, 0)).toBeUndefined();
	});
});

describe('records', () => {
	it('names the basis and the attribution', () => {
		expect(basisText({ costBasis: 'token_estimate', apiEquivalent: true })).toBe(
			'Token estimate · API-equivalent'
		);
		expect(basisText({ costBasis: 'unpriced', apiEquivalent: false })).toBe('Unpriced');
		expect(
			attributionText({
				basis: 'work_for',
				task: { ref: 'T-12', title: 'Add sign-in rate limiting' }
			})
		).toBe('T-12 Add sign-in rate limiting · work for');
		expect(
			attributionText({ basis: 'project_overhead', project: { id: 'atlas', name: 'Atlas' } })
		).toBe('Atlas · project overhead, not on a task');
		expect(attributionText({ basis: 'organization_overhead' })).toBe(
			'Organization overhead · no project or team recorded'
		);
	});
});

describe('budgets (document 17)', () => {
	const budget = (over: Partial<BudgetPeriodState>): BudgetPeriodState => ({
		unit: 'usd',
		limit: 10_000_000,
		spent: 0,
		complete: true,
		period: { key: '2026-11-19', startsAt: '2026-11-19T05:00:00Z', endsAt: '2026-11-20T05:00:00Z' },
		...over
	});
	it('words each state; only reached is amber', () => {
		expect(budgetState(budget({ spent: 7_999_999 }))).toBeUndefined();
		expect(budgetState(budget({ spent: 8_000_000 }))).toEqual({
			word: '80% reached',
			tone: 'quiet'
		});
		expect(budgetState(budget({ spent: 10_000_000 }))).toEqual({
			word: 'Budget reached',
			tone: 'owner'
		});
		expect(budgetState(budget({ spent: 11_000_000, hold: 'stop_pending' }))?.word).toBe(
			'Stopping · Budget reached'
		);
		expect(budgetState(budget({ spent: 11_000_000, hold: 'held' }))?.word).toBe(
			'Paused · Budget reached'
		);
	});
	it('floors the percentage and qualifies an incomplete spend', () => {
		expect(budgetPercent({ spent: 7_999_999, limit: 10_000_000 })).toBe(79);
		expect(budgetAmountText(budget({ spent: 7_871_000, limit: 9_000_000 }))).toBe(
			'≈ $7.87 of $9.00 · 87%'
		);
		expect(budgetAmountText(budget({ spent: 0, complete: false }))).toBe(
			'at least ≈ $0.00 of $10.00 · 0%'
		);
		expect(budgetAmountText(budget({ unit: 'tokens', spent: 18_102_000, limit: 25_000_000 }))).toBe(
			'18.1 M of 25 M tokens · 72%'
		);
	});
});

describe('prices', () => {
	const version = (id: string, model: string, from: string): PriceVersion => ({
		id,
		provider: 'anthropic',
		model,
		effectiveFrom: from,
		inputMicroUsdPerMillion: 1,
		outputMicroUsdPerMillion: 1,
		cacheReadMicroUsdPerMillion: null,
		cacheWriteMicroUsdPerMillion: 0,
		used: false,
		created: { at: from }
	});
	it('splits each model into current, scheduled and earlier at now', () => {
		const groups = groupPrices(
			[
				version('b2', 'opus', '2026-10-15T04:00:00Z'),
				version('a1', 'sonnet', '2026-09-01T04:00:00Z'),
				version('b1', 'opus', '2026-09-01T04:00:00Z'),
				version('b3', 'opus', '2026-12-01T05:00:00Z')
			],
			new Date('2026-11-19T15:00:00Z')
		);
		expect(groups.map((g) => g.model)).toEqual(['opus', 'sonnet']);
		expect(groups[0].current?.id).toBe('b2');
		expect(groups[0].scheduled.map((p) => p.id)).toEqual(['b3']);
		expect(groups[0].earlier.map((p) => p.id)).toEqual(['b1']);
		expect(groups[1].current?.id).toBe('a1');
	});
	it('has no current rate when every version starts later', () => {
		const [group] = groupPrices(
			[version('x', 'm', '2026-12-01T05:00:00Z')],
			new Date('2026-11-19T00:00:00Z')
		);
		expect(group.current).toBeUndefined();
		expect(group.scheduled).toHaveLength(1);
	});
});
