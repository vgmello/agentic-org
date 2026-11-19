import { describe, expect, it } from 'vitest';
import { scoped } from './scoped';
import { costsApi } from './api';

// The web API does not serve costs yet: the production adapter refuses every read, including the project-level
// investment read, as unavailable — never as an empty answer, which would read as "no spend".
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
});
