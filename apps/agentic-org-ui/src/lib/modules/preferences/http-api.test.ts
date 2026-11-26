import { describe, expect, it, vi } from 'vitest';
import { createHttpPreferencesApi } from './http-api';

describe('createHttpPreferencesApi', () => {
	it('reads saved time zone from session and persists updates via POST /api/v1/account/preferences', async () => {
		const submit = vi.fn(async () => ({ status: 200, body: { result: { saved: true } } }));
		const read = vi.fn(async () => ({
			status: 200,
			body: {
				result: {
					instance_id: 'inst-1',
					account: { id: 'acc-1', login: 'vini', time_zone: 'America/Sao_Paulo' },
					admin: true,
					organizations: [],
					server_time_ms: 1000,
					idle_expires_at: '2026-12-01T00:00:00Z'
				}
			}
		}));
		const onSaved = vi.fn();

		const api = createHttpPreferencesApi({
			http: { read, submit },
			onSaved
		});

		expect(await api.timeZone('org-1')).toEqual({ saved: 'America/Sao_Paulo' });
		await api.saveTimeZone('org-1', 'Europe/Lisbon');
		expect(submit).toHaveBeenCalledWith('/api/v1/account/preferences', {
			time_zone: 'Europe/Lisbon'
		});
		expect(onSaved).toHaveBeenCalledOnce();
	});
});
