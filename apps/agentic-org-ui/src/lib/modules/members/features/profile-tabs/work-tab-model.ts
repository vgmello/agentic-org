import { WORK_PRIORITY_LABELS, WORK_STATE_LABELS } from '../../present';
import type { WorkRow, WorkState } from '../../types';

export type MemberWorkPreset = 'open' | 'needs_me' | 'all';
export type MemberWorkGroupBy = 'status' | 'project' | 'none';
export type MemberWorkSortBy = 'default' | 'title' | 'priority';
export type MemberWorkDirection = 'asc' | 'desc';
export type MemberWorkLayout = 'list' | 'board';
export type MemberWorkBoardMode = 'stages' | 'statuses';

export interface MemberWorkViewState {
	preset: MemberWorkPreset;
	status: 'all' | WorkState;
	project: 'all' | string;
	query: string;
	sortBy: MemberWorkSortBy;
	direction: MemberWorkDirection;
}

export interface MemberWorkGroup {
	id: string;
	label: string;
	rows: WorkRow[];
}

const STATE_ORDER: readonly WorkState[] = ['blocked', 'in_progress', 'in_review', 'todo', 'done'];
const PRIORITY_RANK: Record<NonNullable<WorkRow['priority']>, number> = {
	urgent: 0,
	high: 1,
	medium: 2,
	low: 3
};

export function isNeedsAttention(row: WorkRow): boolean {
	return row.state === 'blocked' || row.state === 'in_review';
}

export function memberWorkPresetCounts(rows: readonly WorkRow[]): Record<MemberWorkPreset, number> {
	let open = 0;
	let needsMe = 0;
	for (const row of rows) {
		if (row.state !== 'done') open += 1;
		if (isNeedsAttention(row)) needsMe += 1;
	}
	return { open, needs_me: needsMe, all: rows.length };
}

export function memberWorkProjects(rows: readonly WorkRow[]): string[] {
	const set = new Set<string>();
	for (const row of rows) {
		if (row.project) set.add(row.project);
	}
	return [...set].sort((a, b) => a.localeCompare(b));
}

function matchesSearch(row: WorkRow, needle: string): boolean {
	return [
		row.ref,
		row.title,
		row.project,
		row.state,
		WORK_STATE_LABELS[row.state],
		row.priority ? WORK_PRIORITY_LABELS[row.priority] : undefined
	]
		.filter((part): part is string => Boolean(part))
		.some((part) => part.toLowerCase().includes(needle));
}

function compareWorkRows(a: WorkRow, b: WorkRow, sortBy: MemberWorkSortBy): number {
	if (sortBy === 'title') {
		return a.title.localeCompare(b.title, 'en', { sensitivity: 'base' });
	}
	if (sortBy === 'priority') {
		const pa = a.priority ? PRIORITY_RANK[a.priority] : 2;
		const pb = b.priority ? PRIORITY_RANK[b.priority] : 2;
		return pa - pb || a.ref.localeCompare(b.ref);
	}
	const sa = STATE_ORDER.indexOf(a.state);
	const sb = STATE_ORDER.indexOf(b.state);
	return sa - sb || a.ref.localeCompare(b.ref);
}

export function filterAndSortMemberWork(
	rows: readonly WorkRow[],
	state: MemberWorkViewState
): WorkRow[] {
	const needle = state.query.trim().toLowerCase();
	const filtered = rows.filter((row) => {
		if (state.status !== 'all') {
			if (row.state !== state.status) return false;
		} else if (state.preset === 'open' && row.state === 'done') {
			return false;
		} else if (state.preset === 'needs_me' && !isNeedsAttention(row)) {
			return false;
		}
		if (state.project !== 'all' && row.project !== state.project) return false;
		if (needle && !matchesSearch(row, needle)) return false;
		return true;
	});

	const dir = state.direction === 'desc' ? -1 : 1;
	return filtered.toSorted((a, b) => compareWorkRows(a, b, state.sortBy) * dir);
}

export function groupMemberWork(
	rows: readonly WorkRow[],
	groupBy: MemberWorkGroupBy
): MemberWorkGroup[] {
	if (groupBy === 'none') {
		return rows.length ? [{ id: 'all', label: '', rows: [...rows] }] : [];
	}
	if (groupBy === 'status') {
		const out: MemberWorkGroup[] = [];
		for (const st of STATE_ORDER) {
			const bucket = rows.filter((r) => r.state === st);
			if (bucket.length > 0) {
				out.push({ id: st, label: WORK_STATE_LABELS[st], rows: bucket });
			}
		}
		return out;
	}
	const byProject = new Map<string, WorkRow[]>();
	for (const row of rows) {
		const key = row.project || 'No project';
		const list = byProject.get(key) ?? [];
		list.push(row);
		byProject.set(key, list);
	}
	return [...byProject.entries()]
		.sort(([a], [b]) => a.localeCompare(b))
		.map(([label, bucket]) => ({ id: label, label, rows: bucket }));
}

export function memberWorkBoardColumns(
	rows: readonly WorkRow[],
	mode: MemberWorkBoardMode
): MemberWorkGroup[] {
	if (mode === 'stages') {
		return [
			{ id: 'queued', label: 'To do', rows: rows.filter((r) => r.state === 'todo') },
			{
				id: 'active',
				label: 'In progress',
				rows: rows.filter((r) => r.state === 'in_progress')
			},
			{
				id: 'attention',
				label: 'Review & Blocked',
				rows: rows.filter((r) => r.state === 'in_review' || r.state === 'blocked')
			},
			{ id: 'done', label: 'Done', rows: rows.filter((r) => r.state === 'done') }
		];
	}
	return STATE_ORDER.map((st) => ({
		id: st,
		label: WORK_STATE_LABELS[st],
		rows: rows.filter((r) => r.state === st)
	}));
}
