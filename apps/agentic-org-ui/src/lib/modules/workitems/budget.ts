// Pure budget arithmetic, exact micro-USD parsing, and chip copy (costs-and-insights-ux §2, §4; BUD-1..3).
import type { BudgetUnit, BudgetWord } from './types';

/** Error code on a write whose outcome is unknown (connection dropped before acknowledgement). */
export const OUTCOME_UNKNOWN = 'outcome_unknown';

const MICRO_PER_DOLLAR = 1_000_000;

/**
 * Converts a dollar string ("10", "6.40", "0.000001") to exact integer micro-USD without floating-point drift.
 * Refuses zero, negative, non-numeric, or more than 6 fractional digits.
 */
export function parseDollarsToMicroUsd(
	raw: string
): { ok: true; microUsd: number } | { ok: false; error: string } {
	const trimmed = raw.trim().replace(/^\$/, '').replaceAll(',', '');
	if (!trimmed) return { ok: false, error: 'Enter a budget amount.' };
	if (!/^\d+(?:\.\d+)?$/.test(trimmed))
		return { ok: false, error: 'Enter a positive dollar amount, for example 10 or 12.50.' };
	const [whole, frac = ''] = trimmed.split('.');
	if (frac.length > 6)
		return { ok: false, error: 'Dollar limits support up to six decimal places (1 micro-USD).' };
	const paddedFrac = frac.padEnd(6, '0');
	const microUsd = Number(whole) * MICRO_PER_DOLLAR + Number(paddedFrac);
	if (!Number.isSafeInteger(microUsd) || microUsd <= 0)
		return { ok: false, error: 'Enter an amount greater than zero.' };
	return { ok: true, microUsd };
}

/** Parses a positive integer token limit ("500000", "1,000,000"). */
export function parseTokenLimit(
	raw: string
): { ok: true; tokens: number } | { ok: false; error: string } {
	const trimmed = raw.trim().replaceAll(',', '');
	if (!trimmed) return { ok: false, error: 'Enter a token limit.' };
	if (!/^\d+$/.test(trimmed)) return { ok: false, error: 'Enter a whole number of tokens.' };
	const tokens = Number(trimmed);
	if (!Number.isSafeInteger(tokens) || tokens <= 0)
		return { ok: false, error: 'Enter a whole number greater than zero.' };
	return { ok: true, tokens };
}

/** Formats integer micro-USD as US dollars ("$10", "$6.40", "$0.0025"). */
export function formatMicroUsd(microUsd: number): string {
	const neg = microUsd < 0;
	const abs = Math.abs(Math.trunc(microUsd));
	const dollars = Math.floor(abs / MICRO_PER_DOLLAR);
	const rem = abs % MICRO_PER_DOLLAR;
	const whole = dollars.toLocaleString('en-US');
	if (rem === 0) return `${neg ? '-' : ''}$${whole}`;
	const frac = String(rem).padStart(6, '0').replace(/0+$/, '');
	const shownFrac = frac.length === 1 ? `${frac}0` : frac;
	return `${neg ? '-' : ''}$${whole}.${shownFrac}`;
}

/** Plain editable dollar string for a form input ("10", "6.40"). */
export function microUsdToInputValue(microUsd: number): string {
	const abs = Math.max(0, Math.trunc(microUsd));
	const dollars = Math.floor(abs / MICRO_PER_DOLLAR);
	const rem = abs % MICRO_PER_DOLLAR;
	if (rem === 0) return String(dollars);
	const frac = String(rem).padStart(6, '0').replace(/0+$/, '');
	return `${dollars}.${frac.length === 1 ? `${frac}0` : frac}`;
}

/** Formats token totals cleanly ("1.9 M", "420 k", "850"). */
export function formatTokens(tokens: number): string {
	const n = Math.max(0, Math.trunc(tokens));
	if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)} M`;
	if (n >= 10_000) return `${Math.round(n / 1_000)} k`;
	return n.toLocaleString('en-US');
}

export function formatBudgetAmount(unit: BudgetUnit, amount: number): string {
	return unit === 'usd' ? formatMicroUsd(amount) : `${formatTokens(amount)} tokens`;
}

export function budgetPercent(spent: number, limit: number): number {
	if (limit <= 0) return 0;
	return Math.max(0, Math.round((spent / limit) * 100));
}

/** One-time 20% extension (BUD-3): L + floor(L / 5). */
export function extendedLimit(baseLimit: number): number {
	return baseLimit + Math.floor(baseLimit / 5);
}

/** Whether Allow 20% more once would be meaningful and valid (BUD-3). */
export function canAllowOnce(budget: {
	limit: number;
	spent: number;
	extended: boolean;
	hold?: 'stop_pending' | 'held';
}): { ok: true; nextLimit: number } | { ok: false; reason: string } {
	if (budget.extended)
		return { ok: false, reason: 'This budget already used its one 20% extension.' };
	if (!budget.hold && budget.spent < budget.limit)
		return { ok: false, reason: 'Allow 20% once is available only when the budget is reached.' };
	const increment = Math.floor(budget.limit / 5);
	const nextLimit = budget.limit + increment;
	if (increment <= 0 || nextLimit <= budget.spent)
		return {
			ok: false,
			reason: 'Adding 20% would not exceed current spend. Raise the budget instead.'
		};
	return { ok: true, nextLimit };
}

/** Words for budget chips on rows, cards and state-only readers. */
export function budgetChipText(word: BudgetWord, hold?: 'stop_pending' | 'held'): string {
	if (word === 'reached') {
		if (hold === 'stop_pending') return 'Stopping · Budget reached';
		if (hold === 'held') return 'Paused · Budget reached';
		return 'Budget reached';
	}
	if (word === 'warned') return 'Budget 80%';
	return 'Budget OK';
}

/** Visual tone for budget chips: amber is reserved strictly for reached (waiting on the owner). */
export function budgetTone(word: BudgetWord): 'ok' | 'neutral' | 'owner' {
	if (word === 'reached') return 'owner';
	if (word === 'warned') return 'neutral';
	return 'ok';
}
