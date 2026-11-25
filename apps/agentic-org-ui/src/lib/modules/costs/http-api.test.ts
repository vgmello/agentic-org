import { describe, expect, it, vi } from 'vitest';
import { createHttpCostsReadApi } from './http-api';

describe('createHttpCostsReadApi', () => {
	it('maps POST /costs/query into OrganizationCost without fabricating nulls as zeros', async () => {
		const submit = vi.fn().mockResolvedValue({
			status: 200,
			body: {
				result: {
					time_zone: 'UTC',
					starts_at: '2026-11-01T00:00:00Z',
					ends_at: '2026-11-25T16:00:00Z',
					scoped_to_projects: false,
					summary: {
						known_cost_micro_usd: 307891,
						known_tokens: 334959,
						cache_read_tokens: 266046,
						record_count: 6,
						unpriced_records: 0,
						incomplete_records: 0,
						includes_api_equivalent: false,
						coverage_as_of: '2026-11-25T14:23:45.160Z'
					},
					breakdown: []
				}
			}
		});

		const api = createHttpCostsReadApi({
			http: { read: vi.fn(), submit },
			orgApi: { view: vi.fn() }
		});
		const cost = await api.organizationCost('org-1', { kind: 'this_month', timeZone: 'UTC' });

		expect(submit).toHaveBeenCalledWith('/api/v1/organizations/org-1/costs/query', {
			period: 'this_month',
			time_zone: 'UTC'
		});
		expect(cost.totals).toMatchObject({
			records: 6,
			knownCostMicroUsd: 307891,
			knownTokens: 334959,
			cacheReadTokens: 266046
		});
		expect(cost.coverage.checkpointAt).toBe('2026-11-25T14:23:45.160Z');
	});

	it('maps GET /budgets?scope_kind=agent into AgentBudgets with resolved member names', async () => {
		const read = vi.fn().mockResolvedValue({
			status: 200,
			body: {
				result: {
					budgets: [
						{
							id: 'b-1',
							scope_kind: 'agent',
							agent_id: 'agent-1',
							period_kind: 'month',
							state: 'active',
							time_zone: 'UTC',
							time_zone_source: 'owner_person',
							state_summary: 'warning80',
							amounts_visible: true,
							unit: 'micro_usd',
							limit_amount: 100_000_000,
							effective_limit: 100_000_000,
							used_amount: 85_000_000,
							period_key: '2026-11',
							starts_at: '2026-11-01T00:00:00Z',
							ends_at: '2026-12-01T00:00:00Z',
							usage_status: 'complete',
							extension_used: false
						}
					]
				}
			}
		});
		const view = vi.fn().mockResolvedValue({
			snapshot: {
				position: { epoch: 1, seq: 1 },
				teams: [],
				members: [{ id: 'agent-1', kind: 'agent', name: 'Backend Lead', version: 1 }]
			},
			sample: false
		});

		const api = createHttpCostsReadApi({
			http: { read, submit: vi.fn() },
			orgApi: { view }
		});
		const budgets = await api.agentBudgets('org-1');

		expect(read).toHaveBeenCalledWith('/api/v1/organizations/org-1/budgets?scope_kind=agent');
		expect(budgets.zone).toEqual({ name: 'UTC', source: 'owner_account' });
		expect(budgets.agents).toHaveLength(1);
		expect(budgets.agents[0]).toMatchObject({
			memberId: 'agent-1',
			name: 'Backend Lead',
			monthly: {
				unit: 'usd',
				limit: 100_000_000,
				spent: 85_000_000,
				warned: true
			}
		});
	});
});
