import { describe, expect, it } from 'vitest';
import {
	dayLabel,
	detectZone,
	effectiveZone,
	eventTime,
	formatClock,
	formatFull,
	minutesBetween,
	offsetLabel,
	relativeAgo,
	relativeUntil,
	searchZones,
	zoneOption,
	zoneOptions
} from './time';

describe('account time', () => {
	it('AC-IA20-1: one hour elapses across New York spring-forward, whatever the clock readings', () => {
		const from = new Date('2026-03-08T06:30:00Z'); // 01:30 UTC-5
		const to = new Date('2026-03-08T07:30:00Z'); // 03:30 UTC-4
		expect(formatClock(from, 'America/New_York')).toBe('01:30');
		expect(formatClock(to, 'America/New_York')).toBe('03:30');
		expect(offsetLabel(from, 'America/New_York')).toBe('UTC−05:00');
		expect(offsetLabel(to, 'America/New_York')).toBe('UTC−04:00');
		expect(minutesBetween(from, to)).toBe(60);
	});

	it('renders one instant in two saved zones without changing it', () => {
		const instant = new Date('2026-11-18T12:00:00Z');
		expect(formatClock(instant, 'Asia/Kolkata')).toBe('17:30');
		expect(formatClock(instant, 'America/Los_Angeles')).toBe('04:00');
		expect(formatFull(instant, 'Asia/Kolkata')).toBe('18 Nov 2026, 17:30 UTC+05:30');
		expect(instant.toISOString()).toBe('2026-11-18T12:00:00.000Z');
	});

	it('AC-IA20-2: no valid detection shows unsaved UTC; saved and detected zones win otherwise', () => {
		expect(effectiveZone(null, undefined)).toEqual({ zone: 'UTC', status: 'unsaved-utc' });
		expect(effectiveZone('Not/AZone', undefined)).toEqual({ zone: 'UTC', status: 'unsaved-utc' });
		expect(effectiveZone(undefined, 'Europe/Paris')).toEqual({
			zone: 'Europe/Paris',
			status: 'detected'
		});
		expect(effectiveZone('Asia/Tokyo', 'Europe/Paris')).toEqual({
			zone: 'Asia/Tokyo',
			status: 'saved'
		});
		expect(['string', 'undefined']).toContain(typeof detectZone());
	});

	it('shows relative time within a day and absolute time afterwards', () => {
		const now = new Date('2026-11-18T12:00:00Z');
		expect(eventTime('2026-11-18T11:48:00Z', now, 'UTC').text).toBe('12 min ago');
		expect(eventTime('2026-11-18T09:00:00Z', now, 'UTC').text).toBe('3 h ago');
		expect(eventTime('2026-11-16T14:02:00Z', now, 'UTC').text).toBe('16 Nov, 14:02');
		expect(eventTime('2026-11-16T14:02:00Z', now, 'UTC', true).text).toBe('14:02');
		expect(eventTime('2026-11-16T14:02:00Z', now, 'UTC').full).toBe('16 Nov 2026, 14:02 UTC');
		expect(relativeAgo(new Date('2026-11-18T11:59:50Z'), now)).toBe('just now');
		expect(relativeUntil(new Date('2026-11-18T15:00:00Z'), now)).toBe('in 3 h');
		expect(relativeUntil(new Date('2026-11-18T11:00:00Z'), now)).toBe('expired');
	});

	it('names days in the viewing zone', () => {
		const now = new Date('2026-11-18T23:30:00Z');
		expect(dayLabel(new Date('2026-11-18T01:00:00Z'), now, 'UTC')).toBe('Today');
		expect(dayLabel(new Date('2026-11-17T20:00:00Z'), now, 'UTC')).toBe('Yesterday');
		expect(dayLabel(new Date('2026-11-14T20:00:00Z'), now, 'UTC')).toBe('Sat 14 Nov');
		// 23:30 UTC is already tomorrow in Tokyo.
		expect(dayLabel(new Date('2026-11-18T01:00:00Z'), now, 'Asia/Tokyo')).toBe('Yesterday');
	});

	it('lists zones with city, offset and local time, and searches them', () => {
		const at = new Date('2026-11-18T12:00:00Z');
		expect(zoneOption('America/Argentina/Buenos_Aires', at)).toMatchObject({
			city: 'Buenos Aires',
			region: 'America',
			offset: 'UTC−03:00',
			local: '09:00'
		});
		const all = zoneOptions(at);
		expect(all.length).toBeGreaterThan(50);
		expect(searchZones(all, 'kolkata').map((o) => o.id)).toContain('Asia/Kolkata');
		expect(searchZones(all, 'new york').map((o) => o.id)).toContain('America/New_York');
		expect(searchZones(all, '+05:30').map((o) => o.id)).toContain('Asia/Kolkata');
		expect(searchZones(all, 'zzzz')).toEqual([]);
	});
});
