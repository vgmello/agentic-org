// What Chat shows and decides, as pure functions over a `ChatState` (docs/specs/chat-ux.md; CHAT-1..17).
import type { OrgSnapshot } from '$lib/proto';
import { isFormer } from '$lib/org/model';
import { span } from '$lib/work/time';
import { canRead, isMember, memberOf, mentionsHere, mentionsIn, slug, type ChatContext, type ChatMember } from './engine';
import { plain } from './markdown';
import { CHAT_LIMITS, type ChatMessage, type ChatState, type Conversation, type MessageKind, type Presence, type Typing } from './types';

const ms = (iso: string) => Date.parse(iso);

// ---- the organization, as chat needs it -------------------------------------------------------------

/** Handles: an agent's messaging name, otherwise a slug of the name. The owner is `owner`. */
export function chatMembers(s: OrgSnapshot): ChatMember[] {
	return s.members
		.filter((m) => m.kind === 'agent' || m.kind === 'person')
		.map((m) => ({
			id: m.id,
			kind: m.kind as 'agent' | 'person',
			name: m.name,
			handle: m.owner ? 'owner' : m.kind === 'agent' && m.agent?.messaging_name ? m.agent.messaging_name : slug(m.name) || m.id,
			...(m.owner ? { owner: true } : {}),
			...(isFormer(m) ? { former: true } : {})
		}));
}

export function chatContext(s: OrgSnapshot, init: { now: string; actor?: string; surface?: ChatContext['surface']; newId?: ChatContext['newId']; typing_ms?: number }): ChatContext {
	const owner = s.members.find((m) => m.owner)?.id ?? 'owner';
	return {
		now: init.now,
		actor: init.actor ?? owner,
		surface: init.surface ?? 'app',
		owner,
		members: chatMembers(s),
		teams: s.teams.map((t) => ({ id: t.id, name: t.name, member_ids: s.memberships.filter((m) => m.team_id === t.id).map((m) => m.member_id) })),
		projects: (s.projects ?? []).map((p) => ({ id: p.id, key: p.key, name: p.name, team_ids: (s.project_teams ?? []).filter((l) => l.project_id === p.id).map((l) => l.team_id) })),
		...(init.newId ? { newId: init.newId } : {}),
		...(init.typing_ms !== undefined ? { typing_ms: init.typing_ms } : {})
	};
}

// ---- naming ----------------------------------------------------------------------------------------

export const nameOf = (ctx: ChatContext, id: string): string => (id === 'default' ? 'Default' : (memberOf(ctx, id)?.name ?? 'Someone'));

/** "#agentic-org", a person's or agent's name, or the group's names (CHAT-3). */
export function titleOf(c: Conversation, ctx: ChatContext, viewer: string): string {
	if (c.kind === 'channel') return `#${c.name}`;
	const others = c.members.filter((id) => id !== viewer);
	return (others.length ? others : c.members).map((id) => nameOf(ctx, id)).join(', ');
}

export const PRESENCE_WORD: Record<Presence, string> = { working: 'Working', waiting: 'Waiting', asleep: 'Asleep', offline: 'Offline', paused: 'Paused', online: 'Online' };

export const KIND_WORD: Record<MessageKind, string> = { msg: 'Message', request: 'Ask', handoff: 'Hand-off', announcement: 'Announcement', reply: 'Reply', unknown: 'Message' };
export const KIND_MARK: Partial<Record<MessageKind, string>> = { request: '❓', announcement: '📣', handoff: '🔁' };

// ---- the sidebar (CHAT-1) --------------------------------------------------------------------------

/** The owner's mute or digest setting for an agent here. */
export const muteOf = (s: ChatState, conversationId: string, from: string) => s.mutes.find((x) => x.conversation_id === conversationId && x.member_id === from)?.mode ?? 'none';

/** An ask or hand-off for the owner that has not been answered or accepted: always in Needs you, whatever is muted. */
export const waitingOnOwner = (m: ChatMessage, ctx: ChatContext): boolean => m.addressed_to === ctx.owner && ((m.kind === 'request' && !m.answer) || (m.kind === 'handoff' && !m.accepted_by));

/** Asks and hand-offs waiting for the owner, newest first (the **Needs you** group). */
export const needsYou = (s: ChatState, ctx: ChatContext): ChatMessage[] => s.messages.filter((m) => waitingOnOwner(m, ctx)).sort((a, b) => ms(b.created_at) - ms(a.created_at));

const mentionsOwner = (m: ChatMessage, ctx: ChatContext, c: Conversation) => m.from !== ctx.owner && (mentionsIn(m.body, ctx).includes(ctx.owner) || (mentionsHere(m.body) && c.kind === 'channel' && isMember(c, ctx.owner)));

/** Messages that mention the owner, newest first (the **Mentions** group). */
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

/** The threads in a conversation, by root id. */
export const repliesTo = (s: ChatState, rootId: string): ChatMessage[] => s.messages.filter((m) => m.thread_root_id === rootId).sort((a, b) => ms(a.created_at) - ms(b.created_at));

/** Threads the owner is in (started, replied in, or was mentioned in) with the newest activity first (the **Threads** group). */
export function threadsOf(s: ChatState, ctx: ChatContext): ThreadSummary[] {
	const out: ThreadSummary[] = [];
	for (const root of s.messages.filter((m) => m.thread_root_id === undefined)) {
		const replies = repliesTo(s, root.id);
		if (!replies.length) continue;
		const c = s.conversations.find((x) => x.id === root.conversation_id);
		if (!c || !isMember(c, ctx.owner)) continue;
		const inIt = root.from === ctx.owner || replies.some((r) => r.from === ctx.owner || mentionsIn(r.body, ctx).includes(ctx.owner)) || mentionsIn(root.body, ctx).includes(ctx.owner);
		if (!inIt) continue;
		const readAt = ms(s.read_at[c.id] ?? '1970-01-01T00:00:00Z');
		out.push({ root, conversation: c, replies: replies.length, last_at: replies[replies.length - 1].created_at, new_replies: replies.filter((r) => r.from !== ctx.owner && ms(r.created_at) > readAt && muteOf(s, c.id, r.from) === 'none').length });
	}
	return out.sort((a, b) => ms(b.last_at) - ms(a.last_at));
}

export interface ConversationRow {
	conversation: Conversation;
	title: string;
	/** Unread messages from others, leaving out what is muted or in a digest. */
	unread: number;
	/** Unread messages that mention the owner. */
	mentions: number;
	/** The other member of a DM, for the status dot. */
	other?: ChatMember;
	presence?: Presence;
	/** The owner reads this as the owner, not as a member (CHAT-12). */
	readingAsOwner: boolean;
}

export function unreadIn(s: ChatState, ctx: ChatContext, c: Conversation): { unread: number; mentions: number } {
	const readAt = ms(s.read_at[c.id] ?? '1970-01-01T00:00:00Z');
	const fresh = s.messages.filter((m) => m.conversation_id === c.id && m.from !== ctx.owner && ms(m.created_at) > readAt && (muteOf(s, c.id, m.from) === 'none' || waitingOnOwner(m, ctx)));
	return { unread: fresh.length, mentions: fresh.filter((m) => mentionsOwner(m, ctx, c)).length };
}

export interface Sidebar {
	needsYou: ChatMessage[];
	mentions: ChatMessage[];
	threads: ThreadSummary[];
	channels: ConversationRow[];
	dms: ConversationRow[];
	/** Conversations the owner is not in but may read (CHAT-12). */
	asOwner: ConversationRow[];
}

const rowOf = (s: ChatState, ctx: ChatContext, c: Conversation): ConversationRow => {
	const other = c.kind === 'dm' ? memberOf(ctx, c.members.find((id) => id !== ctx.owner)) : undefined;
	return { conversation: c, title: titleOf(c, ctx, ctx.owner), ...unreadIn(s, ctx, c), ...(other ? { other } : {}), ...(other ? { presence: s.presence[other.id] ?? (other.kind === 'agent' ? 'offline' : 'online') } : {}), readingAsOwner: !isMember(c, ctx.owner) };
};

const lastAt = (s: ChatState, c: Conversation) => s.messages.filter((m) => m.conversation_id === c.id).reduce((t, m) => Math.max(t, ms(m.created_at)), ms(c.created_at));

export function sidebar(s: ChatState, ctx: ChatContext): Sidebar {
	const live = s.conversations.filter((c) => !c.archived && canRead(ctx, ctx.owner, c));
	const mine = live.filter((c) => isMember(c, ctx.owner));
	const byName = (a: ConversationRow, b: ConversationRow) => a.title.localeCompare(b.title, undefined, { sensitivity: 'base' });
	return {
		needsYou: needsYou(s, ctx),
		mentions: mentionsOf(s, ctx),
		threads: threadsOf(s, ctx),
		channels: mine.filter((c) => c.kind === 'channel').map((c) => rowOf(s, ctx, c)).sort(byName),
		// Direct messages by latest activity, like Slack.
		dms: mine.filter((c) => c.kind !== 'channel').sort((a, b) => lastAt(s, b) - lastAt(s, a)).map((c) => rowOf(s, ctx, c)),
		asOwner: live.filter((c) => !isMember(c, ctx.owner)).map((c) => rowOf(s, ctx, c)).sort(byName)
	};
}

// ---- a conversation (CHAT-8, CHAT-11) --------------------------------------------------------------

export interface DigestGroup {
	member_id: string;
	messages: ChatMessage[];
}

export interface ConversationView {
	/** What is drawn, oldest first. */
	shown: ChatMessage[];
	/** Hidden because the owner muted that agent here. They still reach Activity. */
	muted: ChatMessage[];
	/** Collapsed into one daily digest row per agent. */
	digests: DigestGroup[];
}

/**
 * The top level of a conversation as the owner sees it: messages from a muted agent are held back, from a digest agent are
 * gathered into a digest, and everything else is shown. An ask or hand-off waiting for the owner is never held back.
 */
export function conversationView(s: ChatState, ctx: ChatContext, conversationId: string): ConversationView {
	const all = s.messages
		.filter((m) => m.conversation_id === conversationId && (m.thread_root_id === undefined || m.also_in_channel))
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

/** "server-dev posted 3 more updates": the extra announcements folded into a thread (CHAT-10). */
export function foldedWords(s: ChatState, ctx: ChatContext, rootId: string): string | undefined {
	const folded = repliesTo(s, rootId).filter((m) => m.folded);
	if (!folded.length) return undefined;
	return `${nameOf(ctx, folded[0].from)} posted ${folded.length} more ${folded.length === 1 ? 'update' : 'updates'}`;
}

/** "HR is typing…", "Sam and Noor are typing…". */
export function typingWords(typing: Typing[], ctx: ChatContext, conversationId: string, now: string): string | undefined {
	const names = typing.filter((t) => t.conversation_id === conversationId && ms(t.until) > ms(now) && t.member_id !== ctx.owner).map((t) => nameOf(ctx, t.member_id));
	if (!names.length) return undefined;
	return names.length === 1 ? `${names[0]} is typing…` : names.length === 2 ? `${names[0]} and ${names[1]} are typing…` : `${names.length} people are typing…`;
}

// ---- the composer (CHAT-7, states) -----------------------------------------------------------------

export interface ComposerState {
	/** Whether the owner can write here. */
	canWrite: boolean;
	/** The line under or instead of the box. */
	note?: string;
	/** Agents this message would wake. */
	wakes: string[];
}

export function composerState(s: ChatState, ctx: ChatContext, c: Conversation, officeRunning: boolean): ComposerState {
	if (!officeRunning) return { canWrite: false, note: "The office isn't running. Restart it from Offices.", wakes: [] };
	if (c.archived) return { canWrite: false, note: `${c.kind === 'channel' ? `#${c.name}` : 'This conversation'} is archived.`, wakes: [] };
	const gone = c.kind === 'dm' ? c.members.map((id) => memberOf(ctx, id)).find((m) => m?.former) : undefined;
	if (gone) return { canWrite: false, note: `${gone.name} was offboarded.`, wakes: [] };
	const others = c.kind === 'dm' ? c.members.filter((id) => id !== ctx.owner) : [];
	const asleep = others.filter((id) => s.presence[id] === 'asleep');
	if (asleep.length) return { canWrite: true, note: `${asleep.map((id) => nameOf(ctx, id)).join(' and ')} ${asleep.length === 1 ? 'is' : 'are'} asleep. Your message will wake ${asleep.length === 1 ? 'it' : 'them'}.`, wakes: asleep };
	const paused = others.filter((id) => s.presence[id] === 'paused');
	if (paused.length) return { canWrite: true, note: `${paused.map((id) => nameOf(ctx, id)).join(' and ')} ${paused.length === 1 ? 'is' : 'are'} paused and will not read this until resumed.`, wakes: [] };
	return { canWrite: true, wakes: [] };
}

/** The wake a typed message would cause, for the line under the box while typing a mention in a channel. */
export function wakesFor(s: ChatState, ctx: ChatContext, c: Conversation, body: string): string[] {
	if (c.kind === 'dm') return c.members.filter((id) => id !== ctx.owner && s.presence[id] === 'asleep');
	return mentionsIn(body, ctx).filter((id) => memberOf(ctx, id)?.kind === 'agent' && s.presence[id] === 'asleep');
}

// ---- mentions ---------------------------------------------------------------------------------------

export interface MentionQuery {
	/** Where the `@` is. */
	start: number;
	query: string;
}

/** The `@word` being typed at the caret, if any. */
export function mentionAt(text: string, caret: number): MentionQuery | undefined {
	const m = /(^|[^\w@])@([\w-]*)$/.exec(text.slice(0, caret));
	return m ? { start: caret - m[2].length - 1, query: m[2] } : undefined;
}

export interface MentionOption {
	handle: string;
	name: string;
	kind: 'agent' | 'person' | 'here';
}

const fold = (t: string) => t.normalize('NFKD').replace(/[̀-ͯ]/g, '').toLowerCase();

/** Members typed by display name or handle, completed to the handle; `@here` for a channel (CHAT-14). */
export function mentionOptions(ctx: ChatContext, c: Conversation, query: string): MentionOption[] {
	const q = fold(query);
	const pool = c.kind === 'channel' || c.kind === 'group_dm' ? c.members : ctx.members.map((m) => m.id);
	const members = pool.map((id) => memberOf(ctx, id)).filter((m): m is ChatMember => !!m && !m.former && !m.owner);
	const hits = members.filter((m) => q === '' || fold(m.name).split(/\s+/).some((w) => w.startsWith(q)) || fold(m.handle).startsWith(q) || fold(m.name).startsWith(q));
	const out: MentionOption[] = hits.sort((a, b) => a.name.localeCompare(b.name)).map((m) => ({ handle: m.handle, name: m.name, kind: m.kind }));
	if (c.kind === 'channel' && 'here'.startsWith(q)) out.unshift({ handle: 'here', name: "Everyone in this channel's members", kind: 'here' });
	return out.slice(0, 8);
}

export function applyMention(text: string, at: MentionQuery, caret: number, handle: string): { text: string; caret: number } {
	const inserted = `@${handle} `;
	return { text: text.slice(0, at.start) + inserted + text.slice(caret), caret: at.start + inserted.length };
}

// ---- slash commands (CHAT-9) -----------------------------------------------------------------------

export type SlashName = 'bug' | 'request' | 'task' | 'status' | 'wake' | 'remind';
export const SLASH_COMMANDS: { name: SlashName; usage: string; help: string }[] = [
	{ name: 'bug', usage: '/bug [what happened]', help: 'Report a bug, prefilled from your text' },
	{ name: 'request', usage: '/request [what you need]', help: 'Send a feature request' },
	{ name: 'task', usage: '/task @agent [title]', help: 'Assign a task' },
	{ name: 'status', usage: '/status [project]', help: 'Post the live status card' },
	{ name: 'wake', usage: '/wake @agent', help: 'Run an agent now' },
	{ name: 'remind', usage: '/remind [in 30m] [what]', help: 'Remind you about this conversation' }
];

export type Slash = { ok: true; name: SlashName; args: string; handle?: string; rest: string } | { ok: false; message: string };

/** A message that starts with `/` is a command for people. `undefined` when it is not one. */
export function parseSlash(text: string): Slash | undefined {
	const m = /^\/([a-z]+)(?:\s+([\s\S]*))?$/.exec(text.trimStart());
	if (!m) return undefined;
	const cmd = SLASH_COMMANDS.find((c) => c.name === m[1]);
	if (!cmd) return { ok: false, message: `There is no /${m[1]} command. Try ${SLASH_COMMANDS.map((c) => '/' + c.name).join(', ')}.` };
	const args = (m[2] ?? '').trim();
	const who = /^@([\w-]+)\s*([\s\S]*)$/.exec(args);
	if ((cmd.name === 'task' || cmd.name === 'wake') && !who) return { ok: false, message: `Say which agent: ${cmd.usage}` };
	if (cmd.name === 'task' && !who![2].trim()) return { ok: false, message: 'Give the task a title: /task @agent [title]' };
	return { ok: true, name: cmd.name, args, ...(who ? { handle: who[1].toLowerCase(), rest: who[2].trim() } : { rest: args }) };
}

/** Matches for a half-typed command: `/b` lists `/bug`. */
export const slashSuggestions = (text: string) => {
	const m = /^\/([a-z]*)$/.exec(text);
	return m ? SLASH_COMMANDS.filter((c) => c.name.startsWith(m[1])) : [];
};

/** "in 30m", "in 2h", "in 1d" to milliseconds. */
export function remindDelay(text: string): { ms: number; what: string } | undefined {
	const m = /^in\s+(\d+)\s*(m|h|d)\b\s*([\s\S]*)$/i.exec(text.trim());
	if (!m) return undefined;
	const n = Number(m[1]);
	return { ms: n * { m: 60_000, h: 3_600_000, d: 86_400_000 }[m[2].toLowerCase() as 'm'], what: m[3].trim() };
}

/** Turn into… (CHAT-9): the same forms as the slash commands, filled from an existing message. */
export function turnInto(m: ChatMessage, into: 'bug' | 'request' | 'task'): { title: string; body: string } {
	const text = plain(m.body);
	const first = text.split(/(?<=[.?!])\s/)[0] ?? text;
	const title = (first.length > 80 ? first.slice(0, 79).trimEnd() + '…' : first).replace(/^(@[\w-]+\s*)+/, '').trim();
	return { title: title || (into === 'bug' ? 'Bug from chat' : into === 'task' ? 'Task from chat' : 'Request from chat'), body: m.body };
}

// ---- asks (CHAT-5, CHAT-6) -------------------------------------------------------------------------

export const waitingFor = (m: ChatMessage, now: string): string => `Waiting ${span(Math.max(0, ms(now) - ms(m.created_at)))}`;

export interface DeadlineWords {
	text: string;
	/** The default was applied. */
	applied: boolean;
}

/** "If no answer by 18:00, I'll allow it" before the deadline; "Default applied: …" after (CHAT-6). */
export function deadlineWords(m: ChatMessage, now: string, clock: (iso: string) => string = clockTime): DeadlineWords | undefined {
	if (m.kind !== 'request' || !m.default_option || !m.default_at) return undefined;
	const label = m.options?.find((o) => o.id === m.default_option)?.label ?? m.default_option;
	if (m.answer?.by === 'default') return { text: `No answer by ${clock(m.default_at)}: the default was applied (${label}).`, applied: true };
	if (m.answer) return undefined;
	if (ms(m.default_at) <= ms(now)) return { text: `The deadline passed: the default (${label}) is being applied.`, applied: false };
	return { text: `If no answer by ${clock(m.default_at)}, the default applies: ${label}. (${span(ms(m.default_at) - ms(now))} left)`, applied: false };
}

export function answerWords(m: ChatMessage, ctx: ChatContext): string | undefined {
	if (!m.answer) return undefined;
	const label = m.options?.find((o) => o.id === m.answer!.option_id)?.label.replace(/…$/, '');
	const by = m.answer.by === 'default' ? 'the default' : nameOf(ctx, m.answer.by);
	return `Answered by ${by}${label ? `: ${label}` : ''}${m.answer.text ? `${label ? ' · ' : ': '}${m.answer.text}` : ''}`;
}

/** Whether the app (and nothing else) may answer this: asks to the owner and every approval (CHAT-17). */
export const appOnly = (m: ChatMessage, ctx: ChatContext): boolean => m.kind === 'request' && (m.addressed_to === ctx.owner || !!m.approval);

// ---- long text (CHAT-14) ---------------------------------------------------------------------------

export function collapse(body: string, limit: number = CHAT_LIMITS.collapseLines): { shown: string; hidden: number } {
	const lines = body.split('\n');
	return lines.length <= limit ? { shown: body, hidden: 0 } : { shown: lines.slice(0, limit).join('\n'), hidden: lines.length - limit };
}

// ---- search (CHAT-13) ------------------------------------------------------------------------------

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

/** `from:@ada in:#agentic-org has:ask kind:announcement before:2026-11-08 after:2026-11-01 member:@sam budget`. */
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

export function searchMessages(s: ChatState, ctx: ChatContext, q: SearchQuery): ChatMessage[] {
	const handleToId = (h: string) => ctx.members.find((m) => m.handle.toLowerCase() === h || slug(m.name) === h)?.id;
	const from = q.from ? handleToId(q.from) : undefined;
	const member = q.member ? handleToId(q.member) : undefined;
	const text = fold(q.text);
	const before = q.before ? Date.parse(q.before) : undefined;
	const after = q.after ? Date.parse(q.after) : undefined;
	return s.messages
		.filter((m) => {
			const c = s.conversations.find((x) => x.id === m.conversation_id);
			if (!c || !canRead(ctx, ctx.actor, c)) return false;
			if (q.from && m.from !== from) return false;
			if (q.in && !(c.kind === 'channel' ? c.name === q.in : fold(titleOf(c, ctx, ctx.actor)).includes(q.in))) return false;
			if (q.member && !(member && (m.from === member || m.addressed_to === member || mentionsIn(m.body, ctx).includes(member)))) return false;
			if (q.kind && m.kind !== q.kind) return false;
			if (q.has) {
				const has = q.has === 'ask' ? m.kind === 'request' : q.has === 'thread' ? s.messages.some((r) => r.thread_root_id === m.id) : q.has === 'attachment' ? !!m.attachments?.length : m.kind === q.has;
				if (!has) return false;
			}
			if (before !== undefined && !(ms(m.created_at) < before)) return false;
			if (after !== undefined && !(ms(m.created_at) >= after)) return false;
			return text === '' || fold(plain(m.body)).includes(text);
		})
		.sort((a, b) => ms(b.created_at) - ms(a.created_at));
}

// ---- links between Chat and Activity (CHAT-15) -----------------------------------------------------

/** Where "Open in Chat" on an Activity row goes. */
export const chatHref = (m: Pick<ChatMessage, 'id' | 'conversation_id'>): string => `/chat?c=${encodeURIComponent(m.conversation_id)}&m=${encodeURIComponent(m.id)}`;

export function parseChatLocation(search: string): { conversation?: string; message?: string } {
	const p = new URLSearchParams(search);
	return { conversation: p.get('c') ?? undefined, message: p.get('m') ?? undefined };
}

/** Activity shows everything that happens, muted messages included, each with Open in Chat. */
export function activityRows(s: ChatState, ctx: ChatContext) {
	return [...s.messages]
		.sort((a, b) => ms(b.created_at) - ms(a.created_at))
		.map((m) => ({ message: m, who: nameOf(ctx, m.from), href: chatHref(m), muted: muteOf(s, m.conversation_id, m.from) !== 'none' }));
}

/** The conversation, thread and message a link names; falls back to the conversation, then nothing. */
export function locate(s: ChatState, loc: { conversation?: string; message?: string }): { conversation?: Conversation; thread?: ChatMessage; message?: ChatMessage } {
	const message = loc.message ? s.messages.find((m) => m.id === loc.message) : undefined;
	const conversation = s.conversations.find((c) => c.id === (message?.conversation_id ?? loc.conversation));
	const thread = message?.thread_root_id ? s.messages.find((m) => m.id === message.thread_root_id) : undefined;
	return { conversation, thread, message };
}

// ---- times -----------------------------------------------------------------------------------------

export function clockTime(iso: string): string {
	return new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false });
}

/** "Today", "Yesterday" or the date, for the separators between days. */
export function dayLabel(iso: string, now: string): string {
	const day = (t: number) => new Date(t).toDateString();
	const t = ms(iso);
	if (day(t) === day(ms(now))) return 'Today';
	if (day(t) === day(ms(now) - 86_400_000)) return 'Yesterday';
	return new Date(t).toLocaleDateString([], { weekday: 'short', month: 'short', day: 'numeric' });
}

// ---- what the middle of the screen shows -----------------------------------------------------------

export type ChatView = { type: 'needs' } | { type: 'mentions' } | { type: 'threads' } | { type: 'search' } | { type: 'conversation'; id: string };
