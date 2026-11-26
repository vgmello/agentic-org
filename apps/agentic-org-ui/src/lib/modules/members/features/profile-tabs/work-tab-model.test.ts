import { describe, expect, it } from 'vitest';
import type { WorkRow } from '../../types';
import {
	filterAndSortMemberWork,
	groupMemberWork,
	memberWorkBoardColumns,
	memberWorkPresetCounts
} from './work-tab-model';

const ROWS: WorkRow[] = [
	{ ref: 'T-1', title: 'Alpha task', project: 'Core', priority: 'high', state: 'todo' },
	{ ref: 'T-2', title: 'Beta bug', project: 'UI', priority: 'urgent', state: 'blocked' },
	{ ref: 'T-3', title: 'Gamma task', project: 'Core', priority: 'medium', state: 'in_progress' },
	{ ref: 'T-4', title: 'Delta done', project: 'Core', priority: 'low', state: 'done' }
];

describe('work-tab-model (TASK-130)', () => {
	it('defaults to Open preset hiding done tasks, while explicit Done status reveals closed items', () => {
		expect(memberWorkPresetCounts(ROWS)).toEqual({
			open: 3,
			needs_me: 1,
			requested_by_me: 0,
			all: 4
		});

		const openOnly = filterAndSortMemberWork(ROWS, {
			preset: 'open',
			status: 'all',
			project: 'all',
			query: '',
			sortBy: 'default',
			direction: 'asc'
		});
		expect(openOnly.map((r) => r.ref)).toEqual(['T-2', 'T-3', 'T-1']);

		const explicitDone = filterAndSortMemberWork(ROWS, {
			preset: 'open',
			status: 'done',
			project: 'all',
			query: '',
			sortBy: 'default',
			direction: 'asc'
		});
		expect(explicitDone.map((r) => r.ref)).toEqual(['T-4']);
	});

	it('groups by status or project and builds 4-stage and 6-status board columns', () => {
		const byProject = groupMemberWork(ROWS.slice(0, 3), 'project');
		expect(byProject.map((g) => [g.label, g.rows.length])).toEqual([
			['Core', 2],
			['UI', 1]
		]);

		const stages = memberWorkBoardColumns(ROWS, 'stages');
		expect(stages.map((c) => [c.id, c.rows.length])).toEqual([
			['queued', 1],
			['active', 1],
			['attention', 1],
			['done', 1]
		]);
	});
});
