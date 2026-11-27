import { describe, expect, it, vi } from 'vitest';

const timeZoneMock = vi.fn<(orgId: string) => Promise<{ saved: string | null }>>();
vi.mock('./api', () => ({
	preferencesApi: {
		timeZone: (orgId: string) => timeZoneMock(orgId),
		saveTimeZone: vi.fn(async () => undefined)
	}
}));

import { accountZone } from './account-zone.svelte';

describe('accountZone controller', () => {
	it('ignores stale org responses and clears cached state on reset', async () => {
		accountZone.reset();
		let resolveOrgA!: (v: { saved: string | null }) => void;
		timeZoneMock.mockImplementation((orgId) => {
			if (orgId === 'org-a') {
				return new Promise((resolve) => {
					resolveOrgA = resolve;
				});
			}
			return Promise.resolve({ saved: 'America/Sao_Paulo' });
		});

		const first = accountZone.ensure('org-a');
		const second = accountZone.ensure('org-b');
		await second;
		expect(accountZone.saved).toBe('America/Sao_Paulo');
		expect(accountZone.loaded).toBe(true);

		resolveOrgA({ saved: 'Europe/Lisbon' });
		await first;
		expect(accountZone.saved).toBe('America/Sao_Paulo');

		accountZone.reset();
		expect(accountZone.saved).toBeUndefined();
		expect(accountZone.loaded).toBe(false);
	});
});
