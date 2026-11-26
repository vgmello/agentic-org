import { describe, expect, it, vi } from 'vitest';
import { createHttpUpdatesApi } from './http-api';

describe('createHttpUpdatesApi', () => {
	it('returns instance version and office versions from live gateway', async () => {
		const read = vi.fn(async (path: string) => {
			if (path === '/api/v1/instance') {
				return {
					status: 200,
					body: {
						result: {
							instance_id: 'inst-1',
							name: 'default',
							server_version: '0.2.0',
							web_status: 'ok',
							listen_scheme: 'http_loopback',
							server_time_ms: 1000
						}
					}
				};
			}
			if (path === '/api/v1/organizations/org-1/offices') {
				return {
					status: 200,
					body: {
						result: {
							offices: [
								{
									id: 'off-hq',
									organization_id: 'org-1',
									name: 'HQ',
									role: 'hq',
									host: 'hq.local',
									status: 'active'
								},
								{
									id: 'off-eu',
									organization_id: 'org-1',
									name: 'Worker EU',
									role: 'satellite',
									host: 'eu.tailnet',
									status: 'update_needed'
								}
							]
						}
					}
				};
			}
			return { status: 404, body: {} };
		});

		const api = createHttpUpdatesApi({ http: { read, submit: vi.fn() } });
		const overview = await api.overview('org-1');

		expect(overview.instance).toEqual({ name: 'default', version: '0.2.0' });
		expect(overview.check).toEqual({ state: 'unknown' });
		expect(overview.offices).toHaveLength(2);
		expect(overview.offices[0]).toMatchObject({
			id: 'off-hq',
			role: 'hq',
			version: '0.2.0',
			holdsBack: false
		});
		expect(overview.offices[1]).toMatchObject({
			id: 'off-eu',
			role: 'satellite',
			version: null,
			holdsBack: true
		});
	});
});
