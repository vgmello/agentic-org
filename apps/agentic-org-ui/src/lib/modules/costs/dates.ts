// Dates in a given IANA zone, for cost periods and budget bounds. Pure: every function takes the instant and the zone.
// Instants are kept exactly; these only format them, and compare calendar days by their keys.

const formats = new Map<string, Intl.DateTimeFormat>();
function parts(date: Date, zone: string, options: Intl.DateTimeFormatOptions) {
	const id = `${zone}|${JSON.stringify(options)}`;
	let format = formats.get(id);
	if (!format) {
		format = new Intl.DateTimeFormat('en-GB', { timeZone: zone, ...options });
		formats.set(id, format);
	}
	const out: Record<string, string> = {};
	for (const part of format.formatToParts(date)) out[part.type] = part.value;
	return out;
}

/** The calendar day in the zone: "2026-11-19". */
export function dayKey(date: Date, zone: string): string {
	const p = parts(date, zone, { day: '2-digit', month: '2-digit', year: 'numeric' });
	return `${p.year}-${p.month}-${p.day}`;
}

/** "19 Nov 2026". */
export function formatDay(date: Date, zone: string): string {
	const p = parts(date, zone, { day: 'numeric', month: 'short', year: 'numeric' });
	return `${p.day} ${p.month} ${p.year}`;
}

/** "19 Nov 2026, 14:02". */
export function formatDayTime(date: Date, zone: string): string {
	const p = parts(date, zone, {
		day: 'numeric',
		month: 'short',
		year: 'numeric',
		hour: '2-digit',
		minute: '2-digit',
		hourCycle: 'h23'
	});
	return `${p.day} ${p.month} ${p.year}, ${p.hour}:${p.minute}`;
}

/** "14:02". */
export function formatClock(date: Date, zone: string): string {
	const p = parts(date, zone, { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' });
	return `${p.hour}:${p.minute}`;
}

/** A `YYYY-MM-DD` key as "19 Nov 2026", without moving it through any zone. */
export function formatDayKey(key: string): string {
	const [y, m, d] = key.split('-').map(Number);
	return formatDay(new Date(Date.UTC(y, m - 1, d, 12)), 'UTC');
}

/** Whether a string is a real calendar date written `YYYY-MM-DD`. */
export function isDayKey(value: string): boolean {
	const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
	if (!match) return false;
	const [y, m, d] = [Number(match[1]), Number(match[2]), Number(match[3])];
	const date = new Date(Date.UTC(y, m - 1, d));
	return date.getUTCFullYear() === y && date.getUTCMonth() === m - 1 && date.getUTCDate() === d;
}
