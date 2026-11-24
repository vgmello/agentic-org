// Pure timezone-aware snooze preset and custom date/time helpers for Inbox snooze (TASK-81).
import {
	dayKey,
	formatClock,
	formatDate,
	formatFull,
	offsetMinutes
} from '$lib/modules/preferences/time';

export type SnoozePresetId = 'later_today' | 'tomorrow' | 'next_week';

export interface SnoozePreset {
	id: SnoozePresetId;
	label: string;
	detail: string;
	iso: string;
}

const WEEKDAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'] as const;

interface LocalCalendarParts {
	year: number;
	month: number;
	day: number;
	hour: number;
	minute: number;
	weekdayIndex: number;
}

function localCalendarParts(date: Date, zone: string): LocalCalendarParts {
	const parts = Object.fromEntries(
		new Intl.DateTimeFormat('en-GB', {
			timeZone: zone,
			year: 'numeric',
			month: '2-digit',
			day: '2-digit',
			hour: '2-digit',
			minute: '2-digit',
			hourCycle: 'h23',
			weekday: 'short'
		})
			.formatToParts(date)
			.map((p) => [p.type, p.value])
	);
	const weekdayIdx = WEEKDAYS.indexOf(parts.weekday as (typeof WEEKDAYS)[number]);
	return {
		year: Number(parts.year),
		month: Number(parts.month),
		day: Number(parts.day),
		hour: Number(parts.hour),
		minute: Number(parts.minute),
		weekdayIndex: weekdayIdx >= 0 ? weekdayIdx : 0
	};
}

/**
 * Converts a local wall-clock date/time in `zone` into an exact UTC Date (with DST refinement).
 * Probes offsets on both sides of the target instant so repeated fall-back hours in both positive
 * and negative UTC-offset zones deterministically select the earliest round-tripping occurrence.
 */
export function zonedWallToUtc(
	year: number,
	month: number,
	day: number,
	hour: number,
	minute: number,
	zone: string
): Date {
	const utcGuess = Date.UTC(year, month - 1, day, hour, minute, 0, 0);
	const probeOffsets = new Set<number>([
		offsetMinutes(new Date(utcGuess - 18 * 3_600_000), zone),
		offsetMinutes(new Date(utcGuess), zone),
		offsetMinutes(new Date(utcGuess + 18 * 3_600_000), zone)
	]);
	const candidates = new Set<number>();
	for (const off of probeOffsets) {
		const first = utcGuess - off * 60_000;
		candidates.add(first);
		const refined = utcGuess - offsetMinutes(new Date(first), zone) * 60_000;
		candidates.add(refined);
	}
	const sorted = [...candidates].sort((a, b) => a - b);
	for (const ms of sorted) {
		const dt = new Date(ms);
		if (roundTripsWallClock(dt, year, month, day, hour, minute, zone)) {
			return dt;
		}
	}
	return new Date(sorted[0] ?? utcGuess);
}

function roundTripsWallClock(
	date: Date,
	year: number,
	month: number,
	day: number,
	hour: number,
	minute: number,
	zone: string
): boolean {
	const p = localCalendarParts(date, zone);
	return (
		p.year === year && p.month === month && p.day === day && p.hour === hour && p.minute === minute
	);
}

function addDaysWall(
	year: number,
	month: number,
	day: number,
	deltaDays: number
): { year: number; month: number; day: number } {
	const d = new Date(Date.UTC(year, month - 1, day + deltaDays, 12, 0, 0));
	return {
		year: d.getUTCFullYear(),
		month: d.getUTCMonth() + 1,
		day: d.getUTCDate()
	};
}

function formatDayKeyFromWall(w: { year: number; month: number; day: number }): string {
	return `${w.year}-${String(w.month).padStart(2, '0')}-${String(w.day).padStart(2, '0')}`;
}

/** Short + full human labels for a snooze return timestamp in `zone`. */
export function formatSnoozeReturn(
	untilIso: string,
	now: Date,
	zone: string
): { short: string; full: string } {
	const target = new Date(untilIso);
	if (Number.isNaN(target.getTime())) {
		return { short: untilIso, full: untilIso };
	}
	const full = formatFull(target, zone);
	const targetKey = dayKey(target, zone);
	const nowLocal = localCalendarParts(now, zone);
	const today = formatDayKeyFromWall(nowLocal);
	// Compute tomorrow from the local calendar date so 23-hour DST spring-forward days stay exact.
	const tomorrow = formatDayKeyFromWall(
		addDaysWall(nowLocal.year, nowLocal.month, nowLocal.day, 1)
	);
	const clock = formatClock(target, zone);
	if (targetKey === today) return { short: `Today, ${clock}`, full };
	if (targetKey === tomorrow) return { short: `Tomorrow, ${clock}`, full };
	return { short: `${formatDate(target, zone, now)}, ${clock}`, full };
}

/** Computes timezone-aware presets: Later today (when on today's date), Tomorrow 09:00, Next week (Mon 09:00). */
export function computeSnoozePresets(now: Date, zone: string): SnoozePreset[] {
	const local = localCalendarParts(now, zone);
	const todayKey = formatDayKeyFromWall(local);

	// Later today: 17:00 in zone if at least 30 min ahead, otherwise +3 hours rounded up to 5m;
	// omit when the candidate crosses midnight into tomorrow so "Later today" never schedules tomorrow.
	const fivePmToday = zonedWallToUtc(local.year, local.month, local.day, 17, 0, zone);
	const laterCandidate =
		fivePmToday.getTime() - now.getTime() >= 30 * 60_000
			? fivePmToday
			: new Date(Math.ceil((now.getTime() + 3 * 3_600_000) / 300_000) * 300_000);
	const laterTarget = dayKey(laterCandidate, zone) === todayKey ? laterCandidate : undefined;

	// Tomorrow 09:00 in zone.
	const tomorrowWall = addDaysWall(local.year, local.month, local.day, 1);
	const tomorrowTarget = zonedWallToUtc(
		tomorrowWall.year,
		tomorrowWall.month,
		tomorrowWall.day,
		9,
		0,
		zone
	);

	// Next week: next Monday 09:00 in zone (always strictly in the next calendar week).
	const daysUntilMonday = local.weekdayIndex === 0 ? 7 : 7 - local.weekdayIndex;
	const nextMonWall = addDaysWall(local.year, local.month, local.day, daysUntilMonday);
	const nextWeekTarget = zonedWallToUtc(
		nextMonWall.year,
		nextMonWall.month,
		nextMonWall.day,
		9,
		0,
		zone
	);

	const presets: SnoozePreset[] = [];
	if (laterTarget) {
		presets.push({
			id: 'later_today',
			label: 'Later today',
			detail: formatSnoozeReturn(laterTarget.toISOString(), now, zone).short,
			iso: laterTarget.toISOString()
		});
	}
	presets.push(
		{
			id: 'tomorrow',
			label: 'Tomorrow',
			detail: formatSnoozeReturn(tomorrowTarget.toISOString(), now, zone).short,
			iso: tomorrowTarget.toISOString()
		},
		{
			id: 'next_week',
			label: 'Next week',
			detail: formatSnoozeReturn(nextWeekTarget.toISOString(), now, zone).short,
			iso: nextWeekTarget.toISOString()
		}
	);
	return presets;
}

/** Default inputs for the Custom date & time snooze dialog (tomorrow at 09:00 in `zone`). */
export function defaultCustomSnoozeInputs(now: Date, zone: string): { date: string; time: string } {
	const local = localCalendarParts(now, zone);
	const tomorrow = addDaysWall(local.year, local.month, local.day, 1);
	const mm = String(tomorrow.month).padStart(2, '0');
	const dd = String(tomorrow.day).padStart(2, '0');
	return { date: `${tomorrow.year}-${mm}-${dd}`, time: '09:00' };
}

export type CustomSnoozeValidation =
	{ ok: true; iso: string; date: Date } | { ok: false; field: 'date' | 'time'; error: string };

/**
 * Parses and validates user-entered YYYY-MM-DD and HH:MM in `zone`, rejecting:
 * - invalid calendar dates or clock times
 * - nonexistent DST spring-forward local times
 * - timestamps at or before `now`
 *
 * Note: Expiry enforcement stays authoritative on the office (`msg.defer`) because active inbox rows
 * may surface an optional ask's shorter `wait_until` rather than its 24h `msg.expires_at`.
 */
export function parseCustomSnoozeDateTime(
	dateStr: string,
	timeStr: string,
	zone: string,
	now: Date
): CustomSnoozeValidation {
	const dMatch = /^(\d{4})-(\d{2})-(\d{2})$/.exec(dateStr.trim());
	if (!dMatch) {
		return { ok: false, field: 'date', error: 'Choose a valid return date.' };
	}
	const year = Number(dMatch[1]);
	const month = Number(dMatch[2]);
	const day = Number(dMatch[3]);
	const calendarCheck = new Date(Date.UTC(year, month - 1, day));
	if (
		calendarCheck.getUTCFullYear() !== year ||
		calendarCheck.getUTCMonth() + 1 !== month ||
		calendarCheck.getUTCDate() !== day
	) {
		return { ok: false, field: 'date', error: 'Choose a valid calendar date.' };
	}

	const tMatch = /^(\d{2}):(\d{2})$/.exec(timeStr.trim());
	if (!tMatch) {
		return { ok: false, field: 'time', error: 'Choose a valid return time.' };
	}
	const hour = Number(tMatch[1]);
	const minute = Number(tMatch[2]);
	if (hour > 23 || minute > 59) {
		return { ok: false, field: 'time', error: 'Choose a valid return time.' };
	}

	const target = zonedWallToUtc(year, month, day, hour, minute, zone);
	if (
		Number.isNaN(target.getTime()) ||
		!roundTripsWallClock(target, year, month, day, hour, minute, zone)
	) {
		return {
			ok: false,
			field: 'time',
			error: 'That local time does not exist because of a daylight-saving shift.'
		};
	}

	if (target.getTime() <= now.getTime()) {
		return {
			ok: false,
			field: 'time',
			error: 'Return time must be in the future.'
		};
	}

	return { ok: true, iso: target.toISOString(), date: target };
}

/** True when a chosen snooze return timestamp falls at or after the item's expiry deadline (`a >= e`). */
export function snoozeExceedsExpiry(
	untilIso: string | undefined,
	expiresAtIso: string | undefined
): boolean {
	if (!untilIso || !expiresAtIso) return false;
	const untilMs = Date.parse(untilIso);
	const expiresMs = Date.parse(expiresAtIso);
	if (Number.isNaN(untilMs) || Number.isNaN(expiresMs)) return false;
	return untilMs >= expiresMs;
}
