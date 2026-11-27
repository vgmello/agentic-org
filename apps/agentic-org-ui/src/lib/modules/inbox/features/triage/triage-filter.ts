import type { InboxScope } from '../../types';
import type {
	TriageFilterOptions,
	TriageGroup,
	TriageItem,
	TriageOrder,
	TriageTab
} from './triage-types';

const DAY_MS = 24 * 60 * 60 * 1000;

export function matchesScope(item: TriageItem, scope: InboxScope): boolean {
	if (scope.team && item.teamId !== scope.team) return false;
	if (scope.project && item.projectId !== scope.project) return false;
	return true;
}

export function matchesSearch(item: TriageItem, query: string): boolean {
	const needle = query.trim().toLowerCase();
	if (!needle) return true;
	const haystack = [
		item.title,
		item.actorName,
		item.actorRole,
		item.projectName,
		item.teamName ?? '',
		item.typeLabel,
		item.kindDisplay,
		item.workRef ?? '',
		item.workRefDisplay ?? '',
		item.why,
		item.recommendation ?? '',
		item.snoozed?.note ?? '',
		item.handled?.outcome ?? ''
	]
		.join(' ')
		.toLowerCase();
	return haystack.includes(needle);
}

const TAB_CATEGORY_MAP: Partial<Record<TriageTab, TriageItem['category']>> = {
	approvals: 'approval',
	decisions: 'decision',
	questions: 'question',
	problems: 'problem'
};

function matchesTab(item: TriageItem, tab: TriageTab): boolean {
	if (tab === 'handled') return item.state === 'handled';
	if (tab === 'snoozed') return item.state === 'snoozed';
	if (item.state !== 'open') return false;
	if (tab === 'all') return true;
	if (tab === 'work') return item.sourceKind === 'work';
	return item.category === TAB_CATEGORY_MAP[tab];
}

function compareSnoozed(a: TriageItem, b: TriageItem): number {
	const aUntil = a.snoozed?.untilIso ? Date.parse(a.snoozed.untilIso) : Number.POSITIVE_INFINITY;
	const bUntil = b.snoozed?.untilIso ? Date.parse(b.snoozed.untilIso) : Number.POSITIVE_INFINITY;
	return aUntil !== bUntil ? aUntil - bUntil : b.timestampMs - a.timestampMs;
}

function comparePriority(a: TriageItem, b: TriageItem): number {
	const aExp = a.expiresAtIso ? Date.parse(a.expiresAtIso) : Number.POSITIVE_INFINITY;
	const bExp = b.expiresAtIso ? Date.parse(b.expiresAtIso) : Number.POSITIVE_INFINITY;
	if (aExp !== bExp) return aExp - bExp;
	if (a.urgent !== b.urgent) return a.urgent ? -1 : 1;
	return b.timestampMs - a.timestampMs;
}

export function sortTriageItems(
	items: readonly TriageItem[],
	tab: TriageTab,
	order: TriageOrder = 'priority'
): TriageItem[] {
	if (tab === 'snoozed') return [...items].sort(compareSnoozed);
	if (tab === 'handled' || order === 'date') {
		return [...items].sort((a, b) => b.timestampMs - a.timestampMs);
	}
	return [...items].sort(comparePriority);
}

export function filterTriageItems(
	items: readonly TriageItem[],
	options: TriageFilterOptions
): TriageItem[] {
	const checkUrgent = options.tab !== 'handled' && options.tab !== 'snoozed' && options.urgentOnly;
	const matched = items.filter(
		(item) =>
			matchesTab(item, options.tab) &&
			(!checkUrgent || item.urgent) &&
			matchesScope(item, options.scope) &&
			matchesSearch(item, options.search)
	);
	return sortTriageItems(matched, options.tab, options.order ?? 'priority');
}

const CATEGORY_COUNT_KEY: Record<
	Exclude<TriageItem['category'], 'work'>,
	'approvals' | 'decisions' | 'questions' | 'problems'
> = {
	approval: 'approvals',
	decision: 'decisions',
	question: 'questions',
	problem: 'problems'
};

export function triageTabCounts(
	items: readonly TriageItem[],
	scope: InboxScope = {}
): Record<TriageTab, number> {
	const counts: Record<TriageTab, number> = {
		all: 0,
		approvals: 0,
		decisions: 0,
		questions: 0,
		snoozed: 0,
		handled: 0,
		problems: 0,
		work: 0
	};
	for (const item of items) {
		if (!matchesScope(item, scope)) continue;
		if (item.state === 'handled') {
			counts.handled += 1;
			continue;
		}
		if (item.state === 'snoozed') {
			counts.snoozed += 1;
			continue;
		}
		counts.all += 1;
		if (item.category !== 'work') counts[CATEGORY_COUNT_KEY[item.category]] += 1;
		if (item.sourceKind === 'work') counts.work += 1;
	}
	return counts;
}

export function openWaitingCount(items: readonly TriageItem[]): number {
	return items.filter((i) => i.state === 'open').length;
}

export function triageScopeOptions(items: readonly TriageItem[]): {
	teams: { id: string; name: string }[];
	projects: { id: string; name: string }[];
} {
	const teams = new Map<string, string>();
	const projects = new Map<string, string>();
	for (const item of items) {
		if (item.teamId && item.teamName) teams.set(item.teamId, item.teamName);
		if (item.projectId && item.projectName) projects.set(item.projectId, item.projectName);
	}
	const toSorted = (map: Map<string, string>) =>
		[...map].map(([id, name]) => ({ id, name })).sort((a, b) => a.name.localeCompare(b.name));
	return { teams: toSorted(teams), projects: toSorted(projects) };
}

export function groupTriageItems(
	items: readonly TriageItem[],
	tab: TriageTab,
	nowMs = Date.now(),
	order: TriageOrder = 'priority'
): TriageGroup[] {
	if (items.length === 0) return [];
	if (tab === 'handled') return [{ key: 'handled', label: 'Handled', items: [...items] }];
	if (tab === 'snoozed') return [{ key: 'snoozed', label: 'Snoozed', items: [...items] }];

	const urgent: TriageItem[] = [];
	const today: TriageItem[] = [];
	const earlier: TriageItem[] = [];
	for (const item of items) {
		if (order === 'priority' && item.urgent) urgent.push(item);
		else if (nowMs - item.timestampMs < DAY_MS) today.push(item);
		else earlier.push(item);
	}
	const groups: TriageGroup[] = [];
	if (urgent.length > 0) groups.push({ key: 'urgent', label: 'Urgent', items: urgent });
	if (today.length > 0) groups.push({ key: 'today', label: 'Today', items: today });
	if (earlier.length > 0) groups.push({ key: 'earlier', label: 'Earlier', items: earlier });
	return groups;
}

export function flattenGroups(groups: readonly TriageGroup[]): TriageItem[] {
	return groups.flatMap((g) => g.items);
}

/**
 * Returns ordered candidate IDs to select after `resolvedId` leaves `visibleItems`:
 * items after `resolvedId` first (in order), then items before `resolvedId` (nearest first).
 */
export function neighborCandidatesAfterResolve(
	visibleItems: readonly TriageItem[],
	resolvedId: string
): string[] {
	const index = visibleItems.findIndex((i) => i.id === resolvedId);
	if (index < 0) return visibleItems.map((i) => i.id);
	const after = visibleItems.slice(index + 1).map((i) => i.id);
	const before = visibleItems
		.slice(0, index)
		.reverse()
		.map((i) => i.id);
	return [...after, ...before];
}

/**
 * Returns the next item ID to select after `resolvedId` leaves the visible queue:
 * next neighbour first, otherwise previous neighbour, otherwise undefined.
 */
export function nextSelectionAfterResolve(
	visibleItems: readonly TriageItem[],
	resolvedId: string
): string | undefined {
	return neighborCandidatesAfterResolve(visibleItems, resolvedId)[0];
}

export function isDeniedOutcome(outcome: string | undefined): boolean {
	if (!outcome) return false;
	const lower = outcome.trim().toLowerCase();
	return lower.startsWith('denied') || lower.startsWith('declined');
}
