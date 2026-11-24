import { describe, expect, it } from 'vitest';
import {
	budgetInputValue,
	extendedBudgetLimit,
	formatBudgetAmount,
	formatCost,
	formatCount,
	formatRate,
	formatTokens,
	formatUsd,
	parseBudgetTokens,
	parseBudgetUsd,
	parseRate,
	rateInput
} from './money';

describe('formatUsd', () => {
	it('rounds micro-USD half up to whole cents with separators', () => {
		expect(formatUsd(0)).toBe('$0.00');
		expect(formatUsd(1_650)).toBe('< $0.01');
		expect(formatUsd(5_000)).toBe('$0.01');
		expect(formatUsd(4_999)).toBe('< $0.01');
		expect(formatUsd(412_004_999)).toBe('$412.00');
		expect(formatUsd(412_005_000)).toBe('$412.01');
		expect(formatUsd(1_240_500_000)).toBe('$1,240.50');
	});
	it('keeps unknown apart from zero', () => {
		expect(formatCost(null)).toBe('—');
		expect(formatCost(0)).toBe('≈ $0.00');
		expect(formatCost(123_901_525)).toBe('≈ $123.90');
	});
});

describe('rates', () => {
	it('formats exact dollars per million tokens', () => {
		expect(formatRate(3_000_000)).toBe('$3.00');
		expect(formatRate(300_000)).toBe('$0.30');
		expect(formatRate(125_000)).toBe('$0.125');
		expect(formatRate(1)).toBe('$0.000001');
		expect(formatRate(0)).toBe('$0.00');
		expect(formatRate(null)).toBe('—');
		expect(formatRate(1_000_000_000)).toBe('$1,000.00');
	});
	it('parses liberal input exactly, without floating point', () => {
		expect(parseRate('3')).toEqual({ ok: true, value: 3_000_000 });
		expect(parseRate(' $0.30 ')).toEqual({ ok: true, value: 300_000 });
		expect(parseRate('0.125')).toEqual({ ok: true, value: 125_000 });
		expect(parseRate('.5')).toEqual({ ok: true, value: 500_000 });
		expect(parseRate('1,000.5')).toEqual({ ok: true, value: 1_000_500_000 });
		expect(parseRate('0.1')).toEqual({ ok: true, value: 100_000 });
		expect(parseRate('0.000001')).toEqual({ ok: true, value: 1 });
		expect(parseRate('')).toEqual({ ok: true, value: null });
		expect(parseRate('0')).toEqual({ ok: true, value: 0 });
	});
	it('refuses what is not an amount', () => {
		expect(parseRate('-1').ok).toBe(false);
		expect(parseRate('abc').ok).toBe(false);
		expect(parseRate('1.2.3').ok).toBe(false);
		expect(parseRate('0.0000001')).toEqual({
			ok: false,
			error: 'Use at most six decimal places (a millionth of a dollar).'
		});
		expect(parseRate('2000000').ok).toBe(false);
	});
	it('round-trips through the editable form', () => {
		for (const v of [0, 1, 125_000, 3_000_000, 18_750_000, null])
			expect(parseRate(rateInput(v))).toEqual({ ok: true, value: v });
	});
});

describe('tokens', () => {
	it('floors to one decimal so a figure never reads larger than it is', () => {
		expect(formatTokens(null)).toBe('—');
		expect(formatTokens(512)).toBe('512');
		expect(formatTokens(9_999)).toBe('9,999');
		expect(formatTokens(12_560)).toBe('12.5 k');
		expect(formatTokens(61_299_999)).toBe('61.2 M');
		expect(formatTokens(25_000_000)).toBe('25 M');
		expect(formatTokens(1_200_000_000)).toBe('1.2 B');
		expect(formatCount(12_000)).toBe('12,000');
	});
	it('says the budget unit', () => {
		expect(formatBudgetAmount('usd', 20_000_000)).toBe('$20.00');
		expect(formatBudgetAmount('tokens', 25_000_000)).toBe('25 M tokens');
	});
});

describe('budget parsing and 20% extension (BUD-1, BUD-3)', () => {
	it('parses exact micro-USD and token limits and rejects zero/negative/blank', () => {
		expect(parseBudgetUsd('10')).toEqual({ ok: true, value: 10_000_000 });
		expect(parseBudgetUsd('$12.50')).toEqual({ ok: true, value: 12_500_000 });
		expect(parseBudgetUsd('')).toEqual({ ok: false, error: 'Enter a budget amount.' });
		expect(parseBudgetUsd('0')).toEqual({ ok: false, error: 'Enter an amount greater than zero.' });
		expect(parseBudgetUsd('-5').ok).toBe(false);
		expect(parseBudgetUsd('2000000')).toEqual({
			ok: false,
			error: 'That budget amount is too large. Check the amount.'
		});

		expect(parseBudgetTokens('25,000,000')).toEqual({ ok: true, value: 25_000_000 });
		expect(parseBudgetTokens('')).toEqual({ ok: false, error: 'Enter a token limit.' });
		expect(parseBudgetTokens('0').ok).toBe(false);
		expect(parseBudgetTokens('1.5').ok).toBe(false);
	});

	it('formats editable inputs and computes L + floor(L/5)', () => {
		expect(budgetInputValue('usd', 9_000_000)).toBe('9');
		expect(budgetInputValue('usd', 12_500_000)).toBe('12.5');
		expect(budgetInputValue('tokens', 25_000_000)).toBe('25000000');
		expect(extendedBudgetLimit(20_000_000)).toBe(24_000_000);
		expect(extendedBudgetLimit(7)).toBe(8);
	});
});
