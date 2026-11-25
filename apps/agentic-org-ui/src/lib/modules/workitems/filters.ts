// Queue filters, saved filters, Show menu views and view preference storage (work-items-ux §6, §9, §11;
// feature-requests-ux §2). Pure helpers so every screen and test uses one rule.
import type { Priority, Severity, TaskRow, TaskStatus, TaskType, Viewer, WorkQueue } from './types';
import { isOpen, normalizeRef } from './present';

export type ShowView = 'needs_me' | 'mine' | 'by_project' | 'by_assignee';

export const SHOW_VIEWS: { id: ShowView; label: string }[] = [
	{ id: 'needs_me', label: 'Needs me' },
	{ id: 'mine', label: 'Mine' },
	{ id: 'by_project', label: 'By project' },
	{ id: 'by_assignee', label: 'By assignee' }
];

export const isShowView = (value: string | null | undefined): value is ShowView =>
	SHOW_VIEWS.some((v) => v.id === value);

/** Default Show view (work-items-ux §11): Needs me when supported owner-waiting work exists, else Mine. */
export function defaultShowView(queue: Pick<WorkQueue, 'rows' | 'needsMeSources'>): ShowView {
	if (queue.needsMeSources.length > 0 && queue.rows.some((r) => r.needsMe !== null))
		return 'needs_me';
	return 'mine';
}

export type SavedFilterId =
	| 'all'
	| 'owned_by_my_team'
	| 'assigned_to_my_team'
	| 'assigned_to_me'
	| 'reported_by_me'
	| 'blocked';

export interface SavedFilterDef {
	id: SavedFilterId;
	label: string;
	/** Label when shown on the Requests tab (feature-requests-ux §2). */
	requestLabel?: string;
	needsEmployee?: boolean;
}

export const SAVED_FILTERS: SavedFilterDef[] = [
	{ id: 'all', label: 'All' },
	{ id: 'owned_by_my_team', label: 'Owned by my team', needsEmployee: true },
	{ id: 'assigned_to_my_team', label: 'Assigned to my team', needsEmployee: true },
	{ id: 'assigned_to_me', label: 'Assigned to me', needsEmployee: true },
	{
		id: 'reported_by_me',
		label: 'Reported by me',
		requestLabel: 'Raised by me',
		needsEmployee: true
	},
	{ id: 'blocked', label: 'Blocked' }
];

export interface QueueFilters {
	saved: SavedFilterId;
	status: TaskStatus | 'open' | 'all';
	severity: Severity | 'unset' | 'all';
	priority: Priority | 'all';
	teamId: string;
	assignedTeamId: string;
	assigneeId: string;
	reporterId: string;
	/** '' = all, 'none' = projectless, otherwise a project ID. */
	projectId: string;
	query: string;
}

export const defaultFilters = (): QueueFilters => ({
	saved: 'all',
	status: 'all',
	severity: 'all',
	priority: 'all',
	teamId: '',
	assignedTeamId: '',
	assigneeId: '',
	reporterId: '',
	projectId: '',
	query: ''
});

export function hasActiveFilters(f: QueueFilters): boolean {
	return (
		f.saved !== 'all' ||
		f.status !== 'all' ||
		f.severity !== 'all' ||
		f.priority !== 'all' ||
		f.teamId !== '' ||
		f.assignedTeamId !== '' ||
		f.assigneeId !== '' ||
		f.reporterId !== '' ||
		f.projectId !== '' ||
		f.query.trim() !== ''
	);
}

/** Matches one saved filter using the viewer's admitted employee context, never the owner flag (WORK-2). */
export function matchesSavedFilter(row: TaskRow, saved: SavedFilterId, viewer: Viewer): boolean {
	const me = viewer.employee?.id;
	const myTeams = viewer.teamIds;
	switch (saved) {
		case 'all':
			return true;
		case 'owned_by_my_team':
			return !!me && myTeams.includes(row.team.id);
		case 'assigned_to_my_team':
			return !!me && !!row.assignedTeam && myTeams.includes(row.assignedTeam.id);
		case 'assigned_to_me':
			return !!me && row.assignee?.id === me;
		case 'reported_by_me':
			return !!me && row.reporter.id === me;
		case 'blocked':
			return row.status === 'blocked';
	}
}

/** Rows waiting on the owner from available sources. */
export function needsMeRows(rows: TaskRow[]): TaskRow[] {
	return rows.filter((r) => r.needsMe !== null);
}

/** Rows belonging to the viewer's admitted employee (assigned to me, my teams, or reported by me). */
export function mineRows(rows: TaskRow[], viewer: Viewer): TaskRow[] {
	if (!viewer.employee) return [];
	const me = viewer.employee.id;
	return rows.filter(
		(r) =>
			r.assignee?.id === me ||
			r.reporter.id === me ||
			viewer.teamIds.includes(r.team.id) ||
			(!!r.assignedTeam && viewer.teamIds.includes(r.assignedTeam.id))
	);
}

function matchesStatus(row: TaskRow, status: QueueFilters['status']): boolean {
	if (status === 'all') return true;
	return status === 'open' ? isOpen(row.status) : row.status === status;
}

function matchesSeverity(row: TaskRow, severity: QueueFilters['severity']): boolean {
	if (severity === 'all') return true;
	return severity === 'unset' ? row.severity === null : row.severity === severity;
}

function matchesNullableId(refId: string | undefined, wanted: string): boolean {
	if (!wanted) return true;
	return wanted === 'none' ? !refId : refId === wanted;
}

function matchesQuery(row: TaskRow, rawQuery: string): boolean {
	const q = rawQuery.trim().toLowerCase();
	if (!q) return true;
	const qRef = normalizeRef(rawQuery);
	return (
		row.ref.toUpperCase() === qRef ||
		row.ref.toLowerCase().includes(q) ||
		row.title.toLowerCase().includes(q) ||
		row.team.name.toLowerCase().includes(q) ||
		(row.project?.name.toLowerCase().includes(q) ?? false)
	);
}

function matchesFilters(row: TaskRow, viewer: Viewer, filters: QueueFilters): boolean {
	return (
		matchesSavedFilter(row, filters.saved, viewer) &&
		matchesStatus(row, filters.status) &&
		matchesSeverity(row, filters.severity) &&
		(filters.priority === 'all' || row.priority === filters.priority) &&
		(!filters.teamId || row.team.id === filters.teamId) &&
		matchesNullableId(row.assignedTeam?.id, filters.assignedTeamId) &&
		matchesNullableId(row.assignee?.id, filters.assigneeId) &&
		(!filters.reporterId || row.reporter.id === filters.reporterId) &&
		matchesNullableId(row.project?.id, filters.projectId) &&
		matchesQuery(row, filters.query)
	);
}

export function filterRows(
	rows: TaskRow[],
	viewer: Viewer,
	type: TaskType | 'all',
	filters: QueueFilters
): TaskRow[] {
	return rows.filter(
		(r) => (type === 'all' || r.type === type) && matchesFilters(r, viewer, filters)
	);
}

export function savedFilterCounts(
	rows: TaskRow[],
	viewer: Viewer,
	type: TaskType | 'all'
): Record<SavedFilterId, number> {
	const scoped = type === 'all' ? rows : rows.filter((r) => r.type === type);
	return {
		all: scoped.length,
		owned_by_my_team: scoped.filter((r) => matchesSavedFilter(r, 'owned_by_my_team', viewer))
			.length,
		assigned_to_my_team: scoped.filter((r) => matchesSavedFilter(r, 'assigned_to_my_team', viewer))
			.length,
		assigned_to_me: scoped.filter((r) => matchesSavedFilter(r, 'assigned_to_me', viewer)).length,
		reported_by_me: scoped.filter((r) => matchesSavedFilter(r, 'reported_by_me', viewer)).length,
		blocked: scoped.filter((r) => matchesSavedFilter(r, 'blocked', viewer)).length
	};
}

// ---- Storage helpers (safe when storage is unavailable) ------------------------------------------------------------

export type LayoutMode = 'board' | 'list';

export function readStoredString(key: string): string | null {
	try {
		return typeof localStorage === 'undefined' ? null : localStorage.getItem(key);
	} catch {
		return null;
	}
}

export function writeStoredString(key: string, value: string): void {
	try {
		if (typeof localStorage !== 'undefined') localStorage.setItem(key, value);
	} catch {
		// Private browsing or quota full: keep for this visit only.
	}
}

export function readStoredTab(organizationId: string): TaskType {
	const raw = readStoredString(`agentic-org.workitems.tab.${organizationId}`);
	if (raw === 'task' || raw === 'bug' || raw === 'request') return raw;
	return 'task';
}

export function writeStoredTab(organizationId: string, tab: TaskType): void {
	writeStoredString(`agentic-org.workitems.tab.${organizationId}`, tab);
}

export function readStoredLayout(scopeKey: string): LayoutMode {
	const raw = readStoredString(`agentic-org.workitems.layout.${scopeKey}`);
	return raw === 'list' ? 'list' : 'board';
}

export function writeStoredLayout(scopeKey: string, layout: LayoutMode): void {
	writeStoredString(`agentic-org.workitems.layout.${scopeKey}`, layout);
}

const WORK_DIVIDERS_KEY = 'agentic-org.workitems.dividers';

export function readStoredDividers(): boolean {
	return readStoredString(WORK_DIVIDERS_KEY) === '1';
}

export function writeStoredDividers(show: boolean): void {
	writeStoredString(WORK_DIVIDERS_KEY, show ? '1' : '0');
}
