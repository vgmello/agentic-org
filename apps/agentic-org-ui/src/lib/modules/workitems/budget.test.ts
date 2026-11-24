import { describe, expect, it } from 'vitest';
import {
	budgetChipText,
	budgetPercent,
	budgetTone,
	canAllowOnce,
	extendedLimit,
	formatMicroUsd,
	formatTokens,
	microUsdToInputValue,
	parseDollarsToMicroUsd,
	parseTokenLimit
} from './budget';

describe('workitems budget helpers', () => {
	it('converts dollar strings to exact integer micro-USD without floating-point drift', () => {
		expect(parseDollarsToMicroUsd('10')).toEqual({ ok: true, microUsd: 10_000_000 });
		expect(parseDollarsToMicroUsd('$12.50')).toEqual({ ok: true, microUsd: 12_500_000 });
		expect(parseDollarsToMicroUsd('0.000001')).toEqual({ ok: true, microUsd: 1 });
		expect(parseDollarsToMicroUsd('19.99')).toEqual({ ok: true, microUsd: 19_990_000 });
		expect(parseDollarsToMicroUsd('0').ok).toBe(false);
		expect(parseDollarsToMicroUsd('-5').ok).toBe(false);
		expect(parseDollarsToMicroUsd('1.1234567').ok).toBe(false);
		expect(parseDollarsToMicroUsd('abc').ok).toBe(false);
	});

	it('parses positive integer token limits', () => {
		expect(parseTokenLimit('1,500,000')).toEqual({ ok: true, tokens: 1_500_000 });
		expect(parseTokenLimit('0').ok).toBe(false);
		expect(parseTokenLimit('12.5').ok).toBe(false);
	});

	it('formats micro-USD and tokens cleanly', () => {
		expect(formatMicroUsd(10_000_000)).toBe('$10');
		expect(formatMicroUsd(6_400_000)).toBe('$6.40');
		expect(formatMicroUsd(2_500)).toBe('$0.0025');
		expect(microUsdToInputValue(6_400_000)).toBe('6.40');
		expect(formatTokens(1_900_000)).toBe('1.9 M');
		expect(formatTokens(420_000)).toBe('420 k');
		expect(formatTokens(850)).toBe('850');
		expect(budgetPercent(6_400_000, 10_000_000)).toBe(64);
	});

	it('enforces BUD-3 Allow 20% once rules (L + floor(L/5), base reach only, exceeds spend)', () => {
		expect(extendedLimit(10_000_000)).toBe(12_000_000);
		expect(extendedLimit(7)).toBe(8);

		expect(canAllowOnce({ limit: 10_000_000, spent: 8_000_000, extended: false })).toMatchObject({
			ok: false,
			reason: 'Allow 20% once is available only when the budget is reached.'
		});
		expect(
			canAllowOnce({ limit: 10_000_000, spent: 10_200_000, extended: false, hold: 'held' })
		).toEqual({ ok: true, nextLimit: 12_000_000 });
		expect(
			canAllowOnce({ limit: 12_000_000, spent: 12_100_000, extended: true, hold: 'held' })
		).toMatchObject({
			ok: false,
			reason: 'This budget already used its one 20% extension.'
		});
		expect(
			canAllowOnce({ limit: 10_000_000, spent: 12_500_000, extended: false, hold: 'held' }).ok
		).toBe(false);
	});

	it('reserves amber tone strictly for reached budgets and differentiates stop_pending vs held', () => {
		expect(budgetChipText('ok')).toBe('Budget OK');
		expect(budgetChipText('warned')).toBe('Budget 80%');
		expect(budgetChipText('reached', 'stop_pending')).toBe('Stopping · Budget reached');
		expect(budgetChipText('reached', 'held')).toBe('Paused · Budget reached');
		expect(budgetTone('ok')).toBe('ok');
		expect(budgetTone('warned')).toBe('neutral');
		expect(budgetTone('reached')).toBe('owner');
	});
});
