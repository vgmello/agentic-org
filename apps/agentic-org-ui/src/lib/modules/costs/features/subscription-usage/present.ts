// Pure presentation helpers for provider-reported subscription model usage (TASK-79).
// Every null or undefined counter renders as "—", never fabricated as zero.
import { formatDayTime } from '../../dates';
import type {
	SubscriptionCollectResult,
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
	/** Provider-reported component token counts when any are present (kept distinct from omitted totals). */
	tokenBreakdown?: string;
	hasUtilization: boolean;
	hasMetrics: boolean;
	durationLabel?: string;
}

/** Formats any provider-reported component token counts without fabricating a sum. */
export function formatTokenBreakdown(win: SubscriptionUsageWindow): string | undefined {
	const parts: string[] = [];
	const input = formatSubscriptionTokens(win.inputTokens);
	if (input !== '—') parts.push(`${input} in`);
	const output = formatSubscriptionTokens(win.outputTokens);
	if (output !== '—') parts.push(`${output} out`);
	const cacheRead = formatSubscriptionTokens(win.cacheReadTokens);
	if (cacheRead !== '—') parts.push(`${cacheRead} cache read`);
	const cacheWrite = formatSubscriptionTokens(win.cacheWriteTokens);
	if (cacheWrite !== '—') parts.push(`${cacheWrite} cache write`);
	return parts.length > 0 ? parts.join(' · ') : undefined;
}

/** Summarizes one usage window without fabricating zeros or totals for null/missing counters. */
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
	const tokenBreakdown = formatTokenBreakdown(win);
	const hasUtilization = utilization !== '—';
	const hasMetrics =
		hasUtilization || requests !== '—' || tokens !== '—' || Boolean(tokenBreakdown);
	return {
		utilization,
		requests,
		tokens,
		...(tokenBreakdown ? { tokenBreakdown } : {}),
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

/**
 * Human-readable credential status driven by live office credential presence flags
 * (`hasUsageCredential` / `hasEnrolledCredential`), with historical `authSource` labeled
 * separately so a revoked credential never appears currently configured.
 */
export function credentialConfigLabel(snapshot: {
	authSource: SubscriptionUsageAuthSource;
	hasUsageCredential: boolean;
	hasEnrolledCredential: boolean;
}): string {
	if (snapshot.hasUsageCredential) {
		return snapshot.authSource === 'dedicated_usage_credential'
			? 'Dedicated usage credential'
			: 'Dedicated usage credential saved';
	}
	if (snapshot.hasEnrolledCredential) {
		return snapshot.authSource === 'enrolled_subscription_credential'
			? 'Enrolled subscription credential'
			: 'Enrolled subscription credential available';
	}
	if (snapshot.authSource === 'dedicated_usage_credential') {
		return 'No credential configured (last used dedicated usage credential)';
	}
	if (snapshot.authSource === 'enrolled_subscription_credential') {
		return 'No credential configured (last used enrolled subscription credential)';
	}
	return 'No credential configured';
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

export interface CollectToastFeedback {
	tone: 'success' | 'info' | 'error';
	message: string;
}

/**
 * Formats the post-collection toast from the office response, preserving both in-band snapshot
 * statuses (`auth_required`, `unavailable`, `unsupported`) and any `deferredConnectionIds` retry
 * instruction on mixed batch responses.
 */
export function collectOutcomeToast(res: SubscriptionCollectResult): CollectToastFeedback {
	const deferredCount = res.deferredConnectionIds?.length ?? 0;
	const deferredNote =
		deferredCount > 0
			? `${deferredCount} ${deferredCount === 1 ? 'connection was' : 'connections were'} deferred; refresh again for remaining connections.`
			: '';
	const withDeferred = (base: string): string => {
		if (!deferredNote) return base;
		return `${base.trimEnd().replace(/\.$/, '')}. ${deferredNote}`;
	};

	const authFailed = res.snapshots.find((s) => s.status === 'auth_required');
	if (authFailed) {
		return {
			tone: 'error',
			message: withDeferred(
				authFailed.message ||
					`Usage credential was rejected for ${authFailed.connectionName}. Update it and try again.`
			)
		};
	}

	const unavailableSnap = res.snapshots.find((s) => s.status === 'unavailable');
	if (unavailableSnap) {
		return {
			tone: 'error',
			message: withDeferred(
				unavailableSnap.message ||
					`Provider usage endpoint was unavailable for ${unavailableSnap.connectionName}.`
			)
		};
	}

	const unsupportedSnap = res.snapshots.find((s) => s.status === 'unsupported');
	if (unsupportedSnap) {
		return {
			tone: 'info',
			message: withDeferred(
				unsupportedSnap.message ||
					`${unsupportedSnap.connectionName} does not support subscription usage collection.`
			)
		};
	}

	if (deferredCount > 0) {
		return {
			tone: 'info',
			message: `Updated subscription usage (${deferredCount} deferred; refresh again for remaining connections).`
		};
	}

	return {
		tone: 'success',
		message: 'Subscription model usage updated.'
	};
}
