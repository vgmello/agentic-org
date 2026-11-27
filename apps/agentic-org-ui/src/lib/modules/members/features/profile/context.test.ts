import { describe, expect, it, vi } from 'vitest';

const store = new Map<unknown, unknown>();
vi.mock('svelte', () => ({
	createContext: () => {
		const key = Symbol('ctx');
		return [
			() => store.get(key),
			(val: unknown) => {
				store.set(key, val);
				return val;
			}
		];
	}
}));

import { profileContext, setProfileContext, type ProfileContext } from './context.svelte';

describe('profile context', () => {
	it('stores and retrieves the active ProfileContext', () => {
		const ctx = { organizationId: 'org-1', changes: 2 } as unknown as ProfileContext;
		setProfileContext(ctx);
		expect(profileContext()).toBe(ctx);
	});
});
