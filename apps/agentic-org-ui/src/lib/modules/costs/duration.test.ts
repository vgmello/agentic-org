import { describe, expect, it } from 'vitest';
import { formatDuration, formatMedianDuration, formatPreciseDuration } from './duration';

describe('shared duration definitions (TIME-4)', () => {
	it('matches every exact spec check vector', () => {
		expect(formatDuration(59 * 60 + 59)).toBe('59 min');
		expect(formatDuration(47 * 3600 + 59 * 60)).toBe('47 h');
		expect(formatDuration(48 * 3600)).toBe('2.0 d');
		expect(formatDuration(126 * 3600)).toBe('5.2 d');
	});

	it('handles zero, fractional seconds, and unknown values honestly', () => {
		expect(formatDuration(0)).toBe('0 min');
		expect(formatDuration(59.9)).toBe('0 min');
		expect(formatDuration(3600)).toBe('1 h');
		expect(formatDuration(null)).toBe('—');
		expect(formatDuration(undefined)).toBe('—');
		expect(formatDuration(-5)).toBe('—');
		expect(formatDuration(Number.NaN)).toBe('—');
	});

	it('produces readable precise duration labels', () => {
		expect(formatPreciseDuration(0)).toBe('0 minutes');
		expect(formatPreciseDuration(3600 + 120)).toBe('1 hour 2 minutes');
		expect(formatPreciseDuration(86400 * 2 + 3600 * 4)).toBe('2 days 4 hours');
		expect(formatPreciseDuration(null)).toBe('Duration unavailable');
	});

	it('withholds medians below 5 items', () => {
		expect(formatMedianDuration(7200, 4)).toEqual({
			value: '—',
			note: '4 items (needs 5 for median)'
		});
		expect(formatMedianDuration(7200, 1)).toEqual({
			value: '—',
			note: '1 item (needs 5 for median)'
		});
		expect(formatMedianDuration(null, 0)).toEqual({
			value: '—',
			note: 'No completed items'
		});
		expect(formatMedianDuration(172800, 12)).toEqual({
			value: '2.0 d',
			note: '12 items'
		});
	});
});
