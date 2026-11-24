// Pure presentation helpers for provider-reported subscription model usage (TASK-79).
// Every null or undefined counter renders as "—", never fabricated as zero.
import { formatDayTime } from '../../dates';
import type {
	SubscriptionModelUsage,
	SubscriptionUsageAuthSource,
	SubscriptionUsageStatus,
	SubscriptionUsageWindow
} from '../../types';

export const SUBSCRIPTION_USAGE_NOTICE =
	'Provider-reported subscription usage, not API-equivalent USD cost estimates.';

const INT_FORMAT = new Intl.NumberFormat('en-US');

/** Formats utilization basis points (`0..=10000`) as a percentage string, or "—" when unknown. */
export function formatUtilization(bps: number | null | undefined): string {
	if (bps === null || bps === undefined || !Number.isFinite(bps)) return '—';
	const clamped = Math.max(0, Math.min(10_000, Math.round(bps)));
	if (clamped % 100 === 0) return `${clamped / 100}%`;
	const tenths = Math.round(clamped / 10);
	const whole = Math.floor(tenths / 10);
	const rem = tenths % 10;
	return rem === 0 ? `${whole}%` : `${whole}.${rem}%`;
}

/** Formats an integer request count, or "—" when unknown. */
export function formatRequests(count: number | null | undefined): string {
	if (count === null || count === undefined || !Number.isFinite(count) || count < 0) return '—';
	return INT_FORMAT.format(Math.round(count));
}

/** Formats a provider-reported token count concisely, or "—" when unknown. */
export function formatSubscriptionTokens(tokens: number | null | undefined): string {
	if (tokens === null || tokens === undefined || !Number.isFinite(tokens) || tokens < 0) return '—';
	const n = Math.round(tokens);
	if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(1).replace(/\.0$/, '')} B`;
	if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1).replace(/\.0$/, '')} M`;
	if (n >= 1_000) return `${(n / 1_000).toFixed(1).replace(/\.0$/, '')} K`;
	return INT_FORMAT.format(n);
}

/** Formats window duration in seconds (e.g. `18000` -> `5h`, `86400` -> `24h`, `604800` -> `7d`). */
export function formatWindowDuration(seconds: number | null | undefined): string | undefined {
	if (seconds === null || seconds === undefined || !Number.isFinite(seconds) || seconds <= 0) {
		return undefined;
	}
	const s = Math.round(seconds);
	if (s >= 86_400 && s % 86_400 === 0) {
		const days = s / 86_400;
		return days === 1 ? '24h' : `${days}d`;
	}
	if (s >= 3_600 && s % 3_600 === 0) return `${s / 3_600}h`;
	if (s >= 60 && s % 60 === 0) return `${s / 60}m`;
	return `${s}s`;
}

export interface WindowCellSummary {
	utilization: string;
	requests: string;
	tokens: string;
	hasUtilization: boolean;
	hasMetrics: boolean;
	durationLabel?: string;
}

/** Summarizes one usage window without fabricating zeros for null/missing counters. */
export function summarizeWindow(win: SubscriptionUsageWindow | undefined): WindowCellSummary {
	if (!win) {
		return {
			utilization: '—',
			requests: '—',
			tokens: '—',
			hasUtilization: false,
			hasMetrics: false
		};
	}
	const utilization = formatUtilization(win.utilizationBps);
	const requests = formatRequests(win.requests);
	const tokens = formatSubscriptionTokens(win.totalTokens);
	const hasUtilization = utilization !== '—';
	const hasMetrics = hasUtilization || requests !== '—' || tokens !== '—';
	return {
		utilization,
		requests,
		tokens,
		hasUtilization,
		hasMetrics,
		durationLabel: formatWindowDuration(win.windowSeconds)
	};
}

export function hasAnyShortTerm(models: readonly SubscriptionModelUsage[]): boolean {
	return models.some((m) => m.shortTerm !== undefined);
}

export interface SubscriptionStatusDisplay {
	label: string;
	tone: 'ok' | 'owner' | 'quiet';
}

export function subscriptionStatusDisplay(
	status: SubscriptionUsageStatus,
	stale: boolean
): SubscriptionStatusDisplay {
	if (status === 'ok') {
		return stale ? { label: 'Stale', tone: 'owner' } : { label: 'Current', tone: 'ok' };
	}
	switch (status) {
		case 'auth_required':
			return { label: 'Sign-in needed', tone: 'owner' };
		case 'unavailable':
			return { label: 'Unavailable', tone: 'owner' };
		case 'unsupported':
			return { label: 'Not supported', tone: 'quiet' };
		case 'never_collected':
			return { label: 'Not collected yet', tone: 'quiet' };
		default:
			return { label: 'Unknown', tone: 'quiet' };
	}
}

export function authSourceLabel(source: SubscriptionUsageAuthSource): string {
	switch (source) {
		case 'dedicated_usage_credential':
			return 'Dedicated usage credential';
		case 'enrolled_subscription_credential':
			return 'Enrolled subscription credential';
		case 'none':
			return 'No credential configured';
		default:
			return 'Unknown credential source';
	}
}

export function providerLabel(providerType: string): string {
	const key = providerType.trim().toLowerCase();
	if (key === 'claude' || key === 'anthropic' || key === 'claude_code') return 'Claude';
	if (key === 'codex' || key === 'openai') return 'Codex';
	return providerType || 'Provider';
}

export function formatCollectedAt(iso: string | undefined, zone: string): string | undefined {
	if (!iso) return undefined;
	const parsed = new Date(iso);
	if (Number.isNaN(parsed.getTime())) return undefined;
	return formatDayTime(parsed, zone);
}
