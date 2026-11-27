// Organization context, naming, sidebar, conversation views and times for Chat (CHAT-1..3, CHAT-8..12).
import type { OrgSnapshot } from '$lib/proto';
import { isFormer } from '$lib/modules/organization/model';
import {
	canRead,
	isMember,
	memberOf,
	mentionsHere,
	mentionsIn,
	OFFICE,
	slug,
	type ChatContext,
	type ChatMember
} from './engine';
import type { ChatMessage, ChatState, Conversation, MessageKind, Presence, Typing } from './types';

export const ms = (iso: string) => Date.parse(iso);

export function chatMembers(s: OrgSnapshot): ChatMember[] {
	return s.members
		.filter((m) => m.kind === 'agent' || m.kind === 'person')
		.map((m) => ({
			id: m.id,
			kind: m.kind as 'agent' | 'person',
			name: m.name,
			handle: m.owner
				? 'owner'
				: m.kind === 'agent' && m.agent?.messaging_name
					? m.agent.messaging_name
					: slug(m.name) || m.id,
			...(m.owner ? { owner: true } : {}),
			...(isFormer(m) ? { former: true } : {})
		}));
}

export function chatContext(
	s: OrgSnapshot,
	init: {
		now: string;
		actor?: string;
		surface?: ChatContext['surface'];
		newId?: ChatContext['newId'];
		typing_ms?: number;
		directory?: ChatMember[];
	}
): ChatContext {
	const owner = s.members.find((m) => m.owner)?.id ?? 'owner';
	const members = chatMembers(s);
	for (const m of init.directory ?? []) if (!members.some((x) => x.id === m.id)) members.push(m);
	return {
		now: init.now,
		actor: init.actor ?? owner,
		surface: init.surface ?? 'app',
		owner,
		members,
		teams: s.teams.map((t) => ({
			id: t.id,
			name: t.name,
			member_ids: s.members.filter((m) => !m.owner && m.home_team_id === t.id).map((m) => m.id)
		})),
		projects: (s.projects ?? []).map((p) => ({
			id: p.id,
			key: p.key,
			name: p.name,
			team_ids: (s.project_teams ?? []).filter((l) => l.project_id === p.id).map((l) => l.team_id)
		})),
		...(init.newId ? { newId: init.newId } : {}),
		...(init.typing_ms !== undefined ? { typing_ms: init.typing_ms } : {})
	};
}

/** Product notices show as System: display copy over `sender_kind: office`, never a member (chat-ux §4.2). */
export const SYSTEM_NAME = 'System';

export const nameOf = (ctx: ChatContext, id: string): string =>
	id === 'default'
		? 'Default'
		: id === OFFICE
			? SYSTEM_NAME
			: (memberOf(ctx, id)?.name ?? 'Someone');

/** "you" for the viewer, the name for anyone else (chat-ux §2.1: app copy uses you). */
export const whoOf = (ctx: ChatContext, id: string): string =>
	id === ctx.actor ? 'you' : nameOf(ctx, id);

/** "#agentic-org", a person's or agent's name, or the group's names (CHAT-3). */
export function titleOf(c: Conversation, ctx: ChatContext, viewer: string): string {
	if (c.kind === 'channel') return `#${c.name}`;
	const others = c.members.filter((id) => id !== viewer);
	return (others.length ? others : c.members).map((id) => nameOf(ctx, id)).join(', ');
}

export const PRESENCE_WORD: Record<Presence, string> = {
	working: 'Working',
	waiting: 'Waiting for an answer',
	asleep: 'Available',
	offline: 'Offline',
	paused: 'Paused',
	online: 'Online'
};

/** The presence to show for a member, or none: people have no speculative runtime presence (CHAT-3). */
export const presenceOf = (s: ChatState, member: ChatMember | undefined): Presence | undefined =>
	member?.kind === 'agent' ? (s.presence[member.id] ?? 'offline') : undefined;

export const KIND_WORD: Record<MessageKind, string> = {
	msg: 'Message',
	request: 'Ask',
	handoff: 'Hand-off',
	announcement: 'Announcement',
	reply: 'Reply',
	unknown: 'Message'
};

export const muteOf = (s: ChatState, conversationId: string, from: string) =>
	s.mutes.find((x) => x.conversation_id === conversationId && x.member_id === from)?.mode ?? 'none';

export const waitingOnOwner = (m: ChatMessage, ctx: ChatContext): boolean =>
	m.addressed_to === ctx.owner &&
	((m.kind === 'request' && !m.answer && !m.withdrawn) ||
		(m.kind === 'handoff' && !m.accepted_by && !m.handed_back));

export const needsYou = (s: ChatState, ctx: ChatContext): ChatMessage[] =>
	s.messages
		.filter((m) => waitingOnOwner(m, ctx))
		.sort((a, b) => ms(b.created_at) - ms(a.created_at));

const mentionsOwner = (m: ChatMessage, ctx: ChatContext, c: Conversation) =>
	m.from !== ctx.owner &&
	(mentionsIn(m.body, ctx).includes(ctx.owner) ||
		(mentionsHere(m.body) && c.kind === 'channel' && isMember(c, ctx.owner)));

export function mentionsOf(s: ChatState, ctx: ChatContext): ChatMessage[] {
	return s.messages
		.filter((m) => {
			const c = s.conversations.find((x) => x.id === m.conversation_id);
			return !!c && mentionsOwner(m, ctx, c);
		})
		.sort((a, b) => ms(b.created_at) - ms(a.created_at));
}

export interface ThreadSummary {
	root: ChatMessage;
	conversation: Conversation;
	replies: number;
	last_at: string;
	/** Replies from others since the owner last read the conversation. */
	new_replies: number;
}

export const repliesTo = (s: ChatState, rootId: string): ChatMessage[] =>
	s.messages
		.filter((m) => m.thread_root_id === rootId)
		.sort((a, b) => ms(a.created_at) - ms(b.created_at));

export function threadsOf(s: ChatState, ctx: ChatContext): ThreadSummary[] {
	const out: ThreadSummary[] = [];
	for (const root of s.messages.filter((m) => m.thread_root_id === undefined)) {
		const replies = repliesTo(s, root.id);
		if (!replies.length) continue;
		const c = s.conversations.find((x) => x.id === root.conversation_id);
		if (!c || !isMember(c, ctx.owner)) continue;
		const inIt =
			root.from === ctx.owner ||
			replies.some((r) => r.from === ctx.owner || mentionsIn(r.body, ctx).includes(ctx.owner)) ||
			mentionsIn(root.body, ctx).includes(ctx.owner);
		if (!inIt) continue;
		const readAt = ms(s.read_at[c.id] ?? '1970-01-01T00:00:00Z');
		out.push({
			root,
			conversation: c,
			replies: replies.length,
			last_at: replies[replies.length - 1].created_at,
			new_replies: replies.filter(
				(r) =>
					r.from !== ctx.owner && ms(r.created_at) > readAt && muteOf(s, c.id, r.from) === 'none'
			).length
		});
	}
	return out.sort((a, b) => ms(b.last_at) - ms(a.last_at));
}

export interface ConversationRow {
	conversation: Conversation;
	title: string;
	unread: number;
	mentions: number;
	other?: ChatMember;
	presence?: Presence;
	readingAsOwner: boolean;
}

export function unreadIn(
	s: ChatState,
	ctx: ChatContext,
	c: Conversation
): { unread: number; mentions: number } {
	const readAt = ms(s.read_at[c.id] ?? '1970-01-01T00:00:00Z');
	const fresh = s.messages.filter(
		(m) =>
			m.conversation_id === c.id &&
			m.from !== ctx.owner &&
			ms(m.created_at) > readAt &&
			(muteOf(s, c.id, m.from) === 'none' || waitingOnOwner(m, ctx))
	);
	return { unread: fresh.length, mentions: fresh.filter((m) => mentionsOwner(m, ctx, c)).length };
}

export interface Sidebar {
	needsYou: ChatMessage[];
	mentions: ChatMessage[];
	threads: ThreadSummary[];
	channels: ConversationRow[];
	dms: ConversationRow[];
	asOwner: ConversationRow[];
}

const rowOf = (s: ChatState, ctx: ChatContext, c: Conversation): ConversationRow => {
	const other =
		c.kind === 'dm'
			? memberOf(
					ctx,
					c.members.find((id) => id !== ctx.owner)
				)
			: undefined;
	const presence = presenceOf(s, other);
	return {
		conversation: c,
		title: titleOf(c, ctx, ctx.owner),
		...unreadIn(s, ctx, c),
		...(other ? { other } : {}),
		...(presence ? { presence } : {}),
		readingAsOwner: !isMember(c, ctx.owner)
	};
};

const lastAt = (s: ChatState, c: Conversation) =>
	s.messages
		.filter((m) => m.conversation_id === c.id)
		.reduce((t, m) => Math.max(t, ms(m.created_at)), ms(c.created_at));

export function sidebar(s: ChatState, ctx: ChatContext): Sidebar {
	const live = s.conversations.filter((c) => !c.archived && canRead(ctx, ctx.owner, c));
	const mine = live.filter((c) => isMember(c, ctx.owner));
	const byName = (a: ConversationRow, b: ConversationRow) =>
		a.title.localeCompare(b.title, undefined, { sensitivity: 'base' });
	return {
		needsYou: needsYou(s, ctx),
		mentions: mentionsOf(s, ctx),
		threads: threadsOf(s, ctx),
		channels: mine
			.filter((c) => c.kind === 'channel')
			.map((c) => rowOf(s, ctx, c))
			.sort(byName),
		dms: mine
			.filter((c) => c.kind !== 'channel')
			.sort((a, b) => lastAt(s, b) - lastAt(s, a))
			.map((c) => rowOf(s, ctx, c)),
		asOwner: live
			.filter((c) => !isMember(c, ctx.owner))
			.map((c) => rowOf(s, ctx, c))
			.sort(byName)
	};
}

export interface DigestGroup {
	member_id: string;
	messages: ChatMessage[];
}

export interface ConversationView {
	shown: ChatMessage[];
	muted: ChatMessage[];
	digests: DigestGroup[];
}

export function conversationView(
	s: ChatState,
	ctx: ChatContext,
	conversationId: string
): ConversationView {
	const all = s.messages
		.filter(
			(m) =>
				m.conversation_id === conversationId &&
				(m.thread_root_id === undefined || m.also_in_channel)
		)
		.sort((a, b) => ms(a.created_at) - ms(b.created_at));
	const view: ConversationView = { shown: [], muted: [], digests: [] };
	for (const m of all) {
		const mode = m.from === ctx.owner ? 'none' : muteOf(s, conversationId, m.from);
		if (mode === 'none' || waitingOnOwner(m, ctx)) view.shown.push(m);
		else if (mode === 'muted') view.muted.push(m);
		else {
			const g = view.digests.find((d) => d.member_id === m.from);
			if (g) g.messages.push(m);
			else view.digests.push({ member_id: m.from, messages: [m] });
		}
	}
	return view;
}

export function foldedWords(s: ChatState, ctx: ChatContext, rootId: string): string | undefined {
	const folded = repliesTo(s, rootId).filter((m) => m.folded);
	if (!folded.length) return undefined;
	return `${nameOf(ctx, folded[0].from)} posted ${folded.length} more ${folded.length === 1 ? 'update' : 'updates'}`;
}

export function typingWords(
	typing: Typing[],
	ctx: ChatContext,
	conversationId: string,
	now: string
): string | undefined {
	const names = typing
		.filter(
			(t) =>
				t.conversation_id === conversationId && ms(t.until) > ms(now) && t.member_id !== ctx.owner
		)
		.map((t) => nameOf(ctx, t.member_id));
	if (!names.length) return undefined;
	return names.length === 1
		? `${names[0]} is typing…`
		: names.length === 2
			? `${names[0]} and ${names[1]} are typing…`
			: `${names.length} people are typing…`;
}

export function clockTime(iso: string): string {
	return new Date(iso).toLocaleTimeString([], {
		hour: '2-digit',
		minute: '2-digit',
		hour12: false
	});
}

export function dayLabel(iso: string, now: string): string {
	const day = (t: number) => new Date(t).toDateString();
	const t = ms(iso);
	if (day(t) === day(ms(now))) return 'Today';
	if (day(t) === day(ms(now) - 86_400_000)) return 'Yesterday';
	return new Date(t).toLocaleDateString([], { weekday: 'short', month: 'short', day: 'numeric' });
}

export type ChatView =
	| { type: 'needs' }
	| { type: 'mentions' }
	| { type: 'threads' }
	| { type: 'search' }
	| { type: 'conversation'; id: string };
