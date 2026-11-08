import type { TimeFigures } from '$lib/proto';

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;

/** "1 d 4 h", "3 h", "5 min", "0": the two largest units that apply (work-items-ux §9.2). */
export function span(ms: number): string {
	if (ms >= DAY) {
		const h = Math.floor((ms % DAY) / HOUR);
		return h > 0 ? `${Math.floor(ms / DAY)} d ${h} h` : `${Math.floor(ms / DAY)} d`;
	}
	if (ms >= HOUR) {
		const m = Math.floor((ms % HOUR) / MIN);
		return m > 0 ? `${Math.floor(ms / HOUR)} h ${m} min` : `${Math.floor(ms / HOUR)} h`;
	}
	if (ms >= MIN) return `${Math.floor(ms / MIN)} min`;
	return ms > 0 ? '<1 min' : '0';
}

/**
 * The Time block (TSK-11): lead time, waiting on you, blocked, and the rest, which stands in for idle while working
 * time isn't tracked. An open item says "so far".
 */
export function timeBlock(t: TimeFigures): string {
	const rest = Math.max(0, t.lead_ms - t.waiting_on_owner_ms - t.blocked_ms - (t.working_ms ?? 0));
	const tail = t.working_ms === undefined ? ` (working time isn't tracked yet)` : '';
	return `Lead time ${span(t.lead_ms)}${t.open ? ' so far' : ''} · waiting on you ${span(t.waiting_on_owner_ms)} · blocked ${span(t.blocked_ms)} · the rest ${span(rest)}${tail}`;
}
