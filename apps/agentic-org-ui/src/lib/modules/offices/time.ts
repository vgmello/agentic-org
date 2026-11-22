// Relative times for the office rows: "2 min ago", "expires in 21 h". Rounded down, never claiming more than is known.

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;

export function agoText(iso: string, now: number): string {
	const ms = Math.max(0, now - Date.parse(iso));
	if (ms < MINUTE) return 'just now';
	if (ms < HOUR) return `${Math.floor(ms / MINUTE)} min ago`;
	if (ms < 48 * HOUR) return `${Math.floor(ms / HOUR)} h ago`;
	return `${Math.floor(ms / (24 * HOUR))} days ago`;
}

/** "expires in 21 h", "expires in 12 min", or "expired". */
export function expiresText(iso: string, now: number): string {
	const ms = Date.parse(iso) - now;
	if (ms <= 0) return 'expired';
	if (ms < MINUTE) return 'expires in under a minute';
	if (ms < HOUR) return `expires in ${Math.floor(ms / MINUTE)} min`;
	return `expires in ${Math.floor(ms / HOUR)} h`;
}

const clock = new Intl.DateTimeFormat(undefined, {
	hour: '2-digit',
	minute: '2-digit',
	hourCycle: 'h23'
});
const day = new Intl.DateTimeFormat(undefined, {
	weekday: 'short',
	day: 'numeric',
	month: 'short'
});
const startOfDay = (ms: number) => {
	const d = new Date(ms);
	return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
};

/** When something was made, in the viewer's day: "today at 14:02", "yesterday at 23:00", "on Mon 16 Nov at 09:15". */
export function madeAtText(iso: string, now: number): string {
	const at = Date.parse(iso);
	const days = Math.round((startOfDay(now) - startOfDay(at)) / (24 * HOUR));
	const time = clock.format(at);
	if (days === 0) return `today at ${time}`;
	if (days === 1) return `yesterday at ${time}`;
	return `on ${day.format(at)} at ${time}`;
}
