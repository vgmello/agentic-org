import { describe, expect, it } from 'vitest';
import type { OrgSnapshot, WorkItemSummary } from '$lib/proto';
import { nameOf } from './people';
import {
	deriveBoardColumns,
	deriveWorkScopeOptions,
	filterWorkItems,
	groupWorkItems,
	hasWorkScopeFilters,
	matchesWorkSearch,
	openWorkCount,
	workKindCounts
} from './work-toolbar';

function makeItem(
	partial: Partial<WorkItemSummary> & Pick<WorkItemSummary, 'id' | 'kind' | 'status'>
): WorkItemSummary {
	return {
		short_id: partial.short_id ?? partial.id,
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

const snapshot = {
	projects: [
		{ id: 'beacon', name: 'Beacon', key: 'BCN', archived: false },
		{ id: 'atlas', name: 'Atlas', key: 'ATL', archived: false },
		{ id: 'legacy', name: 'Legacy', key: 'LEG', archived: true }
	],
	members: [
		{ id: 'owner', name: 'Alex Kim' },
		{ id: 'sam', name: 'Sam Okafor' },
		{ id: 'noor', name: 'Noor Haddad' }
	]
} as unknown as OrgSnapshot;

const projectName = (id: string | undefined) =>
	(snapshot.projects ?? []).find((p) => p.id === id)?.name ?? 'No project';
const assigneeName = (id: string | undefined) => nameOf(snapshot, id);

describe('work toolbar helpers', () => {
	const sample: WorkItemSummary[] = [
		makeItem({
			id: 't1',
			kind: 'task',
			status: 'in_progress',
			project_id: 'beacon',
			number: 2,
			short_id: 'BCN-2',
			title: 'Ship telemetry panel',
			assignee_id: 'sam'
		}),
		makeItem({
			id: 't2',
			kind: 'task',
			status: 'blocked',
			project_id: 'atlas',
			number: 1,
			short_id: 'ATL-1',
			title: 'Custody lock fix',
			assignee_id: 'owner'
		}),
		makeItem({
			id: 't3',
			kind: 'task',
			status: 'done',
			project_id: 'atlas',
			number: 3,
			short_id: 'ATL-3',
			title: 'Cleanup build script'
		}),
		makeItem({
			id: 'b1',
			kind: 'bug',
			status: 'new',
			project_id: 'atlas',
			number: 1,
			short_id: 'BUG-1',
			title: 'Crash on resize',
			assignee_id: 'noor'
		}),
		makeItem({
			id: 'b2',
			kind: 'bug',
			status: 'closed',
			project_id: 'beacon',
			number: 2,
			short_id: 'BUG-2',
			title: 'Old typo'
		}),
		makeItem({
			id: 'r1',
			kind: 'request',
			status: 'scoped',
			project_id: 'atlas',
			number: 1,
			short_id: 'REQ-1',
			title: 'Weekly digest'
		})
	];

	it('counts open work items and per-kind totals while ignoring deleted items', () => {
		const withDeleted = [
			...sample,
			makeItem({ id: 't-del', kind: 'task', status: 'todo', deleted: true })
		];
		expect(openWorkCount(withDeleted)).toBe(4);
		expect(workKindCounts(withDeleted)).toEqual({ tasks: 3, bugs: 2, requests: 1 });
	});

	it('matches search queries across title, ref, short_id, project name, and assignee name', () => {
		const ctx = {
			refText: 'ATL-1',
			projectName: 'Atlas',
			assigneeName: 'Alex Kim'
		};
		expect(matchesWorkSearch(sample[1], '', ctx)).toBe(true);
		expect(matchesWorkSearch(sample[1], 'custody', ctx)).toBe(true);
		expect(matchesWorkSearch(sample[1], 'atl-1', ctx)).toBe(true);
		expect(matchesWorkSearch(sample[1], 'ATLAS', ctx)).toBe(true);
		expect(matchesWorkSearch(sample[1], 'alex', ctx)).toBe(true);
		expect(matchesWorkSearch(sample[1], 'nonexistent', ctx)).toBe(false);
	});

	it('filters by saved filter, project, assignee, unassigned, and search query in deterministic project order', () => {
		const baseOpts = {
			filterTest: (i: WorkItemSummary) => i.kind === 'task',
			getRef: (i: WorkItemSummary) => i.short_id,
			getProjectName: projectName,
			getAssigneeName: assigneeName
		};

		const allTasks = filterWorkItems(sample, { ...baseOpts, scope: {}, query: '' });
		expect(allTasks.map((i) => i.id)).toEqual(['t2', 't3', 't1']);

		const byProject = filterWorkItems(sample, {
			...baseOpts,
			scope: { project: 'beacon' },
			query: ''
		});
		expect(byProject.map((i) => i.id)).toEqual(['t1']);

		const byAssignee = filterWorkItems(sample, {
			...baseOpts,
			scope: { assignee: 'owner' },
			query: ''
		});
		expect(byAssignee.map((i) => i.id)).toEqual(['t2']);

		const unassigned = filterWorkItems(sample, {
			...baseOpts,
			scope: { assignee: 'unassigned' },
			query: ''
		});
		expect(unassigned.map((i) => i.id)).toEqual(['t3']);

		const searched = filterWorkItems(sample, {
			...baseOpts,
			scope: {},
			query: 'Sam Okafor'
		});
		expect(searched.map((i) => i.id)).toEqual(['t1']);

		expect(hasWorkScopeFilters({})).toBe(false);
		expect(hasWorkScopeFilters({ project: 'atlas' })).toBe(true);
		expect(hasWorkScopeFilters({ assignee: 'unassigned' })).toBe(true);
	});

	it('derives sorted project and assignee scope options from active projects and items', () => {
		const opts = deriveWorkScopeOptions(sample, snapshot);
		expect(opts.projects).toEqual([
			{ id: 'atlas', name: 'Atlas' },
			{ id: 'beacon', name: 'Beacon' }
		]);
		expect(opts.assignees).toEqual([
			{ id: 'owner', name: 'Alex Kim' },
			{ id: 'noor', name: 'Noor Haddad' },
			{ id: 'sam', name: 'Sam Okafor' }
		]);
		expect(opts.hasUnassigned).toBe(true);
	});

	it('groups items by status or alphabetically by project with No project last', () => {
		const tasks = [
			...sample.filter((i) => i.kind === 'task'),
			makeItem({ id: 't4', kind: 'task', status: 'todo', title: 'Loose task' })
		];

		const byStatus = groupWorkItems(tasks, 'tasks', 'status', projectName);
		expect(byStatus.map((g) => g.key)).toEqual(['blocked', 'in_progress', 'todo', 'done']);

		const byProject = groupWorkItems(tasks, 'tasks', 'project', projectName);
		expect(byProject.map((g) => g.label)).toEqual(['Atlas', 'Beacon', 'No project']);
		expect(byProject[0].items.map((i) => i.id)).toEqual(['t2', 't3']);
	});

	it('derives fixed Board columns for Tasks and Bugs while retaining extra statuses', () => {
		const taskCols = deriveBoardColumns(
			sample.filter((i) => i.kind === 'task'),
			'tasks'
		);
		expect(taskCols.map((c) => [c.status, c.label, c.items.length])).toEqual([
			['todo', 'To do', 0],
			['in_progress', 'In progress', 1],
			['blocked', 'Blocked', 1],
			['done', 'Done', 1]
		]);

		const bugCols = deriveBoardColumns(
			sample.filter((i) => i.kind === 'bug'),
			'bugs'
		);
		expect(bugCols.map((c) => [c.status, c.label, c.items.length])).toEqual([
			['new', 'New', 1],
			['triaged', 'Triaged', 0],
			['in_progress', 'In progress', 0],
			['fixed', 'Fixed', 0],
			['closed', 'Closed', 1]
		]);
	});
});
