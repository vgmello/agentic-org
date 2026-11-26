import { describe, expect, it } from 'vitest';
import { defaultFilters } from '../../filters';
import { workRowComparator } from '../../present';
import type { TaskRow } from '../../types';
import { buildActiveFilterChips } from './active-chips';

function makeRow(overrides: Partial<TaskRow>): TaskRow {
	return {
		id: 'id-1',
		ref: 'T-A1',
		type: 'task',
		title: 'Alpha task',
		status: 'todo',
		priority: 'normal',
		severity: null,
		team: { id: 'team-1', name: 'Core' },
		assignedTeam: null,
		assignee: null,
		reporter: { id: 'p-1', name: 'Alex', kind: 'person' },
		project: null,
		createdAt: '2026-11-01T10:00:00Z',
		updatedAt: '2026-11-01T10:00:00Z',
		statusAt: '2026-11-01T10:00:00Z',
		needsMe: null,
		awaitingVerification: false,
		budget: null,
		budgetHold: undefined,
		blockedReason: null,
		checklist: { done: 0, total: 0, requiredLeft: 0 },
		reopenCount: 0,
		effort: null,
		revision: 1,
		...overrides
	};
}

describe('buildActiveFilterChips', () => {
	it('returns empty list when default filters are active', () => {
		expect(
			buildActiveFilterChips({
				filters: defaultFilters(),
				activeTab: 'task',
				projects: [],
				teams: [],
				people: []
			})
		).toEqual([]);
	});

	it('formats active filters with human labels and URL sync metadata', () => {
		const chips = buildActiveFilterChips({
			filters: {
				...defaultFilters(),
				saved: 'reported_by_me',
				status: 'blocked',
				priority: 'urgent',
				projectId: 'none',
				assigneeId: 'p-1',
				query: 'checkout'
			},
			activeTab: 'request',
			projects: [['pr-1', 'Agentic Org v1']],
			teams: [['team-1', 'Core']],
			people: [['p-1', 'Alex Kim']]
		});

		expect(chips.map((c) => c.label)).toEqual([
			'Scope: Raised by me',
			'Status: Blocked',
			'Priority: Urgent',
			'Project: No project',
			'Assignee: Alex Kim',
			'Search: "checkout"'
		]);
		expect(chips.find((c) => c.id === 'project')?.syncUrl).toEqual({ projectId: '' });
		expect(chips.find((c) => c.id === 'assignee')?.syncUrl).toEqual({ assigneeId: '' });
	});
});

describe('workRowComparator', () => {
	const a = makeRow({
		id: 'a',
		ref: 'T-A1',
		title: 'Beta',
		priority: 'low',
		createdAt: '2026-11-02T00:00:00Z'
	});
	const b = makeRow({
		id: 'b',
		ref: 'T-A2',
		title: 'Alpha',
		priority: 'urgent',
		createdAt: '2026-11-01T00:00:00Z'
	});

	it('sorts by title ascending and descending', () => {
		expect([a, b].sort(workRowComparator('task', 'title', 'asc')).map((r) => r.ref)).toEqual([
			'T-A2',
			'T-A1'
		]);
		expect([a, b].sort(workRowComparator('task', 'title', 'desc')).map((r) => r.ref)).toEqual([
			'T-A1',
			'T-A2'
		]);
	});

	it('sorts by priority and created timestamp respecting direction', () => {
		expect([a, b].sort(workRowComparator('task', 'priority', 'asc')).map((r) => r.ref)).toEqual([
			'T-A2',
			'T-A1'
		]);
		expect([a, b].sort(workRowComparator('task', 'created', 'desc')).map((r) => r.ref)).toEqual([
			'T-A1',
			'T-A2'
		]);
	});
});
