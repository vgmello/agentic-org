import { describe, expect, it } from 'vitest';
import {
	authSourceLabel,
	formatRequests,
	formatSubscriptionTokens,
	formatUtilization,
	formatWindowDuration,
	hasAnyShortTerm,
	providerLabel,
	SUBSCRIPTION_USAGE_NOTICE,
	subscriptionStatusDisplay,
	summarizeWindow
} from './present';

describe('subscription usage presentation helpers (TASK-79)', () => {
	it('formats utilization basis points and renders null/undefined as em-dash without fabricating zero', () => {
		expect(formatUtilization(null)).toBe('—');
		expect(formatUtilization(undefined)).toBe('—');
		expect(formatUtilization(Number.NaN)).toBe('—');
		expect(formatUtilization(0)).toBe('0%');
		expect(formatUtilization(4200)).toBe('42%');
		expect(formatUtilization(4250)).toBe('42.5%');
		expect(formatUtilization(10_000)).toBe('100%');
		expect(formatUtilization(12_000)).toBe('100%');
	});

	it('formats requests and tokens while preserving em-dash for unknown counters', () => {
		expect(formatRequests(null)).toBe('—');
		expect(formatRequests(undefined)).toBe('—');
		expect(formatRequests(0)).toBe('0');
		expect(formatRequests(1240)).toBe('1,240');

		expect(formatSubscriptionTokens(null)).toBe('—');
		expect(formatSubscriptionTokens(undefined)).toBe('—');
		expect(formatSubscriptionTokens(0)).toBe('0');
		expect(formatSubscriptionTokens(850)).toBe('850');
		expect(formatSubscriptionTokens(12_500)).toBe('12.5 K');
		expect(formatSubscriptionTokens(4_200_000)).toBe('4.2 M');
		expect(formatSubscriptionTokens(1_500_000_000)).toBe('1.5 B');
	});

	it('formats window durations and summarizes partial windows honestly', () => {
		expect(formatWindowDuration(18_000)).toBe('5h');
		expect(formatWindowDuration(86_400)).toBe('24h');
		expect(formatWindowDuration(604_800)).toBe('7d');
		expect(formatWindowDuration(null)).toBeUndefined();

		expect(summarizeWindow(undefined)).toEqual({
			utilization: '—',
			requests: '—',
			tokens: '—',
			hasUtilization: false,
			hasMetrics: false
		});

		expect(
			summarizeWindow({
				windowSeconds: 18_000,
				utilizationBps: 3850,
				requests: null,
				totalTokens: null
			})
		).toEqual({
			utilization: '38.5%',
			requests: '—',
			tokens: '—',
			hasUtilization: true,
			hasMetrics: true,
			durationLabel: '5h'
		});
	});

	it('maps statuses, auth sources, providers, short-term presence, and non-API-equivalent notice', () => {
		expect(SUBSCRIPTION_USAGE_NOTICE).toContain('not API-equivalent');
		expect(subscriptionStatusDisplay('ok', false)).toEqual({ label: 'Current', tone: 'ok' });
		expect(subscriptionStatusDisplay('ok', true)).toEqual({ label: 'Stale', tone: 'owner' });
		expect(subscriptionStatusDisplay('auth_required', false)).toEqual({
			label: 'Sign-in needed',
			tone: 'owner'
		});
		expect(subscriptionStatusDisplay('unavailable', true)).toEqual({
			label: 'Unavailable',
			tone: 'owner'
		});
		expect(subscriptionStatusDisplay('unsupported', false)).toEqual({
			label: 'Not supported',
			tone: 'quiet'
		});
		expect(subscriptionStatusDisplay('never_collected', false)).toEqual({
			label: 'Not collected yet',
			tone: 'quiet'
		});

		expect(authSourceLabel('dedicated_usage_credential')).toBe('Dedicated usage credential');
		expect(authSourceLabel('enrolled_subscription_credential')).toBe(
			'Enrolled subscription credential'
		);
		expect(authSourceLabel('none')).toBe('No credential configured');

		expect(providerLabel('claude')).toBe('Claude');
		expect(providerLabel('codex')).toBe('Codex');
		expect(providerLabel('custom')).toBe('custom');

		expect(
			hasAnyShortTerm([{ model: 'claude-sonnet-4-5', total: {}, daily: {}, weekly: {} }])
		).toBe(false);
		expect(
			hasAnyShortTerm([
				{
					model: 'claude-sonnet-4-5',
					total: {},
					daily: {},
					weekly: {},
					shortTerm: { windowSeconds: 18_000, utilizationBps: 2400 }
				}
			])
		).toBe(true);
	});
});
