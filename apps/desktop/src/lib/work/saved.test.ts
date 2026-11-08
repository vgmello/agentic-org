import { describe, expect, it } from 'vitest';
import type { WorkItemSummary } from '$lib/proto';
import { countOf, defaultFilter, needsYou, savedFilters } from './saved';

const item = (id: string, over: Partial<WorkItemSummary>): WorkItemSummary => ({
	id,
	kind: 'task',
	project_id: 'p',
	number: 1,
	short_id: id,
	title: id,
	status: 'todo',
	priority: 'normal',
	created_by: 'owner',
	created_at: '2026-11-01T00:00:00.000Z',
	updated_at: '2026-11-01T00:00:00.000Z',
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
	...over
});
const ME = 'owner';

describe('saved filters and defaults', () => {
	const items = [
		item('t1', { assignee_id: ME }),
		item('t2', { status: 'blocked', assignee_id: ME }),
		item('t3', { status: 'done', assignee_id: ME }),
		item('b1', { kind: 'bug', status: 'new', severity: 'minor' }),
		item('b2', { kind: 'bug', status: 'fixed', severity: 'minor' }),
		item('b3', { kind: 'bug', status: 'triaged', severity: 'critical', assignee_id: ME }),
		item('b4', { kind: 'bug', status: 'closed', severity: 'critical' })
	];
	const count = (tab: 'tasks' | 'bugs', id: string) => countOf(items, savedFilters(tab, ME).find((f) => f.id === id)!);
	it('counts each saved filter', () => {
		expect([count('tasks', 'mine'), count('tasks', 'blocked'), count('tasks', 'all')]).toEqual([2, 1, 3]);
		expect([count('bugs', 'mine'), count('bugs', 'triage'), count('bugs', 'verify'), count('bugs', 'serious'), count('bugs', 'all')]).toEqual([1, 1, 1, 1, 4]);
	});
	it('opens Bugs on Needs my triage when there are some, else Critical and Major; Tasks on Blocked when any, else All', () => {
		expect(defaultFilter('bugs', items, ME)).toBe('triage');
		expect(defaultFilter('bugs', items.filter((i) => i.id !== 'b1'), ME)).toBe('serious');
		expect(defaultFilter('tasks', items, ME)).toBe('blocked');
		expect(defaultFilter('tasks', items.filter((i) => i.id !== 't2'), ME)).toBe('all');
	});
	it('what needs the owner: bugs to triage and to verify, and blocked tasks assigned to them', () => {
		expect(needsYou(items, ME)).toBe(3);
		expect(needsYou([item('t', { status: 'blocked', assignee_id: 'sam' })], ME)).toBe(0);
		expect(needsYou([item('b', { kind: 'bug', status: 'new', deleted: true })], ME)).toBe(0);
	});
});
