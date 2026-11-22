import { describe, expect, it } from 'vitest';
import { checkRateForm, fieldOf, firstError, type RateForm } from './rate-form';

const form = (over: Partial<RateForm> = {}): RateForm => ({
	provider: 'anthropic',
	model: 'claude-opus-4-1',
	input: '5',
	output: '25',
	cacheRead: '0.50',
	cacheWrite: '',
	effectiveFrom: '2026-11-20',
	...over
});

describe('Add a rate (costs-and-insights-ux §5)', () => {
	it('converts dollar rates exactly and keeps a blank rate unknown', () => {
		const checked = checkRateForm(form(), '2026-11-19');
		expect(checked).toEqual({
			ok: true,
			values: {
				provider: 'anthropic',
				model: 'claude-opus-4-1',
				inputMicroUsdPerMillion: 5_000_000,
				outputMicroUsdPerMillion: 25_000_000,
				cacheReadMicroUsdPerMillion: 500_000,
				cacheWriteMicroUsdPerMillion: null,
				effectiveFrom: '2026-11-20'
			}
		});
	});
	it('takes effect from today or later only', () => {
		expect(checkRateForm(form({ effectiveFrom: '2026-11-19' }), '2026-11-19').ok).toBe(true);
		const past = checkRateForm(form({ effectiveFrom: '2026-11-18' }), '2026-11-19');
		expect(past).toEqual({
			ok: false,
			errors: { effectiveFrom: 'A new rate takes effect from today or a later date.' }
		});
	});
	it('names every field to fix, first in reading order', () => {
		const checked = checkRateForm(
			form({ provider: ' ', model: '', input: 'x', effectiveFrom: '' }),
			'2026-11-19'
		);
		expect(checked.ok).toBe(false);
		if (checked.ok) return;
		expect(Object.keys(checked.errors)).toEqual(['provider', 'model', 'input', 'effectiveFrom']);
		expect(firstError(checked.errors)).toBe('provider');
	});
	it('needs at least one known rate', () => {
		const checked = checkRateForm(
			form({ input: '', output: '', cacheRead: '', cacheWrite: '' }),
			'2026-11-19'
		);
		expect(checked.ok ? undefined : checked.errors.input).toMatch(/at least one rate/);
		expect(checkRateForm(form({ input: '0', output: '', cacheRead: '' }), '2026-11-19').ok).toBe(
			true
		);
	});
	it('points a server refusal at its field', () => {
		expect(fieldOf('rates')).toBe('input');
		expect(fieldOf('effectiveFrom')).toBe('effectiveFrom');
		expect(fieldOf(undefined)).toBeUndefined();
	});
});
