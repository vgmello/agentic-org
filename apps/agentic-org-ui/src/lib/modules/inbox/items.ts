import { KIND_WORD, type InboxItem, type InboxScope, type InboxTab } from './types';

/** The most urgent first: items that lapse soonest, then the newest. */
export function sortOpen(items: InboxItem[]): InboxItem[] {
	return [...items].sort((a, b) => {
		if (a.expires_at && b.expires_at) return a.expires_at.localeCompare(b.expires_at);
		if (a.expires_at) return -1;
		if (b.expires_at) return 1;
		return b.created_at.localeCompare(a.created_at);
	});
}

/** Handled items, most recently handled first. */
export const sortHandled = (items: InboxItem[]): InboxItem[] =>
	[...items].sort((a, b) => (b.handled?.at ?? '').localeCompare(a.handled?.at ?? ''));

export const openItems = (items: InboxItem[]) => sortOpen(items.filter((i) => i.state === 'open'));

/** The count shown on Home's "Open Inbox (N)" and in the navigation badge: every open item. */
export const openCount = (items: InboxItem[]) => items.filter((i) => i.state === 'open').length;

const TAB_KIND = {
	approvals: 'approval',
	questions: 'question',
	decisions: 'decision',
	problems: 'problem'
} as const;

export function inScope(item: InboxItem, scope: InboxScope): boolean {
	if (scope.team && item.team?.id !== scope.team) return false;
	if (scope.project && item.project?.id !== scope.project) return false;
	return true;
}

/** What a tab lists under the Show filters. */
export function tabItems(items: InboxItem[], tab: InboxTab, scope: InboxScope = {}): InboxItem[] {
	const scoped = items.filter((i) => inScope(i, scope));
	if (tab === 'handled') return sortHandled(scoped.filter((i) => i.state === 'handled'));
	const open = scoped.filter((i) => i.state === 'open');
	return sortOpen(tab === 'all' ? open : open.filter((i) => i.kind === TAB_KIND[tab]));
}

/** The number beside each tab: open items per kind, and handled items. */
export function tabCounts(items: InboxItem[], scope: InboxScope = {}): Record<InboxTab, number> {
	const counts: Record<InboxTab, number> = {
		all: 0,
		approvals: 0,
		questions: 0,
		decisions: 0,
		problems: 0,
		handled: 0
	};
	for (const tab of Object.keys(counts) as InboxTab[])
		counts[tab] = tabItems(items, tab, scope).length;
	return counts;
}

/** The teams and projects that appear on items, for the Show menu. */
export function scopeOptions(items: InboxItem[]) {
	const teams = new Map<string, string>();
	const projects = new Map<string, string>();
	for (const item of items) {
		if (item.team) teams.set(item.team.id, item.team.name);
		if (item.project) projects.set(item.project.id, item.project.name);
	}
	const sorted = (m: Map<string, string>) =>
		[...m].map(([id, name]) => ({ id, name })).sort((a, b) => a.name.localeCompare(b.name));
	return { teams: sorted(teams), projects: sorted(projects) };
}

/** The item to select after `id` leaves `list`: the next one, else the previous, else none. */
export function neighbour(list: InboxItem[], id: string): InboxItem | undefined {
	const index = list.findIndex((i) => i.id === id);
	if (index < 0) return undefined;
	return list[index + 1] ?? list[index - 1];
}

/** "Kai Lindqvist · Platform · Atlas": who, then where. */
export const whoLine = (item: InboxItem): string =>
	[item.who.name, item.team?.name, item.project?.name].filter(Boolean).join(' · ');

export const kindLabel = (item: InboxItem): string => `${KIND_WORD[item.kind]} · ${item.source}`;
