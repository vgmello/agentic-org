import { describe, expect, it } from 'vitest';
import {
	appendInboxDeviceComment,
	clearInboxDeviceComments,
	readInboxDeviceComments
} from './device-comments';

describe('inbox device-only comments', () => {
	it('stores per-item comments and clears all inbox comments on sign-out without touching other keys', () => {
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

		mockStorage.setItem('unrelated.key', 'keep');
		const saved = appendInboxDeviceComment('org-1', 'item-1', 'Check rollout');
		expect(saved).toHaveLength(1);
		expect(readInboxDeviceComments('org-1', 'item-1')).toHaveLength(1);

		clearInboxDeviceComments();
		expect(readInboxDeviceComments('org-1', 'item-1')).toEqual([]);
		expect(mockStorage.getItem('unrelated.key')).toBe('keep');
	});
});
