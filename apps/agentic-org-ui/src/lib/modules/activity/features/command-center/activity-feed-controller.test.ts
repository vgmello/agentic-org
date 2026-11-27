import { describe, expect, it } from 'vitest';
import type { ActivityRow } from '$lib/modules/activity/types';
import { ActivityFeedController } from './activity-feed-controller.svelte';

function deferred<T>() {
	let resolve!: (value: T) => void;
	let reject!: (reason?: unknown) => void;
	const promise = new Promise<T>((res, rej) => {
		resolve = res;
		reject = rej;
	});
	return { promise, resolve, reject };
}

function sampleRow(id: string): ActivityRow {
	return {
		id,
		kind: 'work',
		action: 'task-done',
		actor: { id: 'm1', name: 'Ada', kind: 'agent' },
		text: `Finished ${id}`,
		projectIds: [],
		teamIds: [],
		memberIds: [],
		at: '2026-11-26T10:00:00Z'
	};
}

describe('ActivityFeedController race & disposal guards', () => {
	it('ignores stale in-flight feed responses after an organization switch', async () => {
		let currentOrg = 'org-a';
		const reqA = deferred<{ rows: ActivityRow[]; more: number }>();
		const reqB = deferred<{ rows: ActivityRow[]; more: number }>();

		const ctrl = new ActivityFeedController({
			activityApi: {
				feed: (id) => (id === 'org-a' ? reqA.promise : reqB.promise)
			},
			inboxApi: { list: async () => [] },
			getOrganizationId: () => currentOrg,
			getActiveAllQuery: () => ({ kind: 'all' }),
			getActiveDecisionQuery: () => ({ kind: 'decision' }),
			isDecisionTabActive: () => false,
			refreshShell: async () => undefined
		});

		ctrl.fetchMainFeed('org-a', { kind: 'all' }, true);
		currentOrg = 'org-b';
		ctrl.resetMainScope();
		ctrl.fetchMainFeed('org-b', { kind: 'all' }, true);

		reqB.resolve({ rows: [sampleRow('b-1')], more: 0 });
		await Promise.resolve();
		await Promise.resolve();
		expect(ctrl.activityRows?.map((r) => r.id)).toEqual(['b-1']);

		// Late stale response from org-a must be dropped.
		reqA.resolve({ rows: [sampleRow('a-stale')], more: 5 });
		await Promise.resolve();
		await Promise.resolve();
		expect(ctrl.activityRows?.map((r) => r.id)).toEqual(['b-1']);
		expect(ctrl.feedMore).toBe(0);
	});

	it('ignores in-flight responses after dispose()', async () => {
		const req = deferred<{ rows: ActivityRow[]; more: number }>();
		const ctrl = new ActivityFeedController({
			activityApi: { feed: () => req.promise },
			inboxApi: { list: async () => [] },
			getOrganizationId: () => 'org-a',
			getActiveAllQuery: () => ({ kind: 'all' }),
			getActiveDecisionQuery: () => ({ kind: 'decision' }),
			isDecisionTabActive: () => false,
			refreshShell: async () => undefined
		});

		ctrl.fetchMainFeed('org-a', { kind: 'all' }, true);
		ctrl.dispose();

		req.resolve({ rows: [sampleRow('after-dispose')], more: 0 });
		await Promise.resolve();
		await Promise.resolve();

		expect(ctrl.activityRows).toBeUndefined();
		expect(ctrl.feedPhase).toBe('loading');
	});
});
