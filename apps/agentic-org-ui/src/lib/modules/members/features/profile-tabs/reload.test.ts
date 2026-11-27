import { describe, expect, it, vi } from 'vitest';

const mockCtx = {
	organizationId: 'org-1',
	profile: { id: 'mem-1' },
	changes: 0
};

vi.mock('../profile/context.svelte', () => ({
	profileContext: () => mockCtx
}));

import { createTabChangeSync, tabResource } from './reload.svelte';

describe('profile-tabs tabResource', () => {
	it('loads initially and reloads whenever profile changes counter increments', async () => {
		const load = vi.fn(async (orgId: string, memberId: string) => `${orgId}:${memberId}`);
		mockCtx.changes = 0;

		const resource = tabResource(load);
		await new Promise((r) => setTimeout(r, 0));
		expect(load).toHaveBeenCalledTimes(1);
		expect(resource.data).toBe('org-1:mem-1');

		const sync = createTabChangeSync(resource, 0);
		expect(sync(0)).toBe(false);
		expect(sync(1)).toBe(true);
		await new Promise((r) => setTimeout(r, 0));
		expect(load).toHaveBeenCalledTimes(2);
	});
});
