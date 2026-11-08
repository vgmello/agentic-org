// The cross-project Work screen's saved filters and what needs the owner (work-items-ux §11; U8).
import type { WorkItemSummary } from '$lib/proto';

export type WorkTab = 'tasks' | 'bugs';

export interface SavedFilter {
	id: string;
	label: string;
	test: (item: WorkItemSummary) => boolean;
}

const isTask = (i: WorkItemSummary) => i.kind === 'task';
const isBug = (i: WorkItemSummary) => i.kind === 'bug';
const openTask = (i: WorkItemSummary) => isTask(i) && i.status !== 'done';
const openBug = (i: WorkItemSummary) => isBug(i) && i.status !== 'closed';

/** The saved filters of a tab, with "me" being the owner. Counts come from `countOf`. */
export function savedFilters(tab: WorkTab, me: string): SavedFilter[] {
	if (tab === 'tasks')
		return [
			{ id: 'mine', label: 'Assigned to me', test: (i) => openTask(i) && i.assignee_id === me },
			{ id: 'blocked', label: 'Blocked tasks', test: (i) => isTask(i) && i.status === 'blocked' },
			{ id: 'all', label: 'All', test: isTask }
		];
	return [
		{ id: 'mine', label: 'Assigned to me', test: (i) => openBug(i) && i.assignee_id === me },
		{ id: 'triage', label: 'Needs my triage', test: (i) => isBug(i) && i.status === 'new' },
		{ id: 'verify', label: 'Waiting for me to verify', test: (i) => isBug(i) && i.status === 'fixed' },
		{ id: 'serious', label: 'Critical and Major bugs', test: (i) => openBug(i) && (i.severity === 'critical' || i.severity === 'major') },
		{ id: 'all', label: 'All', test: isBug }
	];
}

export const countOf = (items: WorkItemSummary[], filter: SavedFilter): number => items.filter(filter.test).length;

/**
 * The filter a tab opens on: Bugs on "Needs my triage" if it has any, else "Critical and Major bugs"; Tasks on "Blocked
 * tasks" if any, else "All".
 */
export function defaultFilter(tab: WorkTab, items: WorkItemSummary[], me: string): string {
	const filters = savedFilters(tab, me);
	const has = (id: string) => countOf(items, filters.find((f) => f.id === id)!) > 0;
	if (tab === 'bugs') return has('triage') ? 'triage' : 'serious';
	return has('blocked') ? 'blocked' : 'all';
}

/** What needs the owner: bugs to triage + bugs to verify + blocked tasks assigned to them (the nav badge, U8). */
export function needsYou(items: WorkItemSummary[], me: string): number {
	return items.filter((i) => !i.deleted && ((isBug(i) && (i.status === 'new' || i.status === 'fixed')) || (isTask(i) && i.status === 'blocked' && i.assignee_id === me))).length;
}
