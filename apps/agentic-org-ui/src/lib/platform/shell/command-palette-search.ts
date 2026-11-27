/**
 * Dynamic entity indexing and query matching for the executive command palette.
 */
import type { InboxItem } from '$lib/modules/inbox/types';
import { nonOwners } from '$lib/modules/organization/model';
import type { OrgSnapshot, WorkItemSummary } from '$lib/proto';
import type { CommandEntry } from './command-palette-entries';

export function buildDynamicEntries(
	snapshot: OrgSnapshot | undefined,
	inboxItems: readonly InboxItem[],
	items: readonly WorkItemSummary[]
): CommandEntry[] {
	const out: CommandEntry[] = [];
	if (snapshot) {
		for (const p of (snapshot.projects ?? []).filter((x) => !x.archived)) {
			out.push({
				id: `proj-${p.id}`,
				group: 'Projects',
				tag: 'Project',
				icon: 'folder',
				label: p.name,
				sub: p.key ? `Project · ${p.key.toUpperCase()}` : 'Project',
				keywords: `${p.id} ${p.key ?? ''}`,
				href: `/projects/${p.id}/work`
			});
		}
		for (const m of nonOwners(snapshot)) {
			out.push({
				id: `mem-${m.id}`,
				group: 'Team',
				tag: 'Member',
				icon: 'users',
				label: m.name,
				sub: m.agent?.title ?? m.agent?.role ?? m.kind,
				keywords: `${m.id} ${m.agent?.role ?? ''} ${m.agent?.messaging_name ?? ''}`,
				href: `/members/${m.id}`
			});
		}
	}
	for (const inboxItem of inboxItems) {
		out.push({
			id: `inbox-${inboxItem.id}`,
			group: 'Inbox',
			tag: 'Inbox',
			icon: 'inbox',
			label: inboxItem.title,
			sub: `${inboxItem.source} · ${inboxItem.who.name}`,
			keywords: `${inboxItem.id} ${inboxItem.project?.name ?? ''} ${inboxItem.why}`,
			href:
				inboxItem.state === 'handled'
					? `/inbox?tab=handled&item=${encodeURIComponent(inboxItem.id)}`
					: `/inbox?item=${encodeURIComponent(inboxItem.id)}`
		});
	}
	for (const item of items.filter((i) => !i.deleted)) {
		const dest =
			item.kind === 'request' || !item.project_id
				? `/work/${encodeURIComponent(item.short_id)}`
				: `/projects/${encodeURIComponent(item.project_id)}/work`;
		out.push({
			id: `work-${item.id}`,
			group: 'Work',
			tag: 'Work',
			icon: 'clipboard',
			label: item.title,
			sub: `${item.short_id} · ${item.status.replace('_', ' ')}`,
			keywords: `${item.short_id} ${item.kind} ${item.id}`,
			href: dest
		});
	}
	return out;
}

export function entryMatchesQuery(entry: CommandEntry, q: string): boolean {
	return (
		entry.label.toLowerCase().includes(q) ||
		entry.sub.toLowerCase().includes(q) ||
		entry.group.toLowerCase().includes(q) ||
		entry.tag.toLowerCase().includes(q) ||
		Boolean(entry.keywords?.toLowerCase().includes(q))
	);
}

export function computeCommandEntries(input: {
	query: string;
	isChat: boolean;
	modalOpen: boolean;
	channelEntries: readonly CommandEntry[];
	baseList: readonly CommandEntry[];
	dynamicEntries: readonly CommandEntry[];
	onOpenModalWithQuery: (q: string) => void;
	onAskTess: (prompt: string) => void;
}): CommandEntry[] {
	const trimmed = input.query.trim();
	if (trimmed.startsWith('#')) {
		const cq = trimmed.slice(1).trim().toLowerCase();
		return cq
			? input.channelEntries.filter((e) => entryMatchesQuery(e, cq))
			: [...input.channelEntries];
	}
	const q = trimmed.toLowerCase();
	if (!q) return [...input.baseList];

	const searchable = input.isChat
		? [...input.channelEntries, ...input.baseList, ...input.dynamicEntries]
		: [...input.baseList, ...input.dynamicEntries, ...input.channelEntries];
	const allMatched = searchable.filter((e) => entryMatchesQuery(e, q));
	const shown =
		!input.modalOpen && allMatched.length > 11 ? allMatched.slice(0, 11) : [...allMatched];

	if (!input.modalOpen && allMatched.length > 11) {
		shown.push({
			id: 'show-all-matches',
			group: 'Search',
			tag: 'All',
			icon: 'search',
			label: `Show all ${allMatched.length} matches`,
			sub: 'Open full searchable results list',
			action: () => input.onOpenModalWithQuery(trimmed)
		});
	}
	shown.push({
		id: 'ask-tess-fallback',
		group: 'Ask Tess',
		tag: 'AI',
		icon: 'sparkles',
		label: `Ask Tess about '${trimmed}'`,
		sub: 'Open Tess Chief of Staff panel for this question',
		action: () => input.onAskTess(trimmed)
	});
	return shown;
}
