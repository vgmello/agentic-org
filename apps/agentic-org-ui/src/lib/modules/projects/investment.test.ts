import { describe, expect, it } from 'vitest';
import type { UsageTotals } from '$lib/modules/costs';
import { investmentWords, portfolioInvestmentWords } from './investment';

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

describe('portfolioInvestmentWords', () => {
	it('returns undefined when amounts are missing or not organization-scoped', () => {
		expect(portfolioInvestmentWords(undefined)).toBeUndefined();
		expect(
			portfolioInvestmentWords({
				scope: 'managed',
				byProject: new Map([['a', { toDate: totals() }]]),
				coverage: { checkpointAt: '2026-11-20T00:00:00Z', gaps: [] }
			})
		).toBeUndefined();
	});

	it('keeps exact period-specific qualifications for toDate (3 unpriced) and thisMonth (2 unpriced) with coverage gaps', () => {
		const got = portfolioInvestmentWords({
			scope: 'organization',
			byProject: new Map([
				[
					'priced',
					{
						toDate: totals({ records: 4, knownCostMicroUsd: 2_500_000 }),
						thisMonth: totals({ records: 2, knownCostMicroUsd: 1_200_000 })
					}
				],
				[
					'unpriced',
					{
						toDate: totals({
							records: 3,
							knownCostMicroUsd: null,
							unpricedRecords: 3,
							costIncompleteRecords: 3
						}),
						thisMonth: totals({
							records: 2,
							knownCostMicroUsd: null,
							unpricedRecords: 2,
							costIncompleteRecords: 2
						})
					}
				]
			]),
			coverage: {
				checkpointAt: '2026-11-20T14:00:00Z',
				gaps: [{ source: 'Studio Mac office', reason: 'not reported since 13:40' }]
			}
		});
		expect(got).toEqual({
			toDate: {
				amount: '≈ $2.50',
				qualification:
					'Usage incomplete: excludes 3 unpriced records · 1 usage source not fully reported'
			},
			thisMonth: {
				amount: '≈ $1.20',
				qualification:
					'Usage incomplete: excludes 2 unpriced records · 1 usage source not fully reported'
			}
		});
	});

	it('preserves API-equivalent label on both periods for subscription-derived usage', () => {
		const got = portfolioInvestmentWords({
			scope: 'organization',
			byProject: new Map([
				[
					'sub-plan',
					{
						toDate: totals({
							records: 5,
							knownCostMicroUsd: 8_000_000,
							includesApiEquivalent: true
						}),
						thisMonth: totals({
							records: 2,
							knownCostMicroUsd: 3_000_000,
							includesApiEquivalent: true
						})
					}
				]
			]),
			coverage: { checkpointAt: '2026-11-20T14:00:00Z', gaps: [] }
		});
		expect(got).toEqual({
			toDate: {
				amount: '≈ $8.00',
				apiEquivalent: 'Includes API-equivalent cost for subscription plans'
			},
			thisMonth: {
				amount: '≈ $3.00',
				apiEquivalent: 'Includes API-equivalent cost for subscription plans'
			}
		});
	});

	it('keeps unknown totals as "—" with their reason when no project has known dollars', () => {
		const got = portfolioInvestmentWords({
			scope: 'organization',
			byProject: new Map([
				[
					'only-unpriced',
					{
						toDate: totals({
							records: 5,
							knownCostMicroUsd: null,
							unpricedRecords: 5,
							costIncompleteRecords: 5
						})
					}
				]
			]),
			coverage: { checkpointAt: '2026-11-20T14:00:00Z', gaps: [] }
		});
		expect(got).toEqual({
			toDate: {
				amount: '—',
				qualification: 'No known dollars: 5 records, none priced'
			}
		});
	});

	it('preserves incomplete token counts and coverage gaps alongside zero known dollars', () => {
		const got = portfolioInvestmentWords({
			scope: 'organization',
			byProject: new Map([
				[
					'only-unpriced',
					{
						toDate: totals({
							records: 5,
							knownCostMicroUsd: null,
							unpricedRecords: 5,
							costIncompleteRecords: 5,
							tokenIncompleteRecords: 2
						})
					}
				]
			]),
			coverage: {
				checkpointAt: '2026-11-20T14:00:00Z',
				gaps: [{ source: 'Studio Mac office', reason: 'not reported since 13:40' }]
			}
		});
		expect(got).toEqual({
			toDate: {
				amount: '—',
				qualification:
					'No known dollars: 5 records, none priced · Usage incomplete: 2 records without full token counts · 1 usage source not fully reported'
			}
		});
	});

	it('qualifies an empty ledger when reporting coverage gaps exist', () => {
		const emptyWithGap = portfolioInvestmentWords({
			scope: 'organization',
			byProject: new Map([
				[
					'idle',
					{
						toDate: totals({ records: 0, knownCostMicroUsd: null })
					}
				]
			]),
			coverage: {
				checkpointAt: '2026-11-20T14:00:00Z',
				gaps: [{ source: 'Studio Mac office', reason: 'not reported since 13:40' }]
			}
		});
		expect(emptyWithGap).toEqual({
			toDate: {
				amount: 'No usage yet',
				qualification: 'Usage incomplete: 1 usage source not fully reported'
			}
		});

		const noEntriesWithGap = portfolioInvestmentWords({
			scope: 'organization',
			byProject: new Map(),
			coverage: {
				checkpointAt: '2026-11-20T14:00:00Z',
				gaps: [{ source: 'Studio Mac office', reason: 'not reported since 13:40' }]
			}
		});
		expect(noEntriesWithGap).toEqual({
			toDate: {
				amount: 'No usage yet',
				qualification: 'Usage incomplete: 1 usage source not fully reported'
			}
		});
	});
});
