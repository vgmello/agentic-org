import { describe, expect, it } from 'vitest';
import { scoped } from './scoped';
import { costsApi } from './api';

// The web API does not serve costs yet: the production adapter refuses every read and write
// as unavailable — never as an empty answer, which would read as "no spend".
describe('the costs API in production', () => {
	it('refuses the project investment read as unavailable, not as zero', async () => {
		await expect(
			costsApi.projectInvestment('org', { timeZone: 'Europe/Lisbon' })
		).rejects.toMatchObject({ source: 'api', code: 'unavailable' });
	});

	it('keeps the unavailable refusal distinct from a viewer who may not see amounts', async () => {
		await expect(
			scoped(() => costsApi.projectInvestment('org', { timeZone: 'UTC' }))
		).rejects.toMatchObject({ code: 'unavailable' });
		const refused = await scoped(() =>
			Promise.reject(
				Object.assign(new Error('no'), { source: 'api', code: 'not_authorized', message: 'no' })
			)
		);
		expect(refused).toEqual({ allowed: false, reason: 'no' });
	});

	it('refuses every Step 7 insights, export, member cost, and agent budget call as unavailable', async () => {
		const period = { kind: 'this_month' as const, timeZone: 'UTC' };
		for (const call of [
			costsApi.projectInsights('org', 'atlas', period),
			costsApi.projectRecords('org', 'atlas', period, { all: true }),
			costsApi.organizationInsights('org', period),
			costsApi.exportRecords('org', { kind: 'organization' }, period),
			costsApi.memberCost('org', 'sam', { timeZone: 'UTC' }),
			costsApi.memberRecords('org', 'sam', { timeZone: 'UTC', filter: { kind: 'all' } }),
			costsApi.setAgentBudget('org', 'sam', {
				periodKind: 'daily',
				unit: 'usd',
				limit: 10_000_000,
				operationId: 'op-1'
			}),
			costsApi.agentBudgetAction('org', 'sam', {
				periodKind: 'daily',
				action: 'clear',
				operationId: 'op-2'
			}),
			costsApi.agentBudgetOperation('org', 'op-1')
		]) {
			await expect(call).rejects.toMatchObject({ source: 'api', code: 'unavailable' });
		}
	});
});
