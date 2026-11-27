import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
	appendInboxDeviceComment,
	clearInboxDeviceComments,
	readInboxCommentDrafts,
	readInboxDeviceComments,
	writeInboxCommentDrafts
} from './device-comments';

function createMemoryStorage(): Storage {
	const map = new Map<string, string>();
	return {
		get length() {
			return map.size;
		},
		clear() {
			map.clear();
		},
		getItem(key: string) {
			return map.has(key) ? map.get(key)! : null;
		},
		key(index: number) {
			return [...map.keys()][index] ?? null;
		},
		removeItem(key: string) {
			map.delete(key);
		},
		setItem(key: string, value: string) {
			map.set(key, String(value));
		}
	};
}

describe('Inbox device-local comments and drafts cleanup', () => {
	let local: Storage;
	let session: Storage;

	beforeEach(() => {
		local = createMemoryStorage();
		session = createMemoryStorage();
		vi.stubGlobal('window', { localStorage: local, sessionStorage: session });
		clearInboxDeviceComments();
	});

	afterEach(() => {
		clearInboxDeviceComments();
		vi.unstubAllGlobals();
	});

	it('clears persisted comments, sessionStorage drafts, and in-memory drafts on sign-out', () => {
		appendInboxDeviceComment('org-1', 'item-1', 'Follow up tomorrow');
		writeInboxCommentDrafts({ 'org-1::item-1': 'Unsent draft note' });

		expect(readInboxDeviceComments('org-1', 'item-1')).toHaveLength(1);
		expect(readInboxCommentDrafts()).toEqual({ 'org-1::item-1': 'Unsent draft note' });
		expect(session.getItem('agentic-org.inbox.commentDrafts')).toContain('Unsent draft note');

		clearInboxDeviceComments();

		expect(readInboxDeviceComments('org-1', 'item-1')).toEqual([]);
		expect(readInboxCommentDrafts()).toEqual({});
		expect(session.getItem('agentic-org.inbox.commentDrafts')).toBeNull();
	});
});
