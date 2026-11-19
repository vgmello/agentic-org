/**
 * Account time display (app-shell-ux "Account time display"). Pure helpers: every function takes the
 * instant and the viewing account's IANA zone; nothing reads a clock or a stored zone. Elapsed time is
 * always computed from instants, never from two local clock readings.
 */

/** Whether `zone` is an IANA zone this browser knows. */
export function isValidZone(zone: unknown): zone is string {
	if (typeof zone !== 'string' || zone.trim() === '') return false;
	try {
		new Intl.DateTimeFormat('en-GB', { timeZone: zone });
		return true;
	} catch {
		return false;
	}
}

/** The browser's zone when it is a valid IANA name; otherwise undefined (the "unsaved UTC" case). */
export function detectZone(): string | undefined {
	try {
		const zone = Intl.DateTimeFormat().resolvedOptions().timeZone;
		return isValidZone(zone) ? zone : undefined;
	} catch {
		return undefined;
	}
}

export type ZoneStatus = 'saved' | 'detected' | 'unsaved-utc';

/** The zone to display with, and why: saved wins, then a valid detection, else UTC marked unsaved. */
export function effectiveZone(
	saved: string | null | undefined,
	detected: string | undefined
): { zone: string; status: ZoneStatus } {
	if (isValidZone(saved)) return { zone: saved, status: 'saved' };
	if (detected && isValidZone(detected)) return { zone: detected, status: 'detected' };
	return { zone: 'UTC', status: 'unsaved-utc' };
}

const clockFormats = new Map<string, Intl.DateTimeFormat>();
function formatter(zone: string, key: string, options: Intl.DateTimeFormatOptions) {
	const id = `${zone}|${key}`;
	let format = clockFormats.get(id);
	if (!format) {
		format = new Intl.DateTimeFormat('en-GB', { timeZone: zone, ...options });
		clockFormats.set(id, format);
	}
	return format;
}

function parts(date: Date, zone: string, key: string, options: Intl.DateTimeFormatOptions) {
	const out: Record<string, string> = {};
	for (const part of formatter(zone, key, options).formatToParts(date)) out[part.type] = part.value;
	return out;
}

/** The zone's offset from UTC at `date`, in minutes (DST applied for that instant). */
export function offsetMinutes(date: Date, zone: string): number {
	const p = parts(date, zone, 'offset', { timeZoneName: 'longOffset' });
	const match = /^GMT(?:([+−-])(\d{1,2})(?::(\d{2}))?)?$/.exec(p.timeZoneName ?? '');
	if (!match || !match[1]) return 0;
	const minutes = Number(match[2]) * 60 + Number(match[3] ?? 0);
	return match[1] === '+' ? minutes : -minutes;
}

/** "UTC+05:30", "UTC−05:00", "UTC". */
export function offsetLabel(date: Date, zone: string): string {
	const minutes = offsetMinutes(date, zone);
	if (minutes === 0) return 'UTC';
	const sign = minutes > 0 ? '+' : '−';
	const abs = Math.abs(minutes);
	return `UTC${sign}${String(Math.floor(abs / 60)).padStart(2, '0')}:${String(abs % 60).padStart(2, '0')}`;
}

/** 24-hour clock in the zone: "14:02". */
export function formatClock(date: Date, zone: string): string {
	const p = parts(date, zone, 'clock', { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' });
	return `${p.hour}:${p.minute}`;
}

/** "16 Nov" (and the year when it is not `now`'s year). */
export function formatDate(date: Date, zone: string, now: Date = date): string {
	const p = parts(date, zone, 'date', { day: 'numeric', month: 'short', year: 'numeric' });
	const thisYear = parts(now, zone, 'date', { year: 'numeric' }).year;
	return p.year === thisYear ? `${p.day} ${p.month}` : `${p.day} ${p.month} ${p.year}`;
}

/** The full, unambiguous form for assistive technology and tooltips: "16 Nov 2026, 14:02 UTC+01:00". */
export function formatFull(date: Date, zone: string): string {
	const p = parts(date, zone, 'full', { day: 'numeric', month: 'short', year: 'numeric' });
	return `${p.day} ${p.month} ${p.year}, ${formatClock(date, zone)} ${offsetLabel(date, zone)}`;
}

/** Calendar day key in the zone, "2026-11-16": two instants are on one day when their keys match. */
export function dayKey(date: Date, zone: string): string {
	const p = parts(date, zone, 'day', { day: '2-digit', month: '2-digit', year: 'numeric' });
	return `${p.year}-${p.month}-${p.day}`;
}

/** "Today", "Yesterday", or "Mon 16 Nov": a group header for a day in the zone. */
export function dayLabel(date: Date, now: Date, zone: string): string {
	if (dayKey(date, zone) === dayKey(now, zone)) return 'Today';
	if (dayKey(date, zone) === dayKey(new Date(now.getTime() - 86_400_000), zone)) return 'Yesterday';
	const weekday = parts(date, zone, 'weekday', { weekday: 'short' }).weekday;
	return `${weekday} ${formatDate(date, zone, now)}`;
}

/** Whole minutes between two instants, from the instants themselves. */
export const minutesBetween = (from: Date, to: Date) =>
	Math.round((to.getTime() - from.getTime()) / 60_000);

/** "just now", "12 min ago", "3 h ago": for an instant less than a day before `now`. */
export function relativeAgo(date: Date, now: Date): string {
	const minutes = minutesBetween(date, now);
	if (minutes < 1) return 'just now';
	if (minutes < 60) return `${minutes} min ago`;
	return `${Math.floor(minutes / 60)} h ago`;
}

/** "in 12 min", "in 3 h", "in 2 d": for an instant after `now`; "expired" once passed. */
export function relativeUntil(date: Date, now: Date): string {
	const minutes = minutesBetween(now, date);
	if (minutes < 1) return 'expired';
	if (minutes < 60) return `in ${minutes} min`;
	if (minutes < 48 * 60) return `in ${Math.floor(minutes / 60)} h`;
	return `in ${Math.floor(minutes / 1440)} d`;
}

/** A short duration from instants: "2 min", "4 min", "1 h 05 min". */
export function durationWords(from: Date, to: Date): string {
	const minutes = Math.max(0, minutesBetween(from, to));
	if (minutes < 1) return 'under 1 min';
	if (minutes < 60) return `${minutes} min`;
	return `${Math.floor(minutes / 60)} h ${String(minutes % 60).padStart(2, '0')} min`;
}

export interface EventTime {
	/** What the row shows. */
	text: string;
	/** Full date, time and offset, for assistive technology and the tooltip. */
	full: string;
	/** The instant, for `<time datetime>`. */
	iso: string;
}

/**
 * Event lists: relative within 24 hours, absolute afterwards (24-hour clock). With `underDayHeader`,
 * older rows show only the clock, because the group header already names the day.
 */
export function eventTime(
	at: string | Date,
	now: Date,
	zone: string,
	underDayHeader = false
): EventTime {
	const date = at instanceof Date ? at : new Date(at);
	const full = formatFull(date, zone);
	const age = now.getTime() - date.getTime();
	let text: string;
	if (age < 86_400_000) text = relativeAgo(date, now);
	else if (underDayHeader) text = formatClock(date, zone);
	else text = `${formatDate(date, zone, now)}, ${formatClock(date, zone)}`;
	return { text, full, iso: date.toISOString() };
}

export interface ZoneOption {
	id: string;
	/** The city or name: "New York", "Buenos Aires". */
	city: string;
	/** The region before it: "America", "Asia". */
	region: string;
	offset: string;
	offsetMinutes: number;
	/** The current local time there, 24-hour. */
	local: string;
}

export function zoneOption(id: string, at: Date): ZoneOption {
	const segments = id.split('/');
	const city = (segments.at(-1) ?? id).replaceAll('_', ' ');
	const region = segments.length > 1 ? segments[0].replaceAll('_', ' ') : '';
	return {
		id,
		city,
		region,
		offset: offsetLabel(at, id),
		offsetMinutes: offsetMinutes(at, id),
		local: formatClock(at, id)
	};
}

/** Every IANA zone this browser lists (plus UTC), by offset then city. */
export function zoneOptions(at: Date): ZoneOption[] {
	let ids: string[] = [];
	try {
		ids = Intl.supportedValuesOf('timeZone');
	} catch {
		ids = [];
	}
	// Some engines still list only the retired names (Asia/Calcutta); offer the current ones too.
	const modern = [
		'UTC',
		'Asia/Kolkata',
		'Europe/Kyiv',
		'Asia/Ho_Chi_Minh',
		'Asia/Kathmandu',
		'Asia/Yangon'
	];
	ids = [...ids, ...modern.filter((id) => !ids.includes(id) && isValidZone(id))];
	return ids
		.map((id) => zoneOption(id, at))
		.sort((a, b) => a.offsetMinutes - b.offsetMinutes || a.city.localeCompare(b.city));
}

/** Zones whose city, region, id or offset contain every word of `query`. */
export function searchZones(options: ZoneOption[], query: string): ZoneOption[] {
	const words = query.toLowerCase().split(/\s+/).filter(Boolean);
	if (words.length === 0) return options;
	return options.filter((o) => {
		const haystack =
			`${o.city} ${o.region} ${o.id} ${o.offset} ${o.offset.replace('UTC', 'GMT')}`.toLowerCase();
		return words.every((word) => haystack.includes(word));
	});
}
