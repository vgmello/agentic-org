// Shared aggregate duration formatting (costs-and-insights-ux "Shared duration definitions", TIME-4).
// Whole seconds before aggregation: floor to whole minutes below 1 h, whole hours below 48 h, then days with one
// decimal (floor(seconds * 10 / 86400) / 10). Null stays "—" (unknown is never turned into zero).

const MINUTE = 60;
const HOUR = 3_600;
const DAY = 86_400;

/**
 * Formats an aggregate duration in seconds per TIME-4:
 * - 59m59s (3599s) -> "59 min"
 * - 47h59m (172740s) -> "47 h"
 * - 48h (172800s) -> "2.0 d"
 * - 126h (453600s) -> "5.2 d"
 */
export function formatDuration(seconds: number | null | undefined): string {
	if (seconds === null || seconds === undefined || !Number.isFinite(seconds) || seconds < 0)
		return '—';
	const whole = Math.floor(seconds);
	if (whole < HOUR) return `${Math.floor(whole / MINUTE)} min`;
	if (whole < 48 * HOUR) return `${Math.floor(whole / HOUR)} h`;
	const tenths = Math.floor((whole * 10) / DAY) / 10;
	return `${tenths.toFixed(1)} d`;
}

/** Accessible precise wording for assistive technology / title tooltips. */
export function formatPreciseDuration(seconds: number | null | undefined): string {
	if (seconds === null || seconds === undefined || !Number.isFinite(seconds) || seconds < 0)
		return 'Duration unavailable';
	const whole = Math.floor(seconds);
	const days = Math.floor(whole / DAY);
	const hours = Math.floor((whole % DAY) / HOUR);
	const minutes = Math.floor((whole % HOUR) / MINUTE);
	const parts: string[] = [];
	if (days > 0) parts.push(`${days} ${days === 1 ? 'day' : 'days'}`);
	if (hours > 0) parts.push(`${hours} ${hours === 1 ? 'hour' : 'hours'}`);
	if (minutes > 0 || parts.length === 0)
		parts.push(`${minutes} ${minutes === 1 ? 'minute' : 'minutes'}`);
	return parts.join(' ');
}

/**
 * Formats a median duration with its sample count (doc-51 §8 / TIME-2):
 * below 5 items, shows "— (n items)" rather than a noisy median.
 */
export function formatMedianDuration(
	seconds: number | null | undefined,
	sampleCount: number
): { value: string; note?: string } {
	if (sampleCount < 5 || seconds === null || seconds === undefined) {
		return {
			value: '—',
			note:
				sampleCount === 0
					? 'No completed items'
					: `${sampleCount} ${sampleCount === 1 ? 'item' : 'items'} (needs 5 for median)`
		};
	}
	return {
		value: formatDuration(seconds),
		note: `${sampleCount} items`
	};
}

export const WORKING_TIME_NOT_TRACKED = 'not tracked yet';
