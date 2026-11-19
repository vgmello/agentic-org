import { describe, expect, it } from 'vitest';
import type { UsageTotals } from '$lib/modules/costs';
import { investmentWords } from './investment';

const totals = (over: Partial<UsageTotals> = {}): UsageTotals => ({
	records: 10,
	knownCostMicroUsd: 1_240_500_000,
	knownTokens: 1_000,
	cacheReadTokens: 0,
	tokenIncompleteRecords: 0,
	costIncompleteRecords: 0,
	unpricedRecords: 0,
	unknownCostRecords: 0,
	includesApiEquivalent: false,
	...over
});

describe('investmentWords (costs decision 6)', () => {
	it('gives the total to date with this month beside it', () => {
		expect(
			investmentWords({ toDate: totals(), thisMonth: totals({ knownCostMicroUsd: 412_000_000 }) })
		).toEqual({ toDate: '≈ $1,240.50', thisMonth: '≈ $412.00 this month' });
	});
	it('never turns no records into $0', () => {
		expect(investmentWords(undefined)).toEqual({ toDate: 'No usage yet' });
		expect(investmentWords({ toDate: totals({ records: 0, knownCostMicroUsd: null }) })).toEqual({
			toDate: 'No usage yet'
		});
	});
	it('keeps an unknown total as "—" with its reason, and says what an incomplete one leaves out', () => {
		expect(
			investmentWords({
				toDate: totals({ knownCostMicroUsd: null, unpricedRecords: 10, costIncompleteRecords: 10 })
			})
		).toEqual({ toDate: '—', note: 'No known dollars: 10 records, none priced' });
		expect(
			investmentWords({ toDate: totals({ unpricedRecords: 2, costIncompleteRecords: 2 }) })
		).toEqual({
			toDate: '≈ $1,240.50',
			note: 'Usage incomplete',
			detail: 'Usage incomplete: excludes 2 unpriced records'
		});
	});
	it('leaves out this month when nothing was recorded in it', () => {
		expect(
			investmentWords({
				toDate: totals(),
				thisMonth: totals({ records: 0, knownCostMicroUsd: null })
			}).thisMonth
		).toBeUndefined();
	});
	it('qualifies this month too: unknown says why, incomplete says so', () => {
		expect(
			investmentWords({
				toDate: totals(),
				thisMonth: totals({ records: 3, knownCostMicroUsd: null, unknownCostRecords: 3 })
			}).thisMonth
		).toBe('— this month, cost unknown: the runtime reported no cost for 3 records');
		expect(
			investmentWords({
				toDate: totals(),
				thisMonth: totals({ records: 3, unpricedRecords: 1, costIncompleteRecords: 1 })
			}).thisMonth
		).toBe('≈ $1,240.50 this month, usage incomplete');
	});
	it("says an amount isn't shown rather than leaving a gap that reads as unknown", () => {
		expect(investmentWords(undefined, undefined, false)).toEqual({
			toDate: 'Not shown',
			detail: "Only the organization's owner and the project's manager see its cost."
		});
	});
});
