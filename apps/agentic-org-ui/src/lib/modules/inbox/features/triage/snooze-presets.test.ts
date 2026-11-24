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

	it('rejects nonexistent DST spring-forward local times and selects earlier fall-back hour across negative and positive UTC zones', () => {
		const now = new Date('2027-03-10T12:00:00.000Z');
		// America/New_York springs forward on 2027-03-14 at 02:00 -> 03:00; 02:30 does not exist.
		expect(parseCustomSnoozeDateTime('2027-03-14', '02:30', 'America/New_York', now)).toEqual({
			ok: false,
			field: 'time',
			error: 'That local time does not exist because of a daylight-saving shift.'
		});

		// America/New_York falls back on 2027-11-07 at 02:00 -> 01:00; 01:30 selects the earlier EDT occurrence (05:30Z).
		const fallNow = new Date('2027-11-06T12:00:00.000Z');
		expect(
			parseCustomSnoozeDateTime('2027-11-07', '01:30', 'America/New_York', fallNow)
		).toMatchObject({
			ok: true,
			iso: '2027-11-07T05:30:00.000Z'
		});

		// Europe/Berlin falls back on 2027-10-31 at 03:00 -> 02:00; 02:30 selects the earlier CEST occurrence (00:30Z).
		const berlinNow = new Date('2027-10-30T12:00:00.000Z');
		expect(
			parseCustomSnoozeDateTime('2027-10-31', '02:30', 'Europe/Berlin', berlinNow)
		).toMatchObject({
			ok: true,
			iso: '2027-10-31T00:30:00.000Z'
		});
	});

	it('flags when a snooze return time is at or after the item expiry deadline', () => {
		expect(snoozeExceedsExpiry('2026-11-24T14:00:00Z', '2026-11-23T18:00:00Z')).toBe(true);
		expect(snoozeExceedsExpiry('2026-11-23T18:00:00Z', '2026-11-23T18:00:00Z')).toBe(true);
		expect(snoozeExceedsExpiry('2026-11-23T16:00:00Z', '2026-11-23T18:00:00Z')).toBe(false);
		expect(snoozeExceedsExpiry('2026-11-24T14:00:00Z', undefined)).toBe(false);
	});

	it('formats short and full return labels accurately across 23-hour DST spring-forward days', () => {
		const now = new Date('2026-11-23T10:00:00Z');
		const formatted = formatSnoozeReturn('2026-11-24T14:00:00Z', now, 'UTC');
		expect(formatted.short).toBe('Tomorrow, 14:00');
		expect(formatted.full).toBe('24 Nov 2026, 14:00 UTC');

		// 2027-03-13 23:30 EST -> next calendar day 2027-03-14 09:00 EDT is still "Tomorrow, 09:00"
		const dstEve = new Date('2027-03-14T04:30:00.000Z');
		const dstNext = formatSnoozeReturn('2027-03-14T13:00:00.000Z', dstEve, 'America/New_York');
		expect(dstNext.short).toBe('Tomorrow, 09:00');
	});
});
