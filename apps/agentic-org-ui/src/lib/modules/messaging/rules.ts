// The chat rules the office enforces (ADR-0003 section 6, amended 2026-11-08: chat, proposed), as pure functions over a `ChatState`.
// The mock office runs them; the real one will serve the same calls. Nothing here touches the clock, the network or storage.
import {
	type AskOption,
	type BridgeBinding,
	type ChatErrorCode,
	type ChatMessage,
	type ChatRefusal,
	type ChatState,
	type Conversation
} from './types';
import { answerText, optionsOf, questionsOf, SYSTEM_NO_REPLY } from './questions';

export { answerText, optionsOf, questionsOf, SYSTEM_NO_REPLY };

/** A member as chat needs to know them. */
export interface ChatMember {
	id: string;
	kind: 'agent' | 'person';
	name: string;
	/** What `@` completes to: an agent's messaging name, otherwise a slug of the name. */
	handle: string;
	owner?: boolean;
	/** Offboarded: kept as a record, not a recipient. */
	former?: boolean;
}

/** What the office knows about the organization when it applies a call. */
export interface ChatContext {
	now: string;
	/** The bound member making the call. */
	actor: string;
	/**
	 * `app`: the app's own connection, a person acting through a grant. `agent`: an agent's own connection. `bridge`: a bridge binding (`bridge:<id>`, with a
	 * bridge token), acting as a mapped person in the channels chosen for it; it never holds a person's grant (decision 11).
	 */
	surface: 'app' | 'agent' | 'bridge';
	owner: string;
	members: ChatMember[];
	teams: { id: string; name: string; member_ids: string[] }[];
	projects: { id: string; key?: string; name: string; team_ids: string[] }[];
	/** How long an agent shows as typing after it was woken. */
	typing_ms?: number;
	/** What a `bridge` call is bound as. */
	bridge?: BridgeBinding;
	newId?: (prefix: string) => string;
}

/** An approval is an ask with exactly these two answers; rejecting asks for a reason. */
export const APPROVAL_OPTIONS: AskOption[] = [
	{ id: 'approve', label: 'Approve' },
	{ id: 'reject', label: 'Reject…' }
];

/** The reserved sender of what the office itself says (ADR-0003: `"office"` is never a member). */
export const OFFICE = 'office';

export const emptyChat = (): ChatState => ({
	seq: 0,
	conversations: [],
	messages: [],
	mutes: [],
	read_at: {},
	presence: {},
	typing: [],
	applied: {}
});

export const refuse = (code: ChatErrorCode, message: string): never => {
	throw { source: 'api', code, message } satisfies ChatRefusal;
};


export const slug = (text: string): string =>
	text
		.normalize('NFKD')
		.replace(/[̀-ͯ]/g, '')
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-+|-+$/g, '')
		.slice(0, 40);

/** A channel name as typed: lowercase, spaces become dashes. Empty when nothing usable is left. */
export const channelName = (typed: string): string => slug(typed.replace(/^#/, ''));

export const memberOf = (ctx: ChatContext, id: string | undefined): ChatMember | undefined =>
	id === undefined ? undefined : ctx.members.find((m) => m.id === id);
export const isAgent = (ctx: ChatContext, id: string) => memberOf(ctx, id)?.kind === 'agent';

/** The handles in a message, as member ids (`@here` is not a member: it reaches the channel's members). */
export function mentionsIn(body: string, ctx: ChatContext): string[] {
	const found = new Set<string>();
	for (const m of body.matchAll(/(^|[^\w@])@([a-z0-9][a-z0-9_-]*)/gi)) {
		const handle = m[2].toLowerCase();
		const who = ctx.members.find((x) => x.handle.toLowerCase() === handle);
		if (who) found.add(who.id);
	}
	return [...found];
}

/** `@here` reaches every member of the channel. */
export const mentionsHere = (body: string): boolean => /(^|[^\w@])@here\b/i.test(body);

// ---- the channels the organization keeps ------------------------------------------------------------

/**
 * #general, one channel per project and one per team, created and kept in step with the organization (CHAT-2): members
 * follow the team or project, and the owner (who is in no team) is in every one. A team or project that is gone archives its channel. People's channels are left alone.
 */
export function syncAuto(state: ChatState, ctx: ChatContext): ChatState {
	const s = structuredClone(state);
	const live = ctx.members.filter((m) => !m.former).map((m) => m.id);
	const wanted: {
		key: string;
		auto: NonNullable<Conversation['auto']>;
		name: string;
		members: string[];
	}[] = [
		{ key: 'general', auto: { type: 'general' }, name: 'general', members: live },
		...ctx.projects.map((p) => ({
			key: `project:${p.id}`,
			auto: { type: 'project', project_id: p.id } as const,
			name: slug(p.name) || slug(p.key ?? ''),
			members: unique([
				ctx.owner,
				...p.team_ids.flatMap((t) => ctx.teams.find((x) => x.id === t)?.member_ids ?? [])
			]).filter((id) => live.includes(id))
		})),
		...ctx.teams.map((t) => ({
			key: `team:${t.id}`,
			auto: { type: 'team', team_id: t.id } as const,
			name: slug(t.name),
			members: unique([ctx.owner, ...t.member_ids]).filter((id) => live.includes(id))
		}))
	];
	const keyOf = (c: Conversation) =>
		c.auto
			? c.auto.type === 'general'
				? 'general'
				: c.auto.type === 'project'
					? `project:${c.auto.project_id}`
					: `team:${c.auto.team_id}`
			: undefined;
	for (const w of wanted) {
		let c = s.conversations.find((x) => keyOf(x) === w.key);
		if (!c) {
			s.seq += 1;
			c = {
				id: ctx.newId?.('c') ?? `c-${w.key.replace(':', '-')}`,
				kind: 'channel',
				members: [],
				auto: w.auto,
				created_at: ctx.now
			};
			s.conversations.push(c);
		}
		c.archived = undefined;
		c.members = w.members;
		// A name another channel already holds gets the kind of channel as a suffix, so names stay unique.
		const taken = (n: string) =>
			s.conversations.some((x) => x !== c && x.kind === 'channel' && x.name === n);
		let name = w.name || 'channel';
		if (taken(name)) {
			const base = `${name}-${w.auto.type}`;
			name = base;
			for (let n = 2; taken(name); n++) name = `${base}-${n}`;
		}
		c.name = name;
	}
	for (const c of s.conversations) {
		const key = keyOf(c);
		if (key && !wanted.some((w) => w.key === key)) c.archived = true;
	}
	return s;
}

const unique = <T>(xs: T[]): T[] => [...new Set(xs)];

// ---- reading ---------------------------------------------------------------------------------------

export const isMember = (c: Conversation, id: string) => c.members.includes(id);

/**
 * Whether `viewer` may read a conversation (CHAT-12, CHAT-16): the owner reads every one; anyone else only while a member.
 * A thread is never a conversation of its own: it is read through its parent, so the same check covers it.
 */
/** A bridge reaches only the channels chosen for it: never a DM or a group DM (ADR-0003 6.6). */
export const bridgeReaches = (ctx: ChatContext, c: Conversation): boolean =>
	c.kind === 'channel' && !!ctx.bridge?.channels.includes(c.id);

export const canRead = (ctx: ChatContext, viewer: string, c: Conversation): boolean => {
	if (ctx.surface === 'bridge') return viewer !== ctx.owner && bridgeReaches(ctx, c);
	return (viewer === ctx.owner && ctx.surface === 'app') || isMember(c, viewer);
};

/** The messages of a conversation, or of one thread of it, for `viewer`. Refuses when they may not read it. */
export function messagesIn(
	state: ChatState,
	ctx: ChatContext,
	viewer: string,
	conversationId: string,
	threadRootId?: string
): ChatMessage[] {
	const c = state.conversations.find((x) => x.id === conversationId);
	if (!c) return refuse('not_found', 'There is no such conversation.');
	if (!canRead(ctx, viewer, c))
		return refuse(
			'not_in_conversation',
			`You can read ${label(c)} only while you are a member of it.`
		);
	const all = state.messages.filter((m) => m.conversation_id === conversationId);
	if (threadRootId !== undefined) {
		const root = all.find((m) => m.id === threadRootId);
		if (!root) return refuse('not_found', 'There is no such thread.');
		return all.filter((m) => m.id === threadRootId || m.thread_root_id === threadRootId);
	}
	// The conversation shows its top level; a thread reply shows here only when it was also sent to the channel.
	return all.filter((m) => m.thread_root_id === undefined || m.also_in_channel);
}

/** Everything `viewer` can see across conversations (their feed): only conversations they may read, never a thread on its own. */
export const feedFor = (state: ChatState, ctx: ChatContext, viewer: string): ChatMessage[] =>
	state.messages.filter((m) => {
		const c = state.conversations.find((x) => x.id === m.conversation_id);
		return !!c && canRead(ctx, viewer, c);
	});

export const label = (c: Conversation) =>
	c.kind === 'channel' ? `#${c.name}` : 'this conversation';
