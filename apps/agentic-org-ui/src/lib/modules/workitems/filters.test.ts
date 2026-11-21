import { describe, expect, it } from 'vitest';
import {
	defaultFilters,
	defaultShowView,
	filterRows,
	hasActiveFilters,
	mineRows,
	needsMeRows,
	savedFilterCounts
} from './filters';
import type { TaskRow, Viewer } from './types';

const viewer: Viewer = {
	employee: { id: 'you', name: 'Alex Kim', kind: 'person' },
	teamIds: ['eng', 'web'],
	owner: true
};

const noEmployeeViewer: Viewer = { employee: null, teamIds: [], owner: true };

function row(overrides: Partial<TaskRow> = {}): TaskRow {
	return {
		id: 'id-1',
		ref: 'T-A1',
		type: 'task',
		title: 'Work item',
		status: 'todo',
		priority: 'normal',
		severity: null,
		effort: null,
		project: { id: 'beacon', name: 'Beacon', number: 8 },
		team: { id: 'web', name: 'Web' },
		assignedTeam: null,
		assignee: null,
		reporter: { id: 'avery', name: 'Avery Stone', kind: 'person' },
		createdAt: '2026-11-10T10:00:00Z',
		updatedAt: '2026-11-10T10:00:00Z',
		statusAt: '2026-11-10T10:00:00Z',
		checklist: { done: 0, total: 0, requiredLeft: 0 },
		blockedReason: null,
		reopenCount: 0,
		needsMe: null,
		budget: null,
		...overrides
	};
}

describe('workitems filters', () => {
	const rows: TaskRow[] = [
		row({
			id: '1',
			ref: 'T-A40',
			type: 'task',
			team: { id: 'web', name: 'Web' },
			assignedTeam: { id: 'platform', name: 'Platform' },
			assignee: { id: 'you', name: 'Alex Kim', kind: 'person' }
		}),
		row({
			id: '2',
			ref: 'T-A44',
			type: 'task',
			status: 'blocked',
			blockedReason: 'Waiting on provider',
			team: { id: 'platform', name: 'Platform' },
			assignedTeam: { id: 'web', name: 'Web' },
			reporter: { id: 'you', name: 'Alex Kim', kind: 'person' },
			needsMe: { source: 'owner_block', text: 'Waiting on your call', inboxItemId: 'inbox-3' }
		}),
		row({
			id: '3',
			ref: 'T-B17',
			type: 'request',
			project: null,
			team: { id: 'people', name: 'People' },
			assignedTeam: { id: 'people', name: 'People' }
		}),
		row({
			id: '4',
			ref: 'T-A46',
			type: 'bug',
			severity: null,
			team: { id: 'web', name: 'Web' }
		})
	];

	it('defaults Show view to Needs me only when supported owner-waiting work exists', () => {
		expect(defaultShowView({ rows, needsMeSources: ['owner_block'] })).toBe('needs_me');
		expect(defaultShowView({ rows, needsMeSources: [] })).toBe('mine');
		expect(defaultShowView({ rows: [rows[0]], needsMeSources: ['owner_block'] })).toBe('mine');
		expect(needsMeRows(rows).map((r) => r.ref)).toEqual(['T-A44']);
	});

	it('distinguishes accountable team, assigned team, assignee, reporter and projectless filters', () => {
		const counts = savedFilterCounts(rows, viewer, 'task');
		expect(counts).toEqual({
			all: 2,
			owned_by_my_team: 1,
			assigned_to_my_team: 1,
			assigned_to_me: 1,
			reported_by_me: 1,
			blocked: 1
		});

		// Owner without an employee identity never synthesizes "my" matches (WORK-2).
		expect(mineRows(rows, noEmployeeViewer)).toEqual([]);
		expect(savedFilterCounts(rows, noEmployeeViewer, 'task').assigned_to_me).toBe(0);

		const projectless = filterRows(rows, viewer, 'request', {
			...defaultFilters(),
			projectId: 'none'
		});
		expect(projectless.map((r) => r.ref)).toEqual(['T-B17']);

		const unsetSeverity = filterRows(rows, viewer, 'bug', {
			...defaultFilters(),
			severity: 'unset'
		});
		expect(unsetSeverity.map((r) => r.ref)).toEqual(['T-A46']);

		const byRef = filterRows(rows, viewer, 'all', { ...defaultFilters(), query: 't-a44' });
		expect(byRef.map((r) => r.ref)).toEqual(['T-A44']);

		expect(hasActiveFilters(defaultFilters())).toBe(false);
		expect(hasActiveFilters({ ...defaultFilters(), projectId: 'none' })).toBe(true);
	});
});
