import { describe, expect, it } from 'vitest';
import {
	computeSnoozePresets,
	defaultCustomSnoozeInputs,
	formatSnoozeReturn,
	parseCustomSnoozeDateTime,
	snoozeExceedsExpiry
} from './snooze-presets';

describe('Inbox snooze presets and custom date/time validation (TASK-81)', () => {
	it('computes Later today (17:00), Tomorrow (09:00), and Next week (Mon 09:00) in the account zone', () => {
		// Monday 23 Nov 2026, 10:00 UTC -> 05:00 America/New_York (UTC-5)
		const now = new Date('2026-11-23T10:00:00.000Z');
		const presets = computeSnoozePresets(now, 'America/New_York');
		expect(presets.map((p) => p.id)).toEqual(['later_today', 'tomorrow', 'next_week']);
		// 17:00 EST = 22:00 UTC
		expect(presets[0].iso).toBe('2026-11-23T22:00:00.000Z');
		expect(presets[0].detail).toBe('Today, 17:00');
		// Tomorrow 09:00 EST = 14:00 UTC
		expect(presets[1].iso).toBe('2026-11-24T14:00:00.000Z');
		expect(presets[1].detail).toBe('Tomorrow, 09:00');
		// Next Monday 30 Nov 09:00 EST = 14:00 UTC
		expect(presets[2].iso).toBe('2026-11-30T14:00:00.000Z');
	});

	it('falls back to +3h for Later today when 17:00 has passed or is under 30 min away', () => {
		const now = new Date('2026-11-23T16:40:00.000Z');
		const presets = computeSnoozePresets(now, 'UTC');
		expect(presets[0].iso).toBe('2026-11-23T19:40:00.000Z');
		expect(presets[0].detail).toBe('Today, 19:40');
	});

	it('validates custom date/time inputs and rejects past or invalid calendar dates', () => {
		const now = new Date('2026-11-23T12:00:00.000Z');
		expect(defaultCustomSnoozeInputs(now, 'UTC')).toEqual({ date: '2026-11-24', time: '09:00' });

		expect(parseCustomSnoozeDateTime('2026-11-23', '11:30', 'UTC', now)).toEqual({
			ok: false,
			field: 'time',
			error: 'Return time must be in the future.'
		});
		expect(parseCustomSnoozeDateTime('2026-02-30', '09:00', 'UTC', now)).toEqual({
			ok: false,
			field: 'date',
			error: 'Choose a valid calendar date.'
		});
		expect(parseCustomSnoozeDateTime('2026-11-25', '09:30', 'America/New_York', now)).toMatchObject(
			{
				ok: true,
				iso: '2026-11-25T14:30:00.000Z'
			}
		);
	});

	it('flags when a snooze return time exceeds the item expiry deadline', () => {
		expect(snoozeExceedsExpiry('2026-11-24T14:00:00Z', '2026-11-23T18:00:00Z')).toBe(true);
		expect(snoozeExceedsExpiry('2026-11-23T16:00:00Z', '2026-11-23T18:00:00Z')).toBe(false);
		expect(snoozeExceedsExpiry('2026-11-24T14:00:00Z', undefined)).toBe(false);
	});

	it('formats short and full return labels in the requested time zone', () => {
		const now = new Date('2026-11-23T10:00:00Z');
		const formatted = formatSnoozeReturn('2026-11-24T14:00:00Z', now, 'UTC');
		expect(formatted.short).toBe('Tomorrow, 14:00');
		expect(formatted.full).toBe('24 Nov 2026, 14:00 UTC');
	});
});
