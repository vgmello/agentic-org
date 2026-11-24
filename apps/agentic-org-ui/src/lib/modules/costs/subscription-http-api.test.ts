import { describe, expect, it, vi } from 'vitest';
import type { GatewayHttpTransport, GatewayReply } from '$lib/platform/http/gateway-errors';
import { createHttpSubscriptionUsageApi } from './subscription-http-api';

const ORG = '0193a5c0-0000-7000-8000-000000000001';
const BASE = `/api/v1/organizations/${ORG}/costs/subscriptions/usage`;

function ok(result: unknown): GatewayReply {
	return { status: 200, body: { result } };
}

describe('createHttpSubscriptionUsageApi (real cost_routes.rs wire envelope)', () => {
	it('decodes snake_case subscription usage responses, preserves nulls, and follows pagination cursors', async () => {
		const read = vi
			.fn()
			.mockResolvedValueOnce(
				ok({
					snapshots: [
						{
							connection_id: 'conn-1',
							connection_key: 'claude-max',
							connection_name: 'Claude Team Max',
							provider_type: 'claude',
							office_id: 'off-1',
							status: 'ok',
							auth_source: 'enrolled_subscription_credential',
							has_usage_credential: false,
							has_enrolled_credential: true,
							account_hint: 'eng@lumen.dev',
							collected_at: '2026-11-19T14:52:00.000Z',
							last_ok_collected_at: '2026-11-19T14:52:00.000Z',
							stale: false,
							is_api_equivalent_estimate: false,
							models: [
								{
									model: 'claude-opus-4-1',
									total: { utilization_bps: null, requests: 342, total_tokens: 18_400_000 },
									daily: { window_seconds: 86_400, utilization_bps: 4200, requests: 48 },
									weekly: { window_seconds: 604_800, utilization_bps: 6150 },
									short_term: {
										window_seconds: 18_000,
										utilization_bps: 3850,
										requests: null,
										total_tokens: null
									}
								}
							]
						}
					],
					totals_by_model: [
						{
							model: 'claude-opus-4-1',
							total: { requests: 342 },
							daily: { utilization_bps: 4200 },
							weekly: { utilization_bps: 6150 }
						}
					],
					collected_as_of: '2026-11-19T14:52:00.000Z',
					next_cursor: 'page-2',
					partial_totals: false,
					excluded_snapshot_count: 0,
					is_api_equivalent_estimate: false
				})
			)
			.mockResolvedValueOnce(
				ok({
					snapshots: [
						{
							connection_id: 'conn-2',
							connection_key: 'codex-pro',
							connection_name: 'Codex Pro',
							provider_type: 'codex',
							office_id: 'off-1',
							status: 'never_collected',
							auth_source: 'dedicated_usage_credential',
							has_usage_credential: true,
							has_enrolled_credential: false,
							stale: true,
							is_api_equivalent_estimate: false,
							models: []
						}
					],
					totals_by_model: [],
					partial_totals: true,
					excluded_snapshot_count: 1,
					is_api_equivalent_estimate: false
				})
			);

		const api = createHttpSubscriptionUsageApi({ http: { read, submit: vi.fn() } });
		const overview = await api.subscriptionUsage(ORG);

		expect(read).toHaveBeenNthCalledWith(1, `${BASE}?limit=100`);
		expect(read).toHaveBeenNthCalledWith(2, `${BASE}?limit=100&cursor=page-2`);
		expect(overview.snapshots).toHaveLength(2);
		expect(overview.snapshots[0].models[0].shortTerm).toEqual({
			windowSeconds: 18_000,
			utilizationBps: 3850,
			requests: null,
			totalTokens: null
		});
		expect(overview.isApiEquivalentEstimate).toBe(false);
	});

	it('submits collect, credential set, and credential revoke with exact snake_case bodies and fails closed on errors', async () => {
		const submit = vi
			.fn()
			.mockResolvedValueOnce(
				ok({
					snapshots: [],
					collected_at: '2026-11-19T15:00:00.000Z',
					deferred_connection_ids: ['conn-2']
				})
			)
			.mockResolvedValueOnce(
				ok({
					credential: {
						connection_id: 'conn-1',
						office_id: 'off-1',
						configured: true,
						configured_at: '2026-11-19T15:01:00.000Z',
						account_hint: 'ops@lumen.dev'
					}
				})
			)
			.mockResolvedValueOnce(
				ok({
					credential: {
						connection_id: 'conn-1',
						office_id: 'off-1',
						configured: false
					}
				})
			)
			.mockResolvedValueOnce({
				status: 403,
				body: { error: { code: 'not_authorized', message: 'Owner only' } }
			});

		const http: GatewayHttpTransport = { read: vi.fn(), submit };
		const api = createHttpSubscriptionUsageApi({ http });

		const collected = await api.collectSubscriptionUsage(ORG, {
			connectionId: 'conn-1',
			timeZone: 'America/New_York'
		});
		expect(submit).toHaveBeenNthCalledWith(1, `${BASE}/collect`, {
			connection_id: 'conn-1',
			time_zone: 'America/New_York'
		});
		expect(collected.deferredConnectionIds).toEqual(['conn-2']);

		const saved = await api.setSubscriptionUsageCredential(ORG, {
			connectionId: 'conn-1',
			secret: 'sk-secret',
			accountHint: 'ops@lumen.dev',
			expiresAt: '2026-12-31T23:59:59.000Z',
			expectedOrigin: 'https://api.anthropic.com'
		});
		expect(submit).toHaveBeenNthCalledWith(2, `${BASE}/credentials`, {
			connection_id: 'conn-1',
			secret: 'sk-secret',
			account_hint: 'ops@lumen.dev',
			expires_at: '2026-12-31T23:59:59.000Z',
			expected_origin: 'https://api.anthropic.com'
		});
		expect(saved).toMatchObject({ connectionId: 'conn-1', configured: true });

		const revoked = await api.revokeSubscriptionUsageCredential(ORG, 'conn-1');
		expect(submit).toHaveBeenNthCalledWith(3, `${BASE}/credentials/revoke`, {
			connection_id: 'conn-1'
		});
		expect(revoked.configured).toBe(false);

		await expect(api.collectSubscriptionUsage(ORG)).rejects.toMatchObject({
			source: 'api',
			code: 'not_authorized',
			message: 'Owner only'
		});
	});

	it('tracks snapshot and totals pagination streams independently without restarting an exhausted stream', async () => {
		const read = vi
			.fn()
			.mockResolvedValueOnce(
				ok({
					snapshots: [
						{
							connection_id: 'c1',
							connection_key: 'k1',
							connection_name: 'One',
							provider_type: 'claude',
							office_id: 'off-1',
							status: 'ok',
							auth_source: 'dedicated_usage_credential',
							has_usage_credential: true,
							has_enrolled_credential: false,
							stale: false,
							is_api_equivalent_estimate: false,
							models: []
						}
					],
					totals_by_model: [{ model: 'm1', total: {}, daily: {}, weekly: {} }],
					next_cursor: 's2',
					next_totals_cursor: 't2',
					partial_totals: false,
					excluded_snapshot_count: 0,
					is_api_equivalent_estimate: false
				})
			)
			.mockResolvedValueOnce(
				ok({
					snapshots: [
						{
							connection_id: 'c2',
							connection_key: 'k2',
							connection_name: 'Two',
							provider_type: 'claude',
							office_id: 'off-1',
							status: 'ok',
							auth_source: 'dedicated_usage_credential',
							has_usage_credential: true,
							has_enrolled_credential: false,
							stale: false,
							is_api_equivalent_estimate: false,
							models: []
						}
					],
					totals_by_model: [{ model: 'm2', total: {}, daily: {}, weekly: {} }],
					next_cursor: 's3',
					partial_totals: false,
					excluded_snapshot_count: 0,
					is_api_equivalent_estimate: false
				})
			)
			.mockResolvedValueOnce(
				ok({
					snapshots: [
						{
							connection_id: 'c3',
							connection_key: 'k3',
							connection_name: 'Three',
							provider_type: 'codex',
							office_id: 'off-1',
							status: 'ok',
							auth_source: 'dedicated_usage_credential',
							has_usage_credential: true,
							has_enrolled_credential: false,
							stale: false,
							is_api_equivalent_estimate: false,
							models: []
						}
					],
					totals_by_model: [{ model: 'm1', total: {}, daily: {}, weekly: {} }],
					partial_totals: false,
					excluded_snapshot_count: 0,
					is_api_equivalent_estimate: false
				})
			);

		const api = createHttpSubscriptionUsageApi({ http: { read, submit: vi.fn() } });
		const overview = await api.subscriptionUsage(ORG);
		expect(read).toHaveBeenCalledTimes(3);
		expect(read).toHaveBeenNthCalledWith(3, `${BASE}?limit=100&cursor=s3`);
		expect(overview.snapshots.map((s) => s.connectionId)).toEqual(['c1', 'c2', 'c3']);
		expect(overview.totalsByModel.map((m) => m.model)).toEqual(['m1', 'm2']);
		expect(overview.partialTotals).toBe(false);
	});
});
