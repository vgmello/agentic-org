// Money and token figures, in words a person reads (costs-and-insights-ux §0 decisions 2 and 3). Amounts arrive as
// integer micro-USD and are formatted with integer arithmetic; nothing here turns an unknown amount into zero.

const MICRO = 1_000_000;
const grouped = new Intl.NumberFormat('en-US');

/** "$1,240.50": micro-USD rounded half up to whole cents. A known amount below half a cent reads "< $0.01". */
export function formatUsd(microUsd: number): string {
	if (!Number.isFinite(microUsd)) return '—';
	const negative = microUsd < 0;
	const abs = Math.abs(microUsd);
	if (abs > 0 && abs < 5_000) return negative ? '−< $0.01' : '< $0.01';
	const cents = Math.floor((abs + 5_000) / 10_000);
	const text = `$${grouped.format(Math.floor(cents / 100))}.${String(cents % 100).padStart(2, '0')}`;
	return negative ? `−${text}` : text;
}

/** An estimated cost: "≈ $412.00"; unknown is "—" (the caller says why next to it). */
export function formatCost(microUsd: number | null): string {
	return microUsd === null ? '—' : `≈ ${formatUsd(microUsd)}`;
}

/** A rate in dollars per million tokens, exact: "$3.00", "$0.30", "$0.125"; an unknown rate is "—". */
export function formatRate(microUsdPerMillion: number | null): string {
	if (microUsdPerMillion === null) return '—';
	const whole = Math.floor(microUsdPerMillion / MICRO);
	let fraction = String(microUsdPerMillion % MICRO)
		.padStart(6, '0')
		.replace(/0+$/, '');
	if (fraction.length < 2) fraction = fraction.padEnd(2, '0');
	return `$${grouped.format(whole)}.${fraction}`;
}

/** A rate as an editable value: "3", "0.125"; blank for unknown. */
export function rateInput(microUsdPerMillion: number | null): string {
	if (microUsdPerMillion === null) return '';
	const whole = Math.floor(microUsdPerMillion / MICRO);
	const fraction = String(microUsdPerMillion % MICRO)
		.padStart(6, '0')
		.replace(/0+$/, '');
	return fraction ? `${whole}.${fraction}` : String(whole);
}

/** The most a rate may be, in micro-USD per million tokens ($1,000,000): a typo guard, not a business rule. */
export const MAX_RATE = 1_000_000 * MICRO;

export type ParsedRate = { ok: true; value: number | null } | { ok: false; error: string };

/**
 * A dollar rate typed by a person, to exact micro-USD: "3", "3.50", "$0.125", "1,000". Blank is an unknown rate.
 * Accepts a leading $, spaces and thousands commas; refuses negatives, more than six decimals and anything else.
 */
export function parseRate(input: string): ParsedRate {
	const text = input
		.trim()
		.replace(/^\$\s*/, '')
		.replaceAll(',', '')
		.replaceAll(' ', '');
	if (text === '') return { ok: true, value: null };
	const match = /^(\d+)(?:\.(\d*))?$|^\.(\d+)$/.exec(text);
	if (!match) return { ok: false, error: 'Enter an amount in dollars, such as 3 or 0.30.' };
	const whole = match[1] ?? '0';
	const fraction = match[2] ?? match[3] ?? '';
	if (fraction.length > 6)
		return { ok: false, error: 'Use at most six decimal places (a millionth of a dollar).' };
	const value = Number(whole) * MICRO + Number(fraction.padEnd(6, '0') || '0');
	if (!Number.isSafeInteger(value) || value > MAX_RATE)
		return { ok: false, error: 'That rate is larger than any provider charges. Check the amount.' };
	return { ok: true, value };
}

/** "61.2 M", "940 k", "512"; unknown is "—". */
export function formatTokens(tokens: number | null): string {
	if (tokens === null) return '—';
	if (tokens >= 1_000_000_000) return scaledUnit(tokens, 1_000_000_000, 'B');
	if (tokens >= 1_000_000) return scaledUnit(tokens, 1_000_000, 'M');
	if (tokens >= 10_000) return scaledUnit(tokens, 1_000, 'k');
	return grouped.format(tokens);
}

function scaledUnit(tokens: number, size: number, unit: string): string {
	// Floor to one decimal so a figure never reads larger than it is.
	const tenths = Math.floor((tokens * 10) / size) / 10;
	return `${Number.isInteger(tenths) ? tenths.toFixed(0) : tenths.toFixed(1)} ${unit}`;
}

/** An exact count with separators: "12,000"; unknown is "—". */
export function formatCount(n: number | null): string {
	return n === null ? '—' : grouped.format(n);
}

/** A budget amount in its unit: "$20.00" or "25 M tokens". */
export function formatBudgetAmount(unit: 'usd' | 'tokens', amount: number): string {
	return unit === 'usd' ? formatUsd(amount) : `${formatTokens(amount)} tokens`;
}

/** Editable plain string for an existing budget limit ("20.00" or "25000000"). */
export function budgetInputValue(unit: 'usd' | 'tokens', amount: number): string {
	if (unit === 'tokens') return String(Math.max(0, Math.trunc(amount)));
	return rateInput(amount);
}

/** Parses a positive USD budget limit to exact integer micro-USD (no floats, rejects <= 0). */
export function parseBudgetUsd(
	input: string
): { ok: true; value: number } | { ok: false; error: string } {
	const parsed = parseRate(input);
	if (!parsed.ok) {
		if (parsed.error.includes('provider charges')) {
			return { ok: false, error: 'That budget amount is too large. Check the amount.' };
		}
		return parsed;
	}
	if (parsed.value === null) return { ok: false, error: 'Enter a budget amount.' };
	if (parsed.value <= 0) return { ok: false, error: 'Enter an amount greater than zero.' };
	return { ok: true, value: parsed.value };
}

/** Parses a positive whole-number token budget limit. */
export function parseBudgetTokens(
	input: string
): { ok: true; value: number } | { ok: false; error: string } {
	const text = input.trim().replaceAll(',', '').replaceAll(' ', '');
	if (!text) return { ok: false, error: 'Enter a token limit.' };
	if (!/^\d+$/.test(text)) return { ok: false, error: 'Enter a whole number of tokens.' };
	const value = Number(text);
	if (!Number.isSafeInteger(value) || value <= 0)
		return { ok: false, error: 'Enter a whole number greater than zero.' };
	return { ok: true, value };
}

/** Document 17 one-time 20% extension: L + floor(L / 5). */
export function extendedBudgetLimit(baseLimit: number): number {
	return baseLimit + Math.floor(baseLimit / 5);
}
