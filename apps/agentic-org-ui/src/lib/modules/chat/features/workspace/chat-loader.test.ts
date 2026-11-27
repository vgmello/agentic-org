import { describe, expect, it, vi } from 'vitest';
import type { ChatSnapshot } from '$lib/modules/messaging/types';
import type { ChatApi } from '../../api';
import { createChatSnapshotLoader } from './chat-loader';

function fakeSnap(rev: string): ChatSnapshot {
	return {
		organization_id: 'org-1',
		conversations: [],
		messages: [],
		version: rev
	} as unknown as ChatSnapshot;
}

describe('createChatSnapshotLoader', () => {
	it('ignores out-of-order stale snapshots and cancels on dispose', async () => {
		let resolveFirst!: (s: ChatSnapshot) => void;
		let resolveSecond!: (s: ChatSnapshot) => void;
		const snapshot = vi
			.fn<ChatApi['snapshot']>()
			.mockImplementationOnce(() => new Promise((r) => (resolveFirst = r)))
			.mockImplementationOnce(() => new Promise((r) => (resolveSecond = r)));

		const received: ChatSnapshot[] = [];
		const loader = createChatSnapshotLoader({
			chatApi: { snapshot } as unknown as ChatApi,
			getOrganizationId: () => 'org-1',
			hasSnapshot: () => received.length > 0,
			onSnapshot: (s) => received.push(s),
			onError: vi.fn(),
			debounceMs: 10
		});

		const p1 = loader.load();
		const p2 = loader.load();
		resolveSecond(fakeSnap('v2'));
		await p2;
		resolveFirst(fakeSnap('v1'));
		await p1;

		expect(received).toHaveLength(1);
		expect((received[0] as unknown as { version: string }).version).toBe('v2');

		loader.dispose();
		loader.scheduleLiveReload();
		await new Promise((r) => setTimeout(r, 25));
		expect(snapshot).toHaveBeenCalledTimes(2);
	});
});
