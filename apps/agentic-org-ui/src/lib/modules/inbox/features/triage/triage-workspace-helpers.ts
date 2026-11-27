import { isEditableTarget } from '$lib/platform/shell/shortcuts';
import type { OrgSnapshot } from '$lib/proto';
import { isDenyAction } from '../../spotlight';
import {
	parseTriageTab,
	triageScopeOptions,
	type TriageItem,
	type TriageTab
} from './triage-model';

export function mergeScopeOptions(
	allItems: readonly TriageItem[],
	snapshot: OrgSnapshot
): {
	teams: { id: string; name: string }[];
	projects: { id: string; name: string }[];
} {
	const fromItems = triageScopeOptions(allItems);
	const teams: Record<string, string> = {};
	for (const t of fromItems.teams) teams[t.id] = t.name;
	for (const t of snapshot.teams ?? []) {
		if (!Object.hasOwn(teams, t.id)) teams[t.id] = t.name;
	}
	const projects: Record<string, string> = {};
	for (const p of fromItems.projects) projects[p.id] = p.name;
	for (const p of snapshot.projects ?? []) {
		if (!p.archived && !Object.hasOwn(projects, p.id)) projects[p.id] = p.name;
	}
	const toSorted = (record: Record<string, string>) =>
		Object.entries(record)
			.map(([id, name]) => ({ id, name }))
			.sort((a, b) => a.name.localeCompare(b.name));
	return { teams: toSorted(teams), projects: toSorted(projects) };
}

function nextTabIndex(key: string, index: number, length: number): number | undefined {
	if (key === 'Home') return 0;
	if (key === 'End') return length - 1;
	if (key === 'ArrowRight') return (index + 1) % length;
	if (key === 'ArrowLeft') return (index - 1 + length) % length;
	return undefined;
}

export function moveCategoryTabFocus(event: KeyboardEvent): void {
	if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
	const row = (event.currentTarget as HTMLElement | null)?.closest('.primary-tab-row');
	if (!row) return;
	const options = [...row.querySelectorAll<HTMLButtonElement>('.queue-tab')];
	const index = options.indexOf(event.currentTarget as HTMLButtonElement);
	if (index < 0 || options.length === 0) return;
	const next = nextTabIndex(event.key, index, options.length);
	if (next === undefined) return;
	event.preventDefault();
	options[next]?.focus();
	options[next]?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
}

export type TriageShortcutAction = 'next' | 'prev' | 'approve' | 'deny' | undefined;

export function resolveTriageShortcut(
	key: string,
	selectedItem: TriageItem | undefined,
	actionLocked: boolean
): TriageShortcutAction {
	if (key === 'j') return 'next';
	if (key === 'k') return 'prev';
	if (!selectedItem || selectedItem.state !== 'open' || actionLocked) return undefined;
	if (key === 'a') {
		const primaryKind = selectedItem.decision?.primaryAction.kind;
		return primaryKind === 'approve_inbox' || primaryKind === 'approve_request'
			? 'approve'
			: undefined;
	}
	if (key === 'd') {
		const canDeny =
			selectedItem.inboxItem?.actions.includes('deny') ||
			isDenyAction(selectedItem.decision?.secondaryAction);
		return canDeny ? 'deny' : undefined;
	}
	return undefined;
}

export function computeRelativeSelectionId(
	visibleItems: readonly TriageItem[],
	currentId: string | undefined,
	delta: number
): string | undefined {
	if (visibleItems.length === 0) return undefined;
	const currentIdx = visibleItems.findIndex((i) => i.id === currentId);
	const nextIdx = Math.max(
		0,
		Math.min(visibleItems.length - 1, (currentIdx < 0 ? 0 : currentIdx) + delta)
	);
	return visibleItems[nextIdx]?.id;
}

function isIgnoredShortcutKeydown(event: KeyboardEvent): boolean {
	if (event.defaultPrevented || event.isComposing) return true;
	if (event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) return true;
	return (
		Boolean(document.querySelector('[role="dialog"], [role="menu"]')) ||
		isEditableTarget(event.target)
	);
}

export interface WorkspaceShortcutCallbacks {
	shortcutsEnabled: () => boolean;
	denyGuardVisible: () => boolean;
	selectedItem: () => TriageItem | undefined;
	actionLocked: () => boolean;
	onOpenSearch: () => void;
	onNavigateRelative: (delta: number) => void;
	onApprove: (item: TriageItem) => void;
	onArmDeny: (itemId: string) => void;
	onSelectTab: (tab: TriageTab) => void;
	onShowCategories: () => void;
}

function isShortcutBlocked(event: KeyboardEvent, cb: WorkspaceShortcutCallbacks): boolean {
	return !cb.shortcutsEnabled() || event.repeat || cb.denyGuardVisible();
}

function isInsideChromeRegion(target: EventTarget | null): boolean {
	return (
		target instanceof Element &&
		Boolean(target.closest('#triage-categories, .triage-toolbar, aside.exec-sidebar'))
	);
}

function runShortcutAction(
	action: Exclude<TriageShortcutAction, undefined>,
	item: TriageItem | undefined,
	cb: WorkspaceShortcutCallbacks
): void {
	if (action === 'next') cb.onNavigateRelative(1);
	else if (action === 'prev') cb.onNavigateRelative(-1);
	else if (action === 'approve' && item) cb.onApprove(item);
	else if (action === 'deny' && item) cb.onArmDeny(item.id);
}

function dispatchWorkspaceKeydown(event: KeyboardEvent, cb: WorkspaceShortcutCallbacks): void {
	if (isIgnoredShortcutKeydown(event)) return;
	if (event.key === '/') {
		event.stopPropagation();
		if (isShortcutBlocked(event, cb)) return;
		event.preventDefault();
		cb.onOpenSearch();
		return;
	}
	if (isShortcutBlocked(event, cb) || isInsideChromeRegion(event.target)) return;
	const item = cb.selectedItem();
	const action = resolveTriageShortcut(event.key, item, cb.actionLocked());
	if (!action) return;
	event.preventDefault();
	runShortcutAction(action, item, cb);
}

export function attachWorkspaceShortcuts(
	node: HTMLElement,
	cb: WorkspaceShortcutCallbacks
): () => void {
	const listener = (event: KeyboardEvent) => dispatchWorkspaceKeydown(event, cb);
	const onNavTabSelect = (event: Event) => {
		const tabId = (event as CustomEvent<{ tabId?: string }>).detail?.tabId;
		if (tabId) cb.onSelectTab(parseTriageTab(tabId));
	};
	const onShowCategories = () => cb.onShowCategories();
	node.addEventListener('keydown', listener);
	node.addEventListener('inbox:select-tab', onNavTabSelect);
	node.addEventListener('inbox:show-categories', onShowCategories);
	return () => {
		node.removeEventListener('keydown', listener);
		node.removeEventListener('inbox:select-tab', onNavTabSelect);
		node.removeEventListener('inbox:show-categories', onShowCategories);
	};
}
