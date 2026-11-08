import type { TimeFigures } from '$lib/proto';

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;

/** "2 days", "3 hours", "5 minutes", "under a minute". */
export function duration(ms: number): string {
	if (ms >= DAY) return `${Math.floor(ms / DAY)} ${Math.floor(ms / DAY) === 1 ? 'day' : 'days'}`;
	if (ms >= HOUR) return `${Math.floor(ms / HOUR)} ${Math.floor(ms / HOUR) === 1 ? 'hour' : 'hours'}`;
	if (ms >= MIN) return `${Math.floor(ms / MIN)} ${Math.floor(ms / MIN) === 1 ? 'minute' : 'minutes'}`;
	return 'under a minute';
}

/** The Time block's first line (TSK-11): lead time, "so far" while the item is open. */
export const elapsed = (t: TimeFigures): string =>
	t.open ? `Open for ${duration(t.lead_ms)} so far` : `Done ${duration(t.lead_ms)} after it was created${t.cycle_ms === undefined ? '' : `, ${duration(t.cycle_ms)} after it first started`}`;
