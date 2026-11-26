import { describe, expect, it, vi } from 'vitest';
import type { OrgSnapshot } from '$lib/proto';
import { createHttpCostsReadApi } from './http-api';
import { createHttpCostsFullApi } from './http-full-api';

const snapshot: OrgSnapshot = {
	position: { epoch: 1, seq: 2 },
	teams: [{ id: 't1', name: 'Platform', version: 1 }],
	members: [
		{ id: 'm-owner', kind: 'person', name: 'Vinicius', owner: true, version: 1 },
		{
			id: 'm-ada',
			kind: 'agent',
			name: 'Ada',
			version: 1,
			agent: {
				role: 'Eng',
				title: 'Eng',
				runtime: 'claude_code',
				messaging_name: 'ada',
				status: 'active'
			}
		}
	],
	projects: [{ id: 'p1', key: 'CORE', name: 'Core Platform', version: 1 }],
	project_teams: []
};

describe('createHttpCostsFullApi', () => {
	it('loads prices, organization breakdowns, member cost, and submits budget mutations', async () => {
		const submit = vi.fn(async (path: string) => {
			if (path.endsWith('/costs/query')) {
				return {
					status: 200,
					body: {
						result: {
							time_zone: 'UTC',
							ends_at: '2026-11-26T04:00:00Z',
							scoped_to_projects: false,
							summary: {
								record_count: 1,
								known_cost_micro_usd: 310_000,
								known_tokens: 15_000,
								cache_read_tokens: 4_000,
								incomplete_records: 0,
								unpriced_records: 0,
								includes_api_equivalent: true
							},
							records: [
								{
									id: 'rec-1',
									occurred_at: '2026-11-26T03:00:00Z',
									actor_kind: 'employee',
									employee_id: 'm-ada',
									runtime: 'claude_code',
									provider: 'anthropic',
									model: 'claude-sonnet',
									input_tokens: 10_000,
									output_tokens: 1_000,
									cache_read_tokens: 4_000,
									cache_write_tokens: 0,
									reasoning_tokens: 0,
									token_status: 'complete',
									cost_micro_usd: 310_000,
									cost_basis: 'runtime_estimate',
									cost_status: 'complete',
									project_id: 'p1',
									task_id: 'T-1',
									work_type: 'build',
									attribution_basis: 'current_task'
								}
							]
						}
					}
				};
			}
			if (path.endsWith('/budgets/set')) {
				return {
					status: 200,
					body: {
						result: {
							budget: {
								id: 'b-1',
								unit: 'micro_usd',
								limit_amount: 5_000_000,
								used_amount: 310_000,
								period_key: '2026-11-26',
								starts_at: '2026-11-26T00:00:00Z',
								ends_at: '2026-11-27T00:00:00Z'
							}
						}
					}
				};
			}
			return { status: 200, body: { result: {} } };
		});

		const read = vi.fn(async (path: string) => {
			if (path.endsWith('/costs/prices')) {
				return {
					status: 200,
					body: {
						result: {
							prices: [
								{
									id: 'pr-1',
									provider: 'anthropic',
									model: 'claude-sonnet',
									effective_from: '2026-11-01T00:00:00Z',
									input_micro_usd_per_million: 3_000_000,
									output_micro_usd_per_million: 15_000_000,
									created_at: '2026-11-01T00:00:00Z'
								}
							]
						}
					}
				};
			}
			if (path.includes('/budgets')) {
				return { status: 200, body: { result: { budgets: [] } } };
			}
			return { status: 404, body: {} };
		});

		const http = { read, submit };
		const orgApi = { view: async () => ({ snapshot, position: snapshot.position, sample: false }) };
		const costsReadApi = createHttpCostsReadApi({ http, orgApi });
		const api = createHttpCostsFullApi({
			http,
			orgApi,
			costsReadApi,
			session: () => ({ accountAdmin: true }),
			now: () => '2026-11-26T04:00:00Z'
		});

		const prices = await api.prices('org-1');
		expect(prices.prices).toHaveLength(1);
		expect(prices.viewer.canManage).toBe(true);

		const orgCost = await api.organizationCost('org-1', { kind: 'this_month', timeZone: 'UTC' });
		expect(orgCost.totals.knownCostMicroUsd).toBe(310_000);
		expect(orgCost.byProject).toHaveLength(1);
		expect(orgCost.byConsumer[0]?.label).toBe('Ada');

		const memberCost = await api.memberCost('org-1', 'm-ada', { timeZone: 'UTC' });
		expect(memberCost.periods.thisMonth.knownCostMicroUsd).toBe(310_000);
		expect(memberCost.topTasks[0]?.taskRef).toBe('T-1');

		const budget = await api.setAgentBudget('org-1', 'm-ada', {
			periodKind: 'daily',
			unit: 'usd',
			limit: 5_000_000,
			operationId: '0193a5c0-0000-7000-8000-000000000099'
		});
		expect(budget.limit).toBe(5_000_000);
		expect(budget.unit).toBe('usd');
	});
});
