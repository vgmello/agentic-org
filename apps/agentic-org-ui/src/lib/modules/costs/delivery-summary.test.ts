import { describe, expect, it } from 'vitest';
import type { InboxItem } from '$lib/modules/inbox/types';
import type { OrgSnapshot, WorkItemSummary } from '$lib/proto';
import { deriveProjectDeliverySummary } from './delivery-summary';

function makeSnapshot(overrides: Partial<OrgSnapshot> = {}): OrgSnapshot {
	return {
		position: { epoch: 1, seq: 1 },
		members: [
			{
				id: 'owner-uuid-1',
				kind: 'person',
				name: 'Ana Owner',
				owner: true,
				version: 1
			}
		],
		teams: [],
		projects: [
			{ id: 'proj-a', name: 'Project A', key: 'PA', archived: false },
			{ id: 'proj-b', name: 'Project B', key: 'PB', archived: false }
		],
		areas: [],
		...overrides
	};
}

function makeWorkItem(overrides: Partial<WorkItemSummary> = {}): WorkItemSummary {
	return {
		id: 'task-1',
		short_id: 'T-14',
		kind: 'task',
		title: 'Implement auth',
		status: 'todo',
		project_id: 'proj-a',
		created_by: 'owner-uuid-1',
		created_at: '2026-01-01T00:00:00Z',
		updated_at: '2026-01-01T00:00:00Z',
		severity_confirmed: true,
		reopen_count: 0,
		fixes: [],
		fixed_by: [],
		checklist: { total: 0, done: 0, required_left: 0 },
		done_anyway: false,
		owner_office_id: 'hq',
		generation: 1,
		version: 1,
		deleted: false,
		...overrides
	};
}

function makeInbox(overrides: Partial<InboxItem> = {}): InboxItem {
	return {
		id: 'inb-1',
		kind: 'approval',
		state: 'open',
		title: 'Approve T-14 rollout',
		why: 'Needs approval',
		source: 'Chat approval',
		who: { id: 'agent-1', name: 'Ava', kind: 'agent' },
		actions: ['approve', 'deny'],
		created_at: '2026-01-02T00:00:00Z',
		...overrides
	};
}

describe('deriveProjectDeliverySummary', () => {
	it('isolates Inbox review gates to the target project so sibling projects never leak into review', () => {
		const snapshot = makeSnapshot();
		const workItems = [
			makeWorkItem({ id: 'a-14', short_id: 'T-14', project_id: 'proj-a', status: 'todo' }),
			makeWorkItem({ id: 'b-14', short_id: 'T-14', project_id: 'proj-b', status: 'in_progress' })
		];
		const inboxItems = [
			makeInbox({
				id: 'inb-other',
				project: { id: 'proj-b', name: 'Project B' },
				open: { label: 'Open T-14', path: '/work/T-14' }
			})
		];

		const summaryA = deriveProjectDeliverySummary({
			snapshot,
			projectId: 'proj-a',
			workItems,
			inboxItems
		});
		expect(summaryA.stageCounts).toEqual({ queue: 1, agents: 0, ci: 0, review: 0, merged: 0 });

		const summaryB = deriveProjectDeliverySummary({
			snapshot,
			projectId: 'proj-b',
			workItems,
			inboxItems
		});
		expect(summaryB.stageCounts).toEqual({ queue: 0, agents: 0, ci: 0, review: 1, merged: 0 });
	});

	it('excludes problem inbox notices and terminal declined/cancelled work items', () => {
		const snapshot = makeSnapshot();
		const workItems = [
			makeWorkItem({ id: 't-1', short_id: 'T-1', status: 'todo' }),
			makeWorkItem({ id: 't-cancel', short_id: 'T-2', status: 'todo', resolution: 'cancelled' }),
			makeWorkItem({ id: 'req-dec', short_id: 'R-1', kind: 'request', status: 'declined' }),
			makeWorkItem({
				id: 't-ci',
				short_id: 'T-3',
				status: 'in_progress',
				checklist: { total: 2, done: 2, required_left: 0 }
			}),
			makeWorkItem({ id: 't-done', short_id: 'T-4', status: 'done' })
		];
		const inboxItems = [
			makeInbox({
				id: 'inb-problem',
				kind: 'problem',
				project: { id: 'proj-a', name: 'Project A' },
				open: { label: 'Open T-1', path: '/work/T-1' }
			})
		];

		const summary = deriveProjectDeliverySummary({
			snapshot,
			projectId: 'proj-a',
			workItems,
			inboxItems
		});
		expect(summary.totalItems).toBe(3);
		expect(summary.stageCounts).toEqual({ queue: 1, agents: 0, ci: 1, review: 0, merged: 1 });
	});

	it('falls back to snapshot owner ID when ownerId is omitted for blocked tasks', () => {
		const snapshot = makeSnapshot();
		const workItems = [
			makeWorkItem({
				id: 't-blocked',
				short_id: 'T-5',
				status: 'blocked',
				assignee_id: 'owner-uuid-1'
			})
		];

		const summary = deriveProjectDeliverySummary({
			snapshot,
			projectId: 'proj-a',
			workItems,
			inboxItems: []
		});
		expect(summary.stageCounts.review).toBe(1);
		expect(summary.stageCounts.queue).toBe(0);
	});

	it('groups by project areas when multiple active areas exist, otherwise by work kind', () => {
		const multiAreaSnapshot = makeSnapshot({
			areas: [
				{ id: 'area-api', project_id: 'proj-a', name: 'API', version: 1 },
				{ id: 'area-web', project_id: 'proj-a', name: 'Web', version: 1 }
			]
		});
		const items = [
			makeWorkItem({ id: 't-1', short_id: 'T-1', area_id: 'area-api', status: 'done' }),
			makeWorkItem({ id: 'b-1', short_id: 'B-1', kind: 'bug', area_id: 'area-web', status: 'new' })
		];

		const areaSummary = deriveProjectDeliverySummary({
			snapshot: multiAreaSnapshot,
			projectId: 'proj-a',
			workItems: items,
			inboxItems: []
		});
		expect(areaSummary.tracks.map((t) => [t.name, t.doneCount, t.totalCount, t.pct])).toEqual([
			['API', 1, 1, 100],
			['Web', 0, 1, 0]
		]);

		const kindSummary = deriveProjectDeliverySummary({
			snapshot: makeSnapshot(),
			projectId: 'proj-a',
			workItems: items,
			inboxItems: []
		});
		expect(kindSummary.tracks.map((t) => [t.name, t.doneCount, t.totalCount])).toEqual([
			['Tasks', 1, 1],
			['Bugs', 0, 1]
		]);
	});
});
