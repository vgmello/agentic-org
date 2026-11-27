// Search and Chat <-> Activity links (CHAT-13, CHAT-15).
import { canRead, mentionsIn, slug, type ChatContext } from './engine';
import { plain } from './markdown';
import { foldText } from './model-composer';
import { ms, muteOf, nameOf, titleOf } from './model-sidebar';
import type { ChatMessage, ChatState, Conversation, MessageKind } from './types';

export interface SearchQuery {
	text: string;
	from?: string;
	in?: string;
	member?: string;
	/** `ask`, `announcement`, `handoff`, `reply`, `thread` or `attachment`. */
	has?: string;
	kind?: MessageKind;
	before?: string;
	after?: string;
}

export function parseSearch(q: string): SearchQuery {
	const out: SearchQuery = { text: '' };
	const rest: string[] = [];
	for (const word of q.trim().split(/\s+/).filter(Boolean)) {
		const m = /^(from|in|member|has|kind|before|after):(.*)$/i.exec(word);
		if (!m || !m[2]) {
			rest.push(word);
			continue;
		}
		const key = m[1].toLowerCase();
		const value = m[2].replace(/^[@#]/, '').toLowerCase();
		if (key === 'kind') out.kind = value === 'ask' ? 'request' : (value as MessageKind);
		else (out as unknown as Record<string, string>)[key] = value;
	}
	out.text = rest.join(' ');
	return out;
}

function matchesHasFilter(s: ChatState, m: ChatMessage, has: string): boolean {
	if (has === 'ask') return m.kind === 'request';
	if (has === 'thread') return s.messages.some((r) => r.thread_root_id === m.id);
	if (has === 'attachment') return !!m.attachments?.length;
	return m.kind === has;
}

function matchesConversationFilter(
	c: Conversation,
	ctx: ChatContext,
	inQuery: string | undefined
): boolean {
	if (!inQuery) return true;
	return c.kind === 'channel'
		? c.name === inQuery
		: foldText(titleOf(c, ctx, ctx.actor)).includes(inQuery);
}

function matchesMemberFilter(
	m: ChatMessage,
	ctx: ChatContext,
	memberQuery: string | undefined,
	memberId: string | undefined
): boolean {
	if (!memberQuery) return true;
	return Boolean(
		memberId &&
		(m.from === memberId ||
			m.addressed_to === memberId ||
			mentionsIn(m.body, ctx).includes(memberId))
	);
}

function matchesTimeWindow(
	createdAt: string,
	before: number | undefined,
	after: number | undefined
): boolean {
	if (before !== undefined && !(ms(createdAt) < before)) return false;
	if (after !== undefined && !(ms(createdAt) >= after)) return false;
	return true;
}

export function searchMessages(s: ChatState, ctx: ChatContext, q: SearchQuery): ChatMessage[] {
	const handleToId = (h: string) =>
		ctx.members.find((m) => m.handle.toLowerCase() === h || slug(m.name) === h)?.id;
	const from = q.from ? handleToId(q.from) : undefined;
	const member = q.member ? handleToId(q.member) : undefined;
	const text = foldText(q.text);
	const before = q.before ? Date.parse(q.before) : undefined;
	const after = q.after ? Date.parse(q.after) : undefined;
	return s.messages
		.filter((m) => {
			const c = s.conversations.find((x) => x.id === m.conversation_id);
			if (!c || !canRead(ctx, ctx.actor, c)) return false;
			if (q.from && m.from !== from) return false;
			if (!matchesConversationFilter(c, ctx, q.in)) return false;
			if (!matchesMemberFilter(m, ctx, q.member, member)) return false;
			if (q.kind && m.kind !== q.kind) return false;
			if (q.has && !matchesHasFilter(s, m, q.has)) return false;
			if (!matchesTimeWindow(m.created_at, before, after)) return false;
			return text === '' || foldText(plain(m.body)).includes(text);
		})
		.sort((a, b) => ms(b.created_at) - ms(a.created_at));
}

export const chatHref = (m: Pick<ChatMessage, 'id' | 'conversation_id'>): string =>
	`/chat?c=${encodeURIComponent(m.conversation_id)}&m=${encodeURIComponent(m.id)}`;

export function parseChatLocation(search: string): { conversation?: string; message?: string } {
	const p = new URLSearchParams(search);
	return { conversation: p.get('c') ?? undefined, message: p.get('m') ?? undefined };
}

export function activityRows(s: ChatState, ctx: ChatContext) {
	return [...s.messages]
		.sort((a, b) => ms(b.created_at) - ms(a.created_at))
		.map((m) => ({
			message: m,
			who: nameOf(ctx, m.from),
			href: chatHref(m),
			muted: muteOf(s, m.conversation_id, m.from) !== 'none'
		}));
}

export function locate(
	s: ChatState,
	loc: { conversation?: string; message?: string }
): { conversation?: Conversation; thread?: ChatMessage; message?: ChatMessage } {
	const message = loc.message ? s.messages.find((m) => m.id === loc.message) : undefined;
	const conversation = s.conversations.find(
		(c) => c.id === (message?.conversation_id ?? loc.conversation)
	);
	const thread = message?.thread_root_id
		? s.messages.find((m) => m.id === message.thread_root_id)
		: undefined;
	return { conversation, thread, message };
}
