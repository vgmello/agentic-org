import { describe, expect, it } from 'vitest';
import {
	authSourceLabel,
	collectOutcomeToast,
	credentialConfigLabel,
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
import type { SubscriptionUsageSnapshot } from '../../types';

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

		// Reported component tokens must stay visible without fabricating totalTokens when provider omits it.
		expect(
			summarizeWindow({
				inputTokens: 1000,
				outputTokens: null,
				totalTokens: null
			})
		).toEqual({
			utilization: '—',
			requests: '—',
			tokens: '—',
			tokenBreakdown: '1 K in',
			hasUtilization: false,
			hasMetrics: true,
			durationLabel: undefined
		});

		expect(
			summarizeWindow({
				inputTokens: 12_500,
				outputTokens: 4200,
				cacheReadTokens: 800,
				cacheWriteTokens: 150,
				totalTokens: null
			})
		).toMatchObject({
			tokens: '—',
			tokenBreakdown: '12.5 K in · 4.2 K out · 800 cache read · 150 cache write',
			hasMetrics: true
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
		expect(
			credentialConfigLabel({
				authSource: 'none',
				hasUsageCredential: true,
				hasEnrolledCredential: false
			})
		).toBe('Dedicated usage credential saved');
		expect(
			credentialConfigLabel({
				authSource: 'none',
				hasUsageCredential: false,
				hasEnrolledCredential: true
			})
		).toBe('Enrolled subscription credential available');
		expect(
			credentialConfigLabel({
				authSource: 'dedicated_usage_credential',
				hasUsageCredential: false,
				hasEnrolledCredential: false
			})
		).toBe('No credential configured (last used dedicated usage credential)');
		expect(
			credentialConfigLabel({
				authSource: 'dedicated_usage_credential',
				hasUsageCredential: false,
				hasEnrolledCredential: true
			})
		).toBe('Enrolled subscription credential available');

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

	it('preserves deferred retry instructions alongside unsupported, unavailable, and auth_required collection outcomes', () => {
		const baseSnap = (
			overrides: Partial<SubscriptionUsageSnapshot>
		): SubscriptionUsageSnapshot => ({
			connectionId: 'conn-1',
			connectionKey: 'claude-sub',
			connectionName: 'Claude Max',
			providerType: 'claude',
			officeId: 'office-1',
			authSource: 'dedicated_usage_credential',
			hasUsageCredential: true,
			hasEnrolledCredential: true,
			status: 'ok',
			stale: false,
			isApiEquivalentEstimate: false,
			models: [],
			...overrides
		});

		expect(
			collectOutcomeToast({
				snapshots: [
					baseSnap({
						status: 'unsupported',
						message: 'Provider does not expose a subscription usage endpoint for this account.'
					})
				],
				collectedAt: '2026-11-24T12:00:00Z',
				deferredConnectionIds: ['conn-2']
			})
		).toEqual({
			tone: 'info',
			message:
				'Provider does not expose a subscription usage endpoint for this account. 1 connection was deferred; refresh again for remaining connections.'
		});

		expect(
			collectOutcomeToast({
				snapshots: [
					baseSnap({
						status: 'auth_required',
						message: 'Usage token expired.'
					})
				],
				collectedAt: '2026-11-24T12:00:00Z',
				deferredConnectionIds: ['conn-2', 'conn-3']
			})
		).toEqual({
			tone: 'error',
			message:
				'Usage token expired. 2 connections were deferred; refresh again for remaining connections.'
		});

		expect(
			collectOutcomeToast({
				snapshots: [
					baseSnap({
						status: 'unavailable',
						message: 'Upstream returned 503.'
					})
				],
				collectedAt: '2026-11-24T12:00:00Z',
				deferredConnectionIds: ['conn-2']
			})
		).toEqual({
			tone: 'error',
			message:
				'Upstream returned 503. 1 connection was deferred; refresh again for remaining connections.'
		});

		expect(
			collectOutcomeToast({
				snapshots: [
					baseSnap({
						status: 'unsupported',
						message: 'Provider does not expose a subscription usage endpoint for this account.'
					})
				],
				collectedAt: '2026-11-24T12:00:00Z'
			})
		).toEqual({
			tone: 'info',
			message: 'Provider does not expose a subscription usage endpoint for this account.'
		});

		expect(
			collectOutcomeToast({
				snapshots: [baseSnap({ status: 'ok' })],
				collectedAt: '2026-11-24T12:00:00Z',
				deferredConnectionIds: ['conn-2']
			})
		).toEqual({
			tone: 'info',
			message: 'Updated subscription usage (1 deferred; refresh again for remaining connections).'
		});

		expect(
			collectOutcomeToast({
				snapshots: [baseSnap({ status: 'ok' })],
				collectedAt: '2026-11-24T12:00:00Z'
			})
		).toEqual({
			tone: 'success',
			message: 'Subscription model usage updated.'
		});
	});
});
