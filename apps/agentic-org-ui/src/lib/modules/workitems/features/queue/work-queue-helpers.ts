import { tick } from 'svelte';
import { isEditableTarget } from '$lib/platform/shell/shortcuts';
import type { WorkStatusRingTone } from '$lib/modules/work/compact';
import type { ShowView } from '../../filters';
import type { GroupBy } from '../../present';
import type { Priority, TaskRow, TaskStatus, TaskType } from '../../types';

export const WORK_TABS: readonly { id: TaskType; label: string }[] = [
	{ id: 'task', label: 'Tasks' },
	{ id: 'bug', label: 'Bugs' },
	{ id: 'request', label: 'Requests' }
];

export function readingStatusTone(status: TaskStatus): WorkStatusRingTone {
	if (status === 'done' || status === 'cancelled') return 'done';
	if (status === 'blocked') return 'blocked';
	if (status === 'in_progress') return 'active';
	return 'neutral';
}

export function readingPriority(priority: Priority): {
	level: 1 | 2 | 3;
	label: string;
	strong: boolean;
} {
	if (priority === 'urgent') return { level: 3, label: 'Urgent priority', strong: true };
	if (priority === 'high') return { level: 3, label: 'High priority', strong: false };
	if (priority === 'normal') return { level: 2, label: 'Normal priority', strong: false };
	return { level: 1, label: 'Low priority', strong: false };
}

export function extractUniqueFacets(rows: readonly TaskRow[]): {
	uniqueProjects: [string, string][];
	uniqueTeams: [string, string][];
	uniquePeople: [string, string][];
} {
	const projects: Record<string, string> = {};
	const teams: Record<string, string> = {};
	const people: Record<string, string> = {};
	for (const r of rows) {
		if (r.project) projects[r.project.id] = r.project.name;
		teams[r.team.id] = r.team.name;
		if (r.assignedTeam) teams[r.assignedTeam.id] = r.assignedTeam.name;
		if (r.assignee) people[r.assignee.id] = r.assignee.name;
		people[r.reporter.id] = r.reporter.name;
	}
	const sortEntries = (rec: Record<string, string>) =>
		Object.entries(rec).sort((a, b) => a[1].localeCompare(b[1]));
	return {
		uniqueProjects: sortEntries(projects),
		uniqueTeams: sortEntries(teams),
		uniquePeople: sortEntries(people)
	};
}

export function buildSubgroupCandidates(
	activeTab: TaskType,
	isOrgScope: boolean,
	showMode: ShowView,
	primaryGroup: GroupBy
): GroupBy[] {
	const base: GroupBy[] =
		activeTab === 'bug' && (!isOrgScope || showMode === 'mine')
			? [
					'none',
					'status',
					'severity',
					'priority',
					...(isOrgScope ? (['project'] as GroupBy[]) : []),
					'assigned_team',
					'team',
					'assignee'
				]
			: [
					'none',
					'status',
					'priority',
					...(isOrgScope ? (['project'] as GroupBy[]) : []),
					'assigned_team',
					'team',
					'assignee'
				];
	return base.filter((g) => g === 'none' || g !== primaryGroup);
}

export function focusWorkSearchInput(uid: string): boolean {
	const headerInput = document.querySelector<HTMLInputElement>(
		'[data-testid="scoped-work-search"]'
	);
	if (headerInput && headerInput.offsetParent !== null) {
		headerInput.focus();
		return true;
	}
	const localInput = document.getElementById(`${uid}-search`) as HTMLInputElement | null;
	if (localInput) {
		localInput.focus();
		return true;
	}
	return false;
}

export function focusHeadingFromSearch(event: KeyboardEvent): void {
	if (event.isComposing || event.keyCode === 229 || event.key !== 'Escape') return;
	event.preventDefault();
	const heading = document.querySelector<HTMLElement>(
		'#project-heading, #shell-page-title, main h1, main h2'
	);
	if (heading) {
		if (!heading.hasAttribute('tabindex')) heading.tabIndex = -1;
		heading.focus({ preventScroll: true });
	} else {
		(event.currentTarget as HTMLInputElement | null)?.blur();
	}
}

export function restoreInlineDetailFocus(opts: {
	closedRef: string | null;
	trigger: HTMLElement | null;
	savedScrollTop: number;
	stillOpen: () => boolean;
}): void {
	void tick().then(() => {
		if (opts.stillOpen()) return;
		const scroller = document.querySelector<HTMLElement>('main');
		if (scroller && opts.savedScrollTop > 0) scroller.scrollTop = opts.savedScrollTop;
		if (opts.trigger?.isConnected && opts.trigger.offsetParent !== null) {
			opts.trigger.focus({ preventScroll: true });
			opts.trigger.scrollIntoView({ block: 'nearest', inline: 'nearest' });
			return;
		}
		if (opts.closedRef) {
			const links = document.querySelectorAll<HTMLElement>(
				`[data-ref="${CSS.escape(opts.closedRef)}"] a`
			);
			for (const link of links) {
				if (link.isConnected && link.offsetParent !== null) {
					link.focus({ preventScroll: true });
					link.scrollIntoView({ block: 'nearest', inline: 'nearest' });
					return;
				}
			}
		}
		document.getElementById('shell-page-title')?.focus({ preventScroll: true });
	});
}

const MODAL_OVERLAY_SELECTOR =
	'[role="dialog"]:not(.work-reading), [role="menu"], [role="alertdialog"], [role="listbox"]';

function isBareSlashEvent(event: KeyboardEvent): boolean {
	if (event.defaultPrevented || event.isComposing || event.keyCode === 229) return false;
	if (event.key !== '/' || event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) {
		return false;
	}
	return !document.querySelector(MODAL_OVERLAY_SELECTOR) && !isEditableTarget(event.target);
}

function isSelectOrComboTarget(target: EventTarget | null): boolean {
	if (target instanceof HTMLSelectElement) return true;
	return (
		target instanceof Element &&
		Boolean(target.closest('select, [role="listbox"], [role="combobox"]'))
	);
}

function isCloseDetailEscape(event: KeyboardEvent, hasOpenRow: boolean): boolean {
	if (!hasOpenRow || event.defaultPrevented || event.isComposing || event.keyCode === 229) {
		return false;
	}
	if (event.key !== 'Escape' || event.repeat) return false;
	if (document.querySelector(MODAL_OVERLAY_SELECTOR)) return false;
	return !isSelectOrComboTarget(event.target) && !isEditableTarget(event.target);
}

export interface WorkShortcutDeps {
	uid: string;
	shortcutsEnabled: () => boolean;
	hasOpenRow: () => boolean;
	needsShowSwitch: () => boolean;
	usesHeaderScopedSearch: () => boolean;
	onCloseDetail: (restoreFocus: boolean) => void;
	onFocusSearchAfterSwitch: () => void;
}

export function attachWorkSearchShortcuts(deps: WorkShortcutDeps): () => void {
	const onSearchKey = (event: KeyboardEvent) => {
		if (!isBareSlashEvent(event)) return;
		const openRow = deps.hasOpenRow();
		const needsSwitch = deps.needsShowSwitch();
		if (!openRow && !needsSwitch && deps.usesHeaderScopedSearch()) return;
		if (!openRow && !needsSwitch && !document.getElementById(`${deps.uid}-search`)) return;
		event.stopPropagation();
		if (!deps.shortcutsEnabled() || event.repeat) return;
		event.preventDefault();
		if (openRow) deps.onCloseDetail(false);
		if (openRow || needsSwitch) deps.onFocusSearchAfterSwitch();
		else focusWorkSearchInput(deps.uid);
	};
	const onEscapeKey = (event: KeyboardEvent) => {
		if (!isCloseDetailEscape(event, deps.hasOpenRow())) return;
		event.preventDefault();
		deps.onCloseDetail(true);
	};
	window.addEventListener('keydown', onSearchKey, { capture: true });
	window.addEventListener('keydown', onEscapeKey);
	return () => {
		window.removeEventListener('keydown', onSearchKey, { capture: true });
		window.removeEventListener('keydown', onEscapeKey);
	};
}

export const EMPTY_SAVED_COUNTS = {
	all: 0,
	owned_by_my_team: 0,
	assigned_to_my_team: 0,
	assigned_to_me: 0,
	reported_by_me: 0,
	blocked: 0
} as const;

export function groupOptionsForTab(tab: TaskType, isOrgScope: boolean): GroupBy[] {
	if (tab === 'bug') {
		return isOrgScope
			? ['assigned_team', 'severity', 'priority', 'status', 'project', 'team', 'assignee', 'none']
			: ['assigned_team', 'severity', 'priority', 'status', 'team', 'assignee', 'none'];
	}
	if (tab === 'request') {
		return isOrgScope
			? ['status', 'assigned_team', 'project', 'team', 'assignee', 'none']
			: ['status', 'assigned_team', 'team', 'assignee', 'none'];
	}
	return isOrgScope
		? ['none', 'status', 'priority', 'project', 'assigned_team', 'team', 'assignee']
		: ['none', 'status', 'priority', 'assigned_team', 'team', 'assignee'];
}

/** Keeps Done tasks available for the board's own Done history popover (`7d`/`30d`/`all`). */
export function resolveBoardBaseRows(opts: {
	allRows: TaskRow[];
	fallbackBase: TaskRow[];
	archivedIds: ReadonlySet<string>;
	showArchived: boolean;
	scopeMine: boolean;
	mineFilter: (rows: TaskRow[]) => TaskRow[];
}): TaskRow[] {
	if (opts.showArchived || opts.archivedIds.size === 0) return opts.fallbackBase;
	const scoped = opts.scopeMine ? opts.mineFilter(opts.allRows) : opts.allRows;
	return scoped.filter((r) => r.status === 'done' || !opts.archivedIds.has(r.id));
}
