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
