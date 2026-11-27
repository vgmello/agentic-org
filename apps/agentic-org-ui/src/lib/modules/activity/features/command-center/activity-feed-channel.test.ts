import { describe, expect, it } from 'vitest';
import type { ActivityQuery, ActivityRow } from '$lib/modules/activity/types';
import { FeedChannel } from './activity-feed-channel.svelte';

function makeRow(id: string, at: string): ActivityRow {
	return {
		id,
		organizationId: 'org-1',
		at,
		kind: 'task.created',
		category: 'work',
		actorId: 'mem-1',
		actorName: 'Noor',
		actorKind: 'agent',
		summary: `Event ${id}`
	} as unknown as ActivityRow;
}

const emptyQuery = { kind: 'all' } as unknown as ActivityQuery;

describe('FeedChannel state machine', () => {
	it('buffers live rows while inFlight and merges them on applySuccess', () => {
		const channel = new FeedChannel();
		const seq = channel.beginFetch(true, false);
		expect(seq).toBe(1);
		expect(channel.phase).toBe('loading');
		expect(channel.inFlight).toBe(true);

		const liveRow = makeRow('evt-2', '2026-11-27T08:02:00Z');
		expect(channel.applyLiveRow(liveRow, emptyQuery)).toBe(false);
		expect(channel.buffered).toHaveLength(1);

		channel.catchupFailedOrg = 'org-1';
		const cleared = channel.applySuccess('org-1', emptyQuery, {
			rows: [makeRow('evt-1', '2026-11-27T08:01:00Z')],
			more: 0
		});
		expect(cleared).toBe(true);
		expect(channel.phase).toBe('ready');
		expect(channel.inFlight).toBe(false);
		expect(channel.rows?.map((r) => r.id)).toContain('evt-1');
		expect(channel.rows?.map((r) => r.id)).toContain('evt-2');
	});

	it('records hard failures and resets cleanly', () => {
		const channel = new FeedChannel();
		channel.beginFetch(false, false);
		channel.applyHardFailure('org-1', false, new Error('Offline'), 'Fallback');
		expect(channel.phase).toBe('error');
		expect(channel.error).toBe('Offline');
		expect(channel.catchupFailedOrg).toBe('org-1');

		channel.reset();
		expect(channel.inFlight).toBe(false);
		expect(channel.buffered).toEqual([]);
	});
});
