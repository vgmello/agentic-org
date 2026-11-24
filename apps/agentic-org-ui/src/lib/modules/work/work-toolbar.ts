// Pure toolbar, search, scope-filter, grouping, and Board-column helpers for the cross-project Work view.
import type { OrgSnapshot, WorkItemSummary } from '$lib/proto';
import {
	groupWorkByStatus,
	projectDotTone,
	workStatusLabel,
	workStatusTone,
	type ProjectDotTone,
	type WorkStatusRingTone
} from './compact';
import { nameOf } from './people';
import type { WorkTab } from './saved';

export type WorkLayout = 'list' | 'board';
export type WorkGrouping = 'status' | 'project';

export const WORK_DIVIDERS_STORAGE_KEY = 'agentic-org.work.dividers';
export const WORK_LAYOUT_STORAGE_KEY = 'agentic-org.work.layout';
export const WORK_GROUPING_STORAGE_KEY = 'agentic-org.work.grouping';

export interface WorkScopeFilter {
	project?: string;
	assignee?: string;
}

export interface WorkScopeOption {
	id: string;
	name: string;
}

export interface WorkDisplayGroup {
	key: string;
	label: string;
	tone?: WorkStatusRingTone;
	dotTone?: ProjectDotTone;
	items: WorkItemSummary[];
}

export interface WorkBoardColumn {
	status: string;
	label: string;
	tone: WorkStatusRingTone;
	items: WorkItemSummary[];
}

export interface FilterWorkOptions {
	filterTest: (item: WorkItemSummary) => boolean;
	scope: WorkScopeFilter;
	query: string;
	getRef: (item: WorkItemSummary) => string;
	getProjectName: (projectId: string | undefined) => string;
	getAssigneeName: (assigneeId: string | undefined) => string;
}

const TASK_BOARD_STATUSES = ['todo', 'in_progress', 'blocked', 'done'] as const;
const BUG_BOARD_STATUSES = ['new', 'triaged', 'in_progress', 'fixed'] as const;

/** Whether any Work scope filter (Project or Assignee) is currently active. */
export function hasWorkScopeFilters(scope: WorkScopeFilter): boolean {
	return Boolean(scope.project || scope.assignee);
}

/** Count of non-deleted open work items across tasks, bugs, and requests. */
export function openWorkCount(items: readonly WorkItemSummary[]): number {
	let count = 0;
	for (const item of items) {
		if (item.deleted) continue;
		if (item.kind === 'task' && item.status !== 'done') count += 1;
		else if (item.kind === 'bug' && item.status !== 'closed') count += 1;
		else if (
			item.kind === 'request' &&
			item.status !== 'approved' &&
			item.status !== 'declined' &&
			item.status !== 'merged'
		) {
			count += 1;
		}
	}
	return count;
}

/** Counts per primary Work kind tab (non-deleted items). */
export function workKindCounts(items: readonly WorkItemSummary[]): {
	tasks: number;
	bugs: number;
	requests: number;
} {
	let tasks = 0;
	let bugs = 0;
	let requests = 0;
	for (const item of items) {
		if (item.deleted) continue;
		if (item.kind === 'task') tasks += 1;
		else if (item.kind === 'bug') bugs += 1;
		else if (item.kind === 'request') requests += 1;
	}
	return { tasks, bugs, requests };
}

/** Whether a work item matches the active Project / Assignee scope filter. */
export function matchesWorkScope(item: WorkItemSummary, scope: WorkScopeFilter): boolean {
	if (scope.project) {
		if (scope.project === 'none') {
			if (item.project_id) return false;
		} else if (item.project_id !== scope.project) {
			return false;
		}
	}
	if (scope.assignee) {
		if (scope.assignee === 'unassigned') {
			if (item.assignee_id) return false;
		} else if (item.assignee_id !== scope.assignee) {
			return false;
		}
	}
	return true;
}

/** Case-insensitive search match across title, ref, short_id, project name, and assignee name. */
export function matchesWorkSearch(
	item: WorkItemSummary,
	query: string,
	ctx: { refText: string; projectName: string; assigneeName: string }
): boolean {
	const trimmed = query.trim().toLowerCase();
	if (!trimmed) return true;
	const haystack = [item.title, ctx.refText, item.short_id, ctx.projectName, ctx.assigneeName]
		.filter(Boolean)
		.join('\n')
		.toLowerCase();
	return haystack.includes(trimmed);
}

/** Filter and deterministically sort visible work items for the active saved filter, scope, and search query. */
export function filterWorkItems(
	items: readonly WorkItemSummary[],
	opts: FilterWorkOptions
): WorkItemSummary[] {
	return items
		.filter((item) => {
			if (item.deleted) return false;
			if (!opts.filterTest(item)) return false;
			if (!matchesWorkScope(item, opts.scope)) return false;
			return matchesWorkSearch(item, opts.query, {
				refText: opts.getRef(item),
				projectName: opts.getProjectName(item.project_id),
				assigneeName: opts.getAssigneeName(item.assignee_id)
			});
		})
		.sort(
			(a, b) =>
				opts.getProjectName(a.project_id).localeCompare(opts.getProjectName(b.project_id)) ||
				(a.number ?? 0) - (b.number ?? 0) ||
				a.id.localeCompare(b.id)
		);
}

/** Derive sorted Project and Assignee options for the Filter work menu. */
export function deriveWorkScopeOptions(
	items: readonly WorkItemSummary[],
	snapshot: OrgSnapshot
): {
	projects: WorkScopeOption[];
	assignees: WorkScopeOption[];
	hasUnassigned: boolean;
} {
	const projectMap = new Map<string, string>();
	for (const project of snapshot.projects ?? []) {
		if (!project.archived) projectMap.set(project.id, project.name);
	}
	for (const item of items) {
		if (item.deleted || !item.project_id || projectMap.has(item.project_id)) continue;
		const matched = (snapshot.projects ?? []).find((p) => p.id === item.project_id);
		if (matched) projectMap.set(matched.id, matched.name);
	}
	const projects = [...projectMap.entries()]
		.map(([id, name]) => ({ id, name }))
		.sort((a, b) => a.name.localeCompare(b.name));

	const assigneeMap = new Map<string, string>();
	let hasUnassigned = false;
	for (const item of items) {
		if (item.deleted) continue;
		if (!item.assignee_id) {
			hasUnassigned = true;
			continue;
		}
		if (!assigneeMap.has(item.assignee_id)) {
			assigneeMap.set(item.assignee_id, nameOf(snapshot, item.assignee_id));
		}
	}
	const assignees = [...assigneeMap.entries()]
		.map(([id, name]) => ({ id, name }))
		.sort((a, b) => a.name.localeCompare(b.name));

	return { projects, assignees, hasUnassigned };
}

/** Group visible tasks or bugs by status or by project. */
export function groupWorkItems(
	items: readonly WorkItemSummary[],
	tab: Exclude<WorkTab, 'requests'>,
	grouping: WorkGrouping,
	getProjectName: (projectId: string | undefined) => string
): WorkDisplayGroup[] {
	if (grouping === 'status') {
		return groupWorkByStatus(items, tab).map((group) => ({
			key: group.status,
			label: group.label,
			tone: group.tone,
			items: group.items
		}));
	}

	const buckets = new Map<
		string,
		{ label: string; projectId?: string; items: WorkItemSummary[] }
	>();
	for (const item of items) {
		const key = item.project_id ?? '__none__';
		const existing = buckets.get(key);
		if (existing) {
			existing.items.push(item);
		} else {
			buckets.set(key, {
				label: getProjectName(item.project_id),
				projectId: item.project_id,
				items: [item]
			});
		}
	}

	return [...buckets.entries()]
		.sort(([keyA, groupA], [keyB, groupB]) => {
			if (keyA === '__none__' && keyB !== '__none__') return 1;
			if (keyB === '__none__' && keyA !== '__none__') return -1;
			return groupA.label.localeCompare(groupB.label);
		})
		.map(([key, group]) => ({
			key,
			label: group.label,
			dotTone: projectDotTone(group.projectId),
			items: group.items
		}));
}

/** Derive fixed status columns for the Work Board layout (To do / In progress / Blocked / Done for Tasks; New / Triaged / In progress / Fixed for Bugs). */
export function deriveBoardColumns(
	items: readonly WorkItemSummary[],
	tab: Exclude<WorkTab, 'requests'>
): WorkBoardColumn[] {
	const kind = tab === 'bugs' ? 'bug' : 'task';
	const baseStatuses: readonly string[] = tab === 'bugs' ? BUG_BOARD_STATUSES : TASK_BOARD_STATUSES;
	const buckets = new Map<string, WorkItemSummary[]>();
	for (const status of baseStatuses) buckets.set(status, []);
	for (const item of items) {
		const list = buckets.get(item.status);
		if (list) list.push(item);
		else buckets.set(item.status, [item]);
	}
	return [...buckets.entries()].map(([status, colItems]) => ({
		status,
		label: workStatusLabel(kind, status),
		tone: workStatusTone(kind, status),
		items: colItems
	}));
}
