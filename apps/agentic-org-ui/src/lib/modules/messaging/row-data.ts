import type { ChatContext } from './engine';
import { nameOf } from './model';
import type { ChatMessage, ChatState, MuteMode } from './types';

export interface MessageRowData {
	replies: readonly ChatMessage[];
	folded?: string;
	mode: MuteMode;
}
export interface MessageRows {
	forMessage(message: ChatMessage): MessageRowData;
}
const empty: readonly ChatMessage[] = [];
const compare = (a: ChatMessage, b: ChatMessage) => Date.parse(a.created_at) - Date.parse(b.created_at) || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);

/** Index one snapshot for every row; rows never need the complete message or mute arrays. */
export function messageRows(state: ChatState, ctx: ChatContext): MessageRows {
	const repliesByRoot = new Map<string, ChatMessage[]>();
	const unordered = new Set<string>();
	for (const message of state.messages) {
		const root = message.thread_root_id;
		if (root === undefined) continue;
		const replies = repliesByRoot.get(root) ?? [];
		if (replies.length && compare(replies[replies.length - 1], message) > 0) unordered.add(root);
		replies.push(message);
		repliesByRoot.set(root, replies);
	}
	const foldWordsByRoot = new Map<string, string>();
	const names = new Map<string, string>();
	for (const [root, replies] of repliesByRoot) {
		if (unordered.has(root)) replies.sort(compare);
		const folded = replies.filter(message => message.folded);
		if (!folded.length) continue;
		const from = folded[0].from;
		if (!names.has(from)) names.set(from, nameOf(ctx, from));
		foldWordsByRoot.set(root, `${names.get(from)} posted ${folded.length} more ${folded.length === 1 ? 'update' : 'updates'}`);
	}
	const muteByKey = new Map<string, Map<string, MuteMode>>();
	for (const mute of state.mutes) {
		const members = muteByKey.get(mute.conversation_id) ?? new Map<string, MuteMode>();
		if (!members.has(mute.member_id)) members.set(mute.member_id, mute.mode);
		muteByKey.set(mute.conversation_id, members);
	}
	return {
		forMessage: message => ({ replies: repliesByRoot.get(message.id) ?? empty, folded: foldWordsByRoot.get(message.id), mode: muteByKey.get(message.conversation_id)?.get(message.from) ?? 'none' })
	};
}
