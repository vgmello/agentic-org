import { describe, expect, it, vi } from 'vitest';
import { createHttpOfficesOverviewApi } from './overview-http-api';

describe('createHttpOfficesOverviewApi', () => {
	it('maps satellite offices from GET /offices while excluding HQ from satellites', async () => {
		const read = vi.fn().mockResolvedValue({
			status: 200,
			body: {
				result: {
					offices: [
						{
							id: 'hq-1',
							organization_id: 'org-1',
							name: 'HQ',
							role: 'hq',
							host: 'This Mac',
							status: 'active'
						},
						{
							id: 'sat-1',
							organization_id: 'org-1',
							name: 'apps-worker-eu',
							role: 'satellite',
							host: 'Remote (apps-worker-eu)',
							status: 'active'
						},
						{
							id: 'sat-2',
							organization_id: 'org-1',
							name: 'apps-main-eu',
							role: 'satellite',
							host: 'Remote (apps-main-eu)',
							status: 'offline'
						}
					]
				}
			}
		});

		const api = createHttpOfficesOverviewApi({ http: { read } });
		const overview = await api.overview('org-1');

		expect(read).toHaveBeenCalledWith('/api/v1/organizations/org-1/offices');
		expect(overview.satellites.map((o) => o.name)).toEqual(['apps-worker-eu', 'apps-main-eu']);
		expect(overview.details['sat-1']?.where).toBe('Remote (apps-worker-eu)');
		expect(overview.viewer.isAdministrator).toBe(false);
	});

	it('fails closed on gateway errors', async () => {
		const read = vi.fn().mockResolvedValue({
			status: 403,
			body: { error: { code: 'not_authorized', message: 'Forbidden.' } }
		});
		const api = createHttpOfficesOverviewApi({ http: { read } });
		await expect(api.overview('org-1')).rejects.toMatchObject({
			source: 'api',
			code: 'not_authorized',
			message: 'Forbidden.'
		});
	});
});
