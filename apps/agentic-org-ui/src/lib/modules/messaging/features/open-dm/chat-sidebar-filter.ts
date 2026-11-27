import type { ChatMember } from '$lib/modules/messaging/engine';
import type { ConversationRow, Sidebar } from '$lib/modules/messaging/model';

export interface FilteredChatSidebar {
	channels: ConversationRow[];
	dms: ConversationRow[];
	asOwner: ConversationRow[];
	total: number;
	quickDmCandidates: ChatMember[];
}

export function filterChatSidebar(
	bar: Sidebar,
	members: readonly ChatMember[],
	rawQuery: string
): FilteredChatSidebar {
	const q = rawQuery.trim().toLowerCase();
	if (!q) {
		return {
			channels: bar.channels,
			dms: bar.dms,
			asOwner: bar.asOwner,
			total: bar.channels.length + bar.dms.length + bar.asOwner.length,
			quickDmCandidates: []
		};
	}
	const matchesRow = (r: ConversationRow) =>
		r.title.toLowerCase().includes(q) ||
		Boolean(r.other?.name.toLowerCase().includes(q)) ||
		Boolean(r.other?.handle.toLowerCase().includes(q));
	const channels = bar.channels.filter(matchesRow);
	const dms = bar.dms.filter(matchesRow);
	const asOwner = bar.asOwner.filter(matchesRow);
	const total = channels.length + dms.length + asOwner.length;
	const quickDmCandidates =
		total === 0
			? members
					.filter(
						(m) =>
							!m.owner &&
							!m.former &&
							(m.name.toLowerCase().includes(q) || m.handle.toLowerCase().includes(q))
					)
					.slice(0, 4)
			: [];
	return { channels, dms, asOwner, total, quickDmCandidates };
}
