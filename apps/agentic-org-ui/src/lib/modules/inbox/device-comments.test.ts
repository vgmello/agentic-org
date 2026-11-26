import { describe, expect, it, vi } from 'vitest';

let mockCallerScope: string | undefined = 'inst-1:acct-a';
vi.mock('$lib/platform/auth/app-session.svelte', () => ({
	appSession: {
		get callerScope() {
			return mockCallerScope;
		}
	}
}));

import {
	appendInboxDeviceComment,
	clearInboxDeviceComments,
	readInboxDeviceComments
} from './device-comments';

describe('inbox device-only comments', () => {
	it('preserves notes across undefined caller (reauth), clears on account switch and sign-out', () => {
		const store = new Map<string, string>();
		const mockStorage = {
			get length() {
				return store.size;
			},
			key: (i: number) => [...store.keys()][i] ?? null,
			getItem: (k: string) => store.get(k) ?? null,
			setItem: (k: string, v: string) => void store.set(k, v),
			removeItem: (k: string) => void store.delete(k)
		};
		(globalThis as unknown as { window: { localStorage: typeof mockStorage } }).window = {
			localStorage: mockStorage
		};

		mockCallerScope = 'inst-1:acct-a';
		mockStorage.setItem('unrelated.key', 'keep');
		const saved = appendInboxDeviceComment('org-1', 'item-1', 'Check rollout');
		expect(saved).toHaveLength(1);
		expect(readInboxDeviceComments('org-1', 'item-1')).toHaveLength(1);

		// Transient reauth / unreachable leaves saved notes intact
		mockCallerScope = undefined;
		expect(readInboxDeviceComments('org-1', 'item-1')).toHaveLength(1);

		// Switching to another signed-in account wipes prior caller's notes
		mockCallerScope = 'inst-1:acct-b';
		expect(readInboxDeviceComments('org-1', 'item-1')).toEqual([]);
		expect(mockStorage.getItem('unrelated.key')).toBe('keep');

		// Explicit sign-out cleanup removes new caller's notes too
		appendInboxDeviceComment('org-1', 'item-2', 'Acct B note');
		clearInboxDeviceComments();
		expect(readInboxDeviceComments('org-1', 'item-2')).toEqual([]);
	});
});
