import { describe, expect, it } from 'vitest';
import type { WorkItemSummary } from '$lib/proto';
import {
	assigneeInitials,
	groupWorkByStatus,
	needsOwnerAttention,
	priorityMeter,
	projectDotTone,
	workStatusTone
} from './compact';

function item(
	partial: Partial<WorkItemSummary> & Pick<WorkItemSummary, 'id' | 'kind' | 'status'>
): WorkItemSummary {
	return {
		short_id: partial.id,
		title: partial.title ?? partial.id,
		created_by: 'owner',
		created_at: '2026-11-20T00:00:00Z',
		updated_at: '2026-11-20T00:00:00Z',
		severity_confirmed: false,
		reopen_count: 0,
		fixes: [],
		fixed_by: [],
		checklist: { total: 0, done: 0, required_left: 0 },
		done_anyway: false,
		owner_office_id: 'hq',
		generation: 1,
		version: 1,
		deleted: false,
		...partial
	};
}

describe('compact work helpers', () => {
	it('maps statuses to compact ring tones', () => {
		expect(workStatusTone('task', 'in_progress')).toBe('active');
		expect(workStatusTone('task', 'done')).toBe('done');
		expect(workStatusTone('task', 'blocked')).toBe('blocked');
		expect(workStatusTone('task', 'todo')).toBe('neutral');
		expect(workStatusTone('bug', 'new')).toBe('active');
		expect(workStatusTone('bug', 'triaged')).toBe('review');
		expect(workStatusTone('bug', 'fixed')).toBe('done');
		expect(workStatusTone('bug', 'closed')).toBe('neutral');
	});

	it('flags items waiting on the owner', () => {
		expect(
			needsOwnerAttention(
				item({ id: 't1', kind: 'task', status: 'blocked', assignee_id: 'me' }),
				'me'
			)
		).toBe(true);
		expect(
			needsOwnerAttention(
				item({ id: 't2', kind: 'task', status: 'blocked', assignee_id: 'other' }),
				'me'
			)
		).toBe(false);
		expect(needsOwnerAttention(item({ id: 'b1', kind: 'bug', status: 'new' }), 'me')).toBe(true);
		expect(needsOwnerAttention(item({ id: 'b2', kind: 'bug', status: 'fixed' }), 'me')).toBe(true);
		expect(needsOwnerAttention(item({ id: 'b3', kind: 'bug', status: 'triaged' }), 'me')).toBe(
			false
		);
		expect(needsOwnerAttention(item({ id: 'r1', kind: 'request', status: 'scoped' }), 'me')).toBe(
			true
		);
	});

	it('maps explicit non-Normal priorities to neutral meter levels and leaves Normal/unset quiet', () => {
		expect(priorityMeter('urgent')).toEqual({
			level: 'high',
			label: 'Urgent priority',
			tone: 'strong'
		});
		expect(priorityMeter('high')).toEqual({
			level: 'high',
			label: 'High priority',
			tone: 'neutral'
		});
		expect(priorityMeter('low')).toEqual({ level: 'low', label: 'Low priority', tone: 'neutral' });
		expect(priorityMeter('normal')).toBeUndefined();
		expect(priorityMeter('unknown')).toBeUndefined();
		expect(priorityMeter(undefined)).toBeUndefined();
	});

	it('groups tasks and bugs in priority status order and drops empty buckets', () => {
		const grouped = groupWorkByStatus(
			[
				item({ id: '1', kind: 'task', status: 'done' }),
				item({ id: '2', kind: 'task', status: 'todo' }),
				item({ id: '3', kind: 'task', status: 'blocked' }),
				item({ id: '4', kind: 'task', status: 'in_progress' })
			],
			'tasks'
		);
		expect(grouped.map((g) => g.status)).toEqual(['blocked', 'in_progress', 'todo', 'done']);
		expect(grouped.map((g) => g.label)).toEqual(['Blocked', 'In progress', 'To do', 'Done']);
	});

	it('derives deterministic project dot tones and compact initials', () => {
		expect(projectDotTone('atlas')).toBe(projectDotTone('atlas'));
		expect(projectDotTone(undefined)).toBe('accent');
		expect(assigneeInitials('Alex Kim')).toBe('AK');
		expect(assigneeInitials('Unassigned')).toBe('UN');
		expect(assigneeInitials('')).toBe('?');
	});
});
