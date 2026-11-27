// Test-only chat state reducer used by unit tests and e2e fakes (moved out of production bundle in TASK-227).
import {
	AGENT_KINDS,
	ANNOUNCEMENT_REASONS,
	CHAT_LIMITS,
	REACTIONS,
	type AskOption,
	type ChatAccepted,
	type ChatCall,
	type ChatMessage,
	type ChatState,
	type Conversation,
	type MuteMode,
	type QuestionAnswer
} from '../types';
import { answerText, optionsOf, questionsOf, SYSTEM_NO_REPLY, validQuestions } from '../questions';
import {
	APPROVAL_OPTIONS,
	OFFICE,
	bridgeReaches,
	canRead,
	channelName,
	emptyChat,
	feedFor,
	isAgent,
	isMember,
	label,
	memberOf,
	mentionsHere,
	mentionsIn,
	messagesIn,
	refuse,
	slug,
	syncAuto,
	type ChatContext,
	type ChatMember
} from '../rules';

export {
	APPROVAL_OPTIONS,
	OFFICE,
	SYSTEM_NO_REPLY,
	answerText,
	bridgeReaches,
	canRead,
	channelName,
	emptyChat,
	feedFor,
	isAgent,
	isMember,
	label,
	memberOf,
	mentionsHere,
	mentionsIn,
	messagesIn,
	optionsOf,
	questionsOf,
	refuse,
	slug,
	syncAuto,
	type ChatContext,
	type ChatMember
};

const ms = (iso: string) => Date.parse(iso);
const bytes = (text: string) => new TextEncoder().encode(text).length;
const APPLIED_KEPT = 500;
const unique = <T>(xs: T[]): T[] => [...new Set(xs)];

// ---- applying a call -------------------------------------------------------------------------------

const newIdOf = (s: ChatState, ctx: ChatContext, prefix: string) => {
	s.seq += 1;
	return ctx.newId?.(prefix) ?? `${prefix}-${s.seq}`;
};

const truncateLines = (body: string): string => {
	const lines = body.split('\n');
	return lines.length <= CHAT_LIMITS.collapseLines
		? body
		: [
				...lines.slice(0, CHAT_LIMITS.collapseLines),
				'… (cut: the full output is in the session log)'
			].join('\n');
};

/** Apply one call as `ctx.actor`. Returns the new state and what to tell the caller; throws a `ChatRefusal` and changes nothing. */
export function applyChat(
	state: ChatState,
	call: ChatCall,
	ctx: ChatContext
): { state: ChatState; accepted: ChatAccepted } {
	const done = state.applied[call.change_id];
	if (done) return { state, accepted: done };
	const actor = memberOf(ctx, ctx.actor);
	if (!actor || actor.former)
		return refuse('not_authorized', 'Only a current member can use chat.');
	enforceBridgePolicy(call, ctx, actor);
	const s = structuredClone(state);
	const accepted = dispatchChatCall(s, call, ctx, actor);
	s.applied[call.change_id] = accepted;
	const keys = Object.keys(s.applied);
	if (keys.length > APPLIED_KEPT)
		for (const k of keys.slice(0, keys.length - APPLIED_KEPT)) delete s.applied[k];
	return { state: s, accepted };
}

function enforceBridgePolicy(call: ChatCall, ctx: ChatContext, actor: ChatMember) {
	// A bridge has its own binding, not a person's grant: it can post as a mapped person, never as the owner (decision 11), and it
	// does three things only: post, react and answer, in the channels chosen for it.
	if (ctx.surface !== 'bridge') return;
	if (ctx.actor === ctx.owner) return refuse('not_authorized', 'A bridge never acts as the owner.');
	if (actor.kind !== 'person')
		return refuse('not_authorized', 'A bridge acts only as a mapped person, never as an agent.');
	if (!ctx.bridge?.people.some((p) => p.member_id === ctx.actor))
		return refuse('not_authorized', 'That person is not mapped to this bridge.');
	if (!['msg.send', 'chat.react', 'chat.answer'].includes(call.method))
		return refuse('not_authorized', 'A bridge cannot do this.');
}

function dispatchChatCall(
	s: ChatState,
	call: ChatCall,
	ctx: ChatContext,
	actor: ChatMember
): ChatAccepted {
	switch (call.method) {
		case 'msg.send':
			return send(s, call, ctx, actor);
		case 'chat.answer':
			return answer(s, call, ctx);
		case 'chat.withdraw':
			return withdrawAsk(s, call, ctx);
		case 'chat.hand_back':
			return handBackHandoff(s, call, ctx);
		case 'channel.join':
		case 'channel.request_invitation':
			return channelDirectoryCall(s, call, ctx);
		case 'chat.accept_handoff':
			return acceptHandoff(s, call, ctx);
		case 'chat.react':
			return toggleReaction(s, call, ctx);
		case 'chat.mute':
			return setMute(s, call, ctx);
		case 'chat.mark_read':
			return markRead(s, call, ctx);
		case 'chat.pin':
			return togglePin(s, call, ctx);
		case 'dm.open':
			return openDm(s, call.members, ctx);
		case 'msg.reply': {
			const target = s.messages.find((m) => m.id === call.to_id);
			if (!target) return refuse('not_found', 'There is no message to reply to.');
			return send(
				s,
				{
					method: 'msg.send',
					change_id: call.change_id,
					conversation_id: target.conversation_id,
					reply_to: target.id,
					kind: 'reply',
					body: call.body,
					attachment_ids: call.attachment_ids
				},
				ctx,
				actor
			);
		}
		default:
			return channelCall(s, call, ctx);
	}
}

function acceptHandoff(
	s: ChatState,
	call: Extract<ChatCall, { method: 'chat.accept_handoff' }>,
	ctx: ChatContext
): ChatAccepted {
	const m = s.messages.find((x) => x.id === call.message_id && x.kind === 'handoff');
	if (!m) return refuse('not_found', 'There is no such hand-off.');
	if (m.accepted_by)
		return refuse(
			'already_exists',
			`Already accepted by ${memberOf(ctx, m.accepted_by)?.name ?? 'someone'}.`
		);
	if (m.handed_back) return refuse('already_exists', 'Already handed back');
	// Only its recipient accepts it (the owner when it is handed to the owner); the owner reading as the owner only replies.
	if (ctx.actor !== m.addressed_to)
		return refuse('not_authorized', 'Only the person it is handed to can accept it.');
	const home = s.conversations.find((x) => x.id === m.conversation_id)!;
	if (m.addressed_to === ctx.owner && !(ctx.actor === ctx.owner && ctx.surface === 'app'))
		return refuse(
			'not_authorized',
			'Open Agentic Org to accept a hand-off addressed to the owner.'
		);
	mustBeIn(ctx, home);
	m.accepted_by = ctx.actor;
	m.accepted_at = ctx.now;
	return { id: m.id };
}

function toggleReaction(
	s: ChatState,
	call: Extract<ChatCall, { method: 'chat.react' }>,
	ctx: ChatContext
): ChatAccepted {
	const m = s.messages.find((x) => x.id === call.message_id);
	const c = m && s.conversations.find((x) => x.id === m.conversation_id);
	if (!m || !c) return refuse('not_found', 'There is no such message.');
	if (!canRead(ctx, ctx.actor, c))
		return refuse(
			'not_in_conversation',
			`You can react in ${label(c)} only while you are a member of it.`
		);
	mustBeIn(ctx, c);
	if (!REACTIONS.some((r) => r.id === call.reaction))
		return refuse('invalid_request', 'Unknown reaction.');
	const who = new Set(m.reactions?.[call.reaction] ?? []);
	if (who.has(ctx.actor)) who.delete(ctx.actor);
	else who.add(ctx.actor);
	m.reactions = { ...m.reactions, [call.reaction]: [...who] };
	return { id: m.id };
}

function setMute(
	s: ChatState,
	call: Extract<ChatCall, { method: 'chat.mute' }>,
	ctx: ChatContext
): ChatAccepted {
	ownerOnly(ctx);
	const c = s.conversations.find((x) => x.id === call.conversation_id);
	if (!c) return refuse('not_found', 'There is no such conversation.');
	if (!isAgent(ctx, call.member_id) || !isMember(c, call.member_id))
		return refuse('not_in_conversation', 'Only an agent that is a member can be muted here.');
	if (!(['none', 'muted', 'digest'] as MuteMode[]).includes(call.mode))
		return refuse('invalid_request', 'Unknown mute setting.');
	s.mutes = s.mutes.filter(
		(x) => !(x.conversation_id === call.conversation_id && x.member_id === call.member_id)
	);
	if (call.mode !== 'none')
		s.mutes.push({
			conversation_id: call.conversation_id,
			member_id: call.member_id,
			mode: call.mode
		});
	return {};
}

function markRead(
	s: ChatState,
	call: Extract<ChatCall, { method: 'chat.mark_read' }>,
	ctx: ChatContext
): ChatAccepted {
	ownerOnly(ctx);
	if (!s.conversations.some((x) => x.id === call.conversation_id))
		return refuse('not_found', 'There is no such conversation.');
	s.read_at[call.conversation_id] = ctx.now;
	return {};
}

function togglePin(
	s: ChatState,
	call: Extract<ChatCall, { method: 'chat.pin' }>,
	ctx: ChatContext
): ChatAccepted {
	const m = s.messages.find((x) => x.id === call.message_id);
	const c = m && s.conversations.find((x) => x.id === m.conversation_id);
	if (!m || !c) return refuse('not_found', 'There is no such message.');
	if (ctx.surface !== 'app') return refuse('not_authorized', 'Pinning is done in the app.');
	if (!canRead(ctx, ctx.actor, c))
		return refuse('not_in_conversation', 'Pin only in a conversation you are in.');
	m.pinned = m.pinned ? undefined : true;
	return {};
}

/** Answering or accepting happens where you are: a member (the owner in the app excepted), and a bridge only in its channels. */
function mustBeIn(ctx: ChatContext, c: Conversation) {
	if (ctx.actor === ctx.owner && ctx.surface === 'app') return;
	if (ctx.surface === 'bridge' && !bridgeReaches(ctx, c))
		refuse('not_authorized', 'A bridge reaches only the channels chosen for it.');
	if (!isMember(c, ctx.actor))
		refuse(
			'not_in_conversation',
			`You can answer in ${label(c)} only while you are a member of it.`
		);
}

/** Calls only the owner makes, in the app. */
function ownerOnly(ctx: ChatContext) {
	if (ctx.actor !== ctx.owner || ctx.surface !== 'app')
		refuse('not_authorized', 'This is done in the app, by the owner.');
}

function hasSendContent(body: string, call: Extract<ChatCall, { method: 'msg.send' }>): boolean {
	return body !== '' || Boolean(call.attachments?.length || call.attachment_ids?.length);
}

function hasAnswerContent(
	option: unknown,
	text: string | undefined,
	attachmentIds?: readonly string[]
): boolean {
	return Boolean(option || text || attachmentIds?.length);
}

function send(
	s: ChatState,
	call: Extract<ChatCall, { method: 'msg.send' }>,
	ctx: ChatContext,
	actor: ChatMember
): ChatAccepted {
	const c = s.conversations.find((x) => x.id === call.conversation_id);
	if (ctx.surface === 'bridge' && (!c || !bridgeReaches(ctx, c)))
		return refuse('not_authorized', 'A bridge reaches only the channels chosen for it.');
	if (!c) return refuse('not_found', 'There is no such conversation.');
	if (c.archived) return refuse('conversation_archived', `${label(c)} is archived.`);
	// Reading as the owner gives no right to post (chat-ux decision 5): posting needs membership.
	if (!canRead(ctx, ctx.actor, c) || !isMember(c, ctx.actor))
		return refuse(
			'not_in_conversation',
			`You can post in ${label(c)} only while you are a member of it.`
		);
	// A bridge posts as a mapped person, and only where that person is a member.
	if (ctx.surface === 'bridge' && !isMember(c, ctx.actor))
		return refuse(
			'not_in_conversation',
			`${memberOf(ctx, ctx.actor)?.name ?? 'They'} is not a member of ${label(c)}.`
		);
	if (c.kind === 'dm' && c.members.some((id) => memberOf(ctx, id)?.former))
		return refuse(
			'unavailable',
			`${
				memberOf(
					ctx,
					c.members.find((id) => memberOf(ctx, id)?.former)!
				)?.name
			} was offboarded.`
		);
	// A DM keeps every kind ADR-0003 has, custom tags included, so agents' `agent-os msg send` keeps working (section 6 is
	// additive). Channels and group DMs take the chat kinds only.
	const known = ['msg', 'request', 'handoff', 'announcement', 'reply'].includes(call.kind);
	const tagShaped =
		/^[a-z][a-z0-9_.-]{0,63}$/.test(call.kind) &&
		(call.kind as string) !== 'delivery_failed' &&
		(call.kind as string) !== 'unknown';
	if (!known && !tagShaped) return refuse('invalid_request', 'Unknown message kind.');
	// The office enforces the kinds, not only the guidance (CHAT-4): in a channel or group DM an agent posts asks, hand-offs, replies and announcements.
	if (
		actor.kind === 'agent' &&
		c.kind !== 'dm' &&
		!(AGENT_KINDS as readonly string[]).includes(call.kind)
	)
		return refuse(
			'kind_not_allowed',
			'An agent posts only an ask, a hand-off, a reply or an announcement.'
		);
	// A custom tag is for DMs; a person in a channel or group DM sends the chat kinds too.
	if (!known && c.kind !== 'dm') return refuse('invalid_request', 'Unknown message kind.');
	const body = call.body.trim();
	if (!hasSendContent(body, call)) return refuse('invalid_request', 'Write something first.');
	if (bytes(body) > CHAT_LIMITS.bodyMax)
		return refuse('invalid_request', 'That message is too long.');

	let root = call.thread_root_id;
	let replyTarget: ChatMessage | undefined;
	if (call.reply_to !== undefined) {
		replyTarget = s.messages.find((m) => m.id === call.reply_to && m.conversation_id === c.id);
		if (!replyTarget) return refuse('not_found', 'There is no message to reply to.');
		// Agent conversations reply in threads by default (CHAT-8); in a DM there is nothing to thread under.
		if (root === undefined && c.kind !== 'dm') root = replyTarget.thread_root_id ?? replyTarget.id;
	}
	if (
		actor.kind === 'agent' &&
		call.kind === 'reply' &&
		replyTarget === undefined &&
		root === undefined &&
		c.kind !== 'dm'
	)
		return refuse('invalid_request', 'A reply says which message it answers.');
	if (root !== undefined) {
		const found = s.messages.find((m) => m.id === root && m.conversation_id === c.id);
		if (!found) return refuse('not_found', 'There is no such thread.');
		root = found.thread_root_id ?? found.id;
		if (s.messages.find((m) => m.id === root)?.from === OFFICE)
			return refuse('invalid_request', SYSTEM_NO_REPLY);
	}
	if (call.also_in_channel && root === undefined)
		return refuse('invalid_request', 'Only a thread reply can also be sent to the channel.');

	const m: ChatMessage = {
		id: '',
		conversation_id: c.id,
		from: ctx.actor,
		kind: call.kind,
		body: actor.kind === 'agent' ? truncateLines(body) : body,
		created_at: ctx.now
	};
	if (root !== undefined) m.thread_root_id = root;
	if (call.also_in_channel) m.also_in_channel = true;
	if (call.attachments?.length) m.attachments = call.attachments.slice(0, 10);
	if (call.linked_ref) m.linked_ref = call.linked_ref;
	// A bridged message is posted as the bridged person (their own handle, a person's rights), never as the owner (decision 11).
	if (ctx.surface === 'bridge' && ctx.bridge) m.bridge = ctx.bridge.platform;

	let folded_into: string | undefined;
	if (call.kind === 'request' || call.kind === 'handoff') {
		const to =
			call.addressed_to ?? (c.kind === 'dm' ? c.members.find((id) => id !== ctx.actor) : undefined);
		if (to === undefined || !memberOf(ctx, to) || memberOf(ctx, to)?.former)
			return refuse(
				'invalid_request',
				`Say who the ${call.kind === 'request' ? 'ask' : 'hand-off'} is for.`
			);
		if (to === ctx.actor) return refuse('invalid_request', 'You cannot address this to yourself.');
		m.addressed_to = to;
	}
	if (call.kind === 'request') {
		applyRequestFields(m, call, ctx);
	}
	if (call.kind === 'announcement') {
		if (!call.reason || !(ANNOUNCEMENT_REASONS as readonly string[]).includes(call.reason))
			return refuse(
				'reason_required',
				'An announcement says why: delivered, ready for review, blocked, a report or a decision needed.'
			);
		m.reason = call.reason;
		// At most one announcement per agent per channel per window, wherever it was posted; the extras fold into a thread under
		// the last one (CHAT-10). The last one is the latest that was not itself folded.
		if (actor.kind === 'agent' && c.kind === 'channel') {
			const last = s.messages
				.filter(
					(x) =>
						x.conversation_id === c.id &&
						x.from === ctx.actor &&
						x.kind === 'announcement' &&
						!x.folded
				)
				.sort((a, b) => ms(b.created_at) - ms(a.created_at))[0];
			if (last && ms(ctx.now) - ms(last.created_at) < CHAT_LIMITS.announcementWindowMs) {
				const rootOfLast = last.thread_root_id ?? last.id;
				m.thread_root_id = rootOfLast;
				m.folded = true;
				delete m.also_in_channel;
				folded_into = rootOfLast;
			}
		}
	}
	m.id = newIdOf(s, ctx, 'm');
	s.messages.push(m);

	// Messaging an asleep agent wakes it, and it shows as typing while it works (CHAT-7).
	const woke: string[] = [];
	const reached =
		c.kind === 'dm'
			? c.members.filter((id) => id !== ctx.actor)
			: mentionsIn(body, ctx).filter((id) => isMember(c, id));
	for (const id of unique(reached)) {
		if (id === ctx.actor || !isAgent(ctx, id)) continue;
		if (s.presence[id] === 'asleep') {
			s.presence[id] = 'working';
			woke.push(id);
		}
		if (s.presence[id] === 'working' || s.presence[id] === 'waiting') {
			const until = new Date(ms(ctx.now) + (ctx.typing_ms ?? 4000)).toISOString();
			s.typing = s.typing.filter((t) => !(t.conversation_id === c.id && t.member_id === id));
			s.typing.push({ conversation_id: c.id, member_id: id, until });
		}
	}
	return {
		id: m.id,
		conversation_id: c.id,
		...(folded_into ? { folded_into } : {}),
		...(woke.length ? { woke } : {})
	};
}

function applyRequestChoices(
	m: ChatMessage,
	call: Extract<ChatCall, { method: 'msg.send' }>,
	ctx: ChatContext
) {
	if (call.approval) {
		if (m.addressed_to !== ctx.owner)
			return refuse('invalid_request', 'An approval is asked of the owner.');
		m.approval = true;
		m.options = APPROVAL_OPTIONS;
		return;
	}
	if (call.questions?.length) {
		m.questions = validQuestions(call.questions);
		const first = m.questions.length === 1 ? m.questions[0] : undefined;
		const singleButtons =
			first &&
			(first.question_type === 'yes_no' ||
				(first.question_type === 'choice' && !first.allow_multiple));
		if (singleButtons) m.options = optionsOf(first);
		return;
	}
	if (call.options?.length) m.options = validateAskOptions(call.options);
}

function validateAskOptions(options: AskOption[]): AskOption[] {
	if (options.length > CHAT_LIMITS.askOptionsMax)
		return refuse('invalid_request', `An ask has at most ${CHAT_LIMITS.askOptionsMax} options.`);
	const ids = new Set(options.map((o) => o.id));
	if (ids.size !== options.length || options.some((o) => !o.id || !o.label.trim()))
		return refuse('invalid_request', 'Each option needs its own id and a label.');
	return options.map((o) => ({
		id: o.id,
		label: o.label.trim(),
		...(o.description?.trim() ? { description: o.description.trim() } : {})
	}));
}

function applyRequestFields(
	m: ChatMessage,
	call: Extract<ChatCall, { method: 'msg.send' }>,
	ctx: ChatContext
) {
	applyRequestChoices(m, call, ctx);
	if (call.required) {
		if (call.default_at !== undefined)
			return refuse(
				'invalid_request',
				'A required ask has no deadline: a default never answers it.'
			);
		m.required = true;
	}
	if (call.wait_until !== undefined) {
		if (Number.isNaN(ms(call.wait_until)))
			return refuse('invalid_request', 'The end of the wait is not a time.');
		m.wait_until = call.wait_until;
	}
	applyRequestDefault(m, call);
	if (call.blocks) m.blocks = call.blocks.slice(0, 80);
}

function applyQuestionDefault(m: ChatMessage, defaultAt: string) {
	if (Number.isNaN(ms(defaultAt)))
		return refuse('invalid_request', 'A default needs the time it applies.');
	if (!m.questions?.some((q) => q.default_option))
		return refuse('invalid_request', 'A deadline needs a default on at least one question.');
	m.default_at = defaultAt;
	const single = m.questions.length === 1 ? m.questions[0].default_option : undefined;
	if (single && m.options) m.default_option = single;
}

function applyRequestDefault(m: ChatMessage, call: Extract<ChatCall, { method: 'msg.send' }>) {
	if (call.default_at !== undefined && m.questions) {
		applyQuestionDefault(m, call.default_at);
		return;
	}
	if (call.default_option === undefined && call.default_at === undefined) return;
	const missingPair = call.default_option === undefined || call.default_at === undefined;
	if (missingPair || Number.isNaN(ms(call.default_at!)))
		return refuse('invalid_request', 'A default needs the option and the time it applies.');
	if (!m.options?.some((o) => o.id === call.default_option) || m.approval)
		return refuse('invalid_request', 'The default must be one of the options.');
	m.default_option = call.default_option;
	m.default_at = call.default_at;
}

function closedRefusal(ask: ChatMessage, ctx: ChatContext): never | undefined {
	if (ask.withdrawn)
		return refuse(
			'already_exists',
			`Withdrawn by ${ask.withdrawn.by === OFFICE ? 'System' : (memberOf(ctx, ask.withdrawn.by)?.name ?? 'someone')}`
		);
	if (ask.answer)
		return refuse(
			'already_exists',
			ask.answer.by === 'default'
				? 'Already answered: default applied'
				: `Already answered by ${memberOf(ctx, ask.answer.by)?.name ?? 'someone'}`
		);
	return undefined;
}

function withdrawAsk(
	s: ChatState,
	call: Extract<ChatCall, { method: 'chat.withdraw' }>,
	ctx: ChatContext
): ChatAccepted {
	const ask = s.messages.find((m) => m.id === call.ask_id && m.kind === 'request');
	if (!ask) return refuse('not_found', 'There is no such ask.');
	if (ask.from !== ctx.actor)
		return refuse('not_authorized', 'Only the one who asked can withdraw it.');
	if (ask.withdrawn) return refuse('already_exists', 'Already withdrawn');
	closedRefusal(ask, ctx);
	const reason = call.reason?.trim() || undefined;
	ask.withdrawn = { by: ctx.actor, at: ctx.now, ...(reason ? { reason } : {}) };
	return { id: ask.id };
}

function handBackHandoff(
	s: ChatState,
	call: Extract<ChatCall, { method: 'chat.hand_back' }>,
	ctx: ChatContext
): ChatAccepted {
	const m = s.messages.find((x) => x.id === call.message_id && x.kind === 'handoff');
	if (!m) return refuse('not_found', 'There is no such hand-off.');
	if (m.accepted_by)
		return refuse(
			'already_exists',
			`Already accepted by ${memberOf(ctx, m.accepted_by)?.name ?? 'someone'}`
		);
	if (m.handed_back) return refuse('already_exists', 'Already handed back');
	if (ctx.actor !== m.addressed_to)
		return refuse('not_authorized', 'Only the one it is handed to can hand it back.');
	const home = s.conversations.find((x) => x.id === m.conversation_id)!;
	mustBeIn(ctx, home);
	const reason = call.reason.trim();
	if (!reason) return refuse('invalid_request', 'Say why you hand it back.');
	m.handed_back = { by: ctx.actor, at: ctx.now, reason };
	const reply: ChatMessage = {
		id: newIdOf(s, ctx, 'm'),
		conversation_id: home.id,
		from: ctx.actor,
		kind: 'reply',
		body: `Handed back: ${reason}`,
		created_at: ctx.now
	};
	if (home.kind !== 'dm') reply.thread_root_id = m.thread_root_id ?? m.id;
	s.messages.push(reply);
	return { id: m.id };
}

function channelDirectoryCall(
	s: ChatState,
	call: Extract<ChatCall, { method: 'channel.join' | 'channel.request_invitation' }>,
	ctx: ChatContext
): ChatAccepted {
	const c = s.conversations.find((x) => x.id === call.id && x.kind === 'channel' && !x.archived);
	if (!c || (c.access === 'restricted' && !isMember(c, ctx.actor)))
		return refuse('not_found', 'There is no such channel.');
	if (isMember(c, ctx.actor)) return refuse('already_exists', `You're already in #${c.name}.`);
	if (c.auto)
		return refuse(
			'invalid_request',
			`#${c.name} follows the organization: its members change with the ${c.auto.type === 'general' ? 'organization' : c.auto.type}.`
		);
	if (call.method === 'channel.join') {
		if ((c.access ?? 'open') !== 'open')
			return refuse('not_authorized', `#${c.name} is joined by invitation.`);
		c.members.push(ctx.actor);
		c.invite_requests = c.invite_requests?.filter((id) => id !== ctx.actor);
	} else {
		if (c.access !== 'invite') return refuse('invalid_request', `#${c.name} is open: join it.`);
		c.invite_requests = unique([...(c.invite_requests ?? []), ctx.actor]);
	}
	return { id: c.id, conversation_id: c.id };
}

function answer(
	s: ChatState,
	call: Extract<ChatCall, { method: 'chat.answer' }>,
	ctx: ChatContext
): ChatAccepted {
	const ask = s.messages.find((m) => m.id === call.ask_id && m.kind === 'request');
	const c = ask && s.conversations.find((x) => x.id === ask.conversation_id);
	if (!ask || !c) return refuse('not_found', 'There is no such ask.');
	closedRefusal(ask, ctx);
	if (
		(ask.addressed_to === ctx.owner || ask.approval) &&
		!(ctx.actor === ctx.owner && ctx.surface === 'app')
	)
		return refuse(
			'not_authorized',
			'Open Agentic Org to answer this: asks to the owner and approvals are answered only in the app.'
		);
	if (ctx.actor !== ask.addressed_to)
		return refuse('not_authorized', 'Only the person it is asked of can answer it.');
	mustBeIn(ctx, c);
	if (call.answers) return answerForm(s, ask, c, call.answers, ctx);
	const text = call.text?.trim() || undefined;
	if (text && bytes(text) > CHAT_LIMITS.bodyMax)
		return refuse('invalid_request', 'That reply is too long.');
	const option =
		call.option_id === undefined ? undefined : ask.options?.find((o) => o.id === call.option_id);
	if (call.option_id !== undefined && !option)
		return refuse('invalid_request', 'That is not one of the options.');
	if (!hasAnswerContent(option, text, call.attachment_ids))
		return refuse('invalid_request', 'Choose an option or write a reply.');
	if (ask.approval && option?.id === 'reject' && !text)
		return refuse('invalid_request', 'Say why you reject it.');
	ask.answer = {
		by: ctx.actor,
		at: ctx.now,
		...(option ? { option_id: option.id } : {}),
		...(text ? { text } : {})
	};
	const said = ask.approval
		? option?.id === 'approve'
			? 'Approved'
			: `Rejected: ${text}`
		: [option?.label.replace(/…$/, ''), text].filter(Boolean).join(': ');
	const reply: ChatMessage = {
		id: newIdOf(s, ctx, 'm'),
		conversation_id: c.id,
		from: ctx.actor,
		kind: 'reply',
		body: said,
		created_at: ctx.now
	};
	if (c.kind !== 'dm') reply.thread_root_id = ask.thread_root_id ?? ask.id;
	s.messages.push(reply);
	return { id: ask.id, conversation_id: c.id };
}

function normalizeQuestionAnswer(
	a: QuestionAnswer,
	questions: ReturnType<typeof questionsOf>,
	validIds: string
): QuestionAnswer | undefined {
	const q = questions.find((x) => x.id === a.question_id);
	if (!q)
		return refuse(
			'invalid_request',
			`There is no question ${a.question_id}. The questions are ${validIds}.`
		);
	const ids = [...new Set(a.option_ids ?? [])];
	const text = a.text?.trim() || undefined;
	const invalidChoice =
		q.question_type === 'free_text'
			? ids.length > 0
			: ids.some((id) => !optionsOf(q).some((o) => o.id === id));
	if (invalidChoice) return refuse('invalid_request', `That answer doesn't fit question ${q.id}.`);
	if (ids.length > 1 && !q.allow_multiple)
		return refuse('invalid_request', `Question ${q.id} takes one answer.`);
	if (!ids.length && !text) return undefined;
	return {
		question_id: q.id,
		...(ids.length ? { option_ids: ids } : {}),
		...(text ? { text } : {})
	};
}

function answerForm(
	s: ChatState,
	ask: ChatMessage,
	c: Conversation,
	given: QuestionAnswer[],
	ctx: ChatContext
): ChatAccepted {
	const questions = questionsOf(ask);
	const validIds = questions.map((q) => q.id).join(', ');
	const answers: QuestionAnswer[] = [];
	for (const a of given) {
		const normalized = normalizeQuestionAnswer(a, questions, validIds);
		if (normalized) answers.push(normalized);
	}
	const missing = questions.filter(
		(q) => !q.optional && !answers.some((a) => a.question_id === q.id)
	);
	if (missing.length)
		return refuse('invalid_request', `Answer ${missing.map((q) => q.id).join(', ')} first.`);
	ask.answer = { by: ctx.actor, at: ctx.now, answers };
	const body = questions
		.map(
			(q) =>
				`${q.prompt}: ${answerText(
					q,
					answers.find((a) => a.question_id === q.id)
				)}`
		)
		.join('\n');
	const reply: ChatMessage = {
		id: newIdOf(s, ctx, 'm'),
		conversation_id: c.id,
		from: ctx.actor,
		kind: 'reply',
		body,
		created_at: ctx.now
	};
	if (c.kind !== 'dm') reply.thread_root_id = ask.thread_root_id ?? ask.id;
	s.messages.push(reply);
	return { id: ask.id, conversation_id: c.id };
}

function openDm(s: ChatState, members: string[], ctx: ChatContext): ChatAccepted {
	const ids = unique([ctx.actor, ...members]);
	if (ids.length < 2) return refuse('invalid_request', 'Pick at least one other member.');
	if (ids.length > CHAT_LIMITS.groupDmMax)
		return refuse(
			'too_many_members',
			`A group message has up to ${CHAT_LIMITS.groupDmMax} members.`
		);
	for (const id of ids)
		if (!memberOf(ctx, id) || memberOf(ctx, id)?.former)
			return refuse('not_found', 'That member is not in the organization.');
	const key = [...ids].sort().join('|');
	const kind = ids.length === 2 ? 'dm' : 'group_dm';
	const existing = s.conversations.find(
		(c) => c.kind === kind && [...c.members].sort().join('|') === key
	);
	if (existing) return { conversation_id: existing.id, id: existing.id };
	const c: Conversation = { id: newIdOf(s, ctx, 'c'), kind, members: ids, created_at: ctx.now };
	s.conversations.push(c);
	return { conversation_id: c.id, id: c.id };
}

function channelCall(
	s: ChatState,
	call: Extract<ChatCall, { method: `channel.${string}` }>,
	ctx: ChatContext
): ChatAccepted {
	ownerOnly(ctx);
	const named = (typed: string, exceptId?: string): string => {
		const name = channelName(typed);
		if (!name) return refuse('invalid_request', 'Give the channel a name with letters or numbers.');
		if (s.conversations.some((c) => c.kind === 'channel' && c.name === name && c.id !== exceptId))
			return refuse('already_exists', `There is already a channel named #${name}.`);
		return name;
	};
	if (call.method === 'channel.create') {
		const name = named(call.name);
		const members = unique([ctx.actor, ...call.members]);
		for (const id of members)
			if (!memberOf(ctx, id) || memberOf(ctx, id)?.former)
				return refuse('not_found', 'That member is not in the organization.');
		const c: Conversation = {
			id: newIdOf(s, ctx, 'c'),
			kind: 'channel',
			name,
			members,
			created_at: ctx.now
		};
		s.conversations.push(c);
		return { conversation_id: c.id, id: c.id };
	}
	const c = s.conversations.find((x) => x.id === call.id && x.kind === 'channel');
	if (!c) return refuse('not_found', 'There is no such channel.');
	if (c.auto)
		return refuse(
			'invalid_request',
			`#${c.name} follows the organization: its name and members change with the ${c.auto.type === 'general' ? 'organization' : c.auto.type}.`
		);
	switch (call.method) {
		case 'channel.rename':
			c.name = named(call.name, c.id);
			break;
		case 'channel.archive':
			c.archived = true;
			break;
		case 'channel.add_member':
			if (!memberOf(ctx, call.member_id) || memberOf(ctx, call.member_id)?.former)
				return refuse('not_found', 'That member is not in the organization.');
			if (!c.members.includes(call.member_id)) c.members.push(call.member_id);
			break;
		case 'channel.remove_member':
			if (!c.members.includes(call.member_id))
				return refuse('not_in_conversation', 'They are not in this channel.');
			c.members = c.members.filter((id) => id !== call.member_id);
			break;
	}
	return { conversation_id: c.id, id: c.id };
}

// ---- time ------------------------------------------------------------------------------------------

function isDefaultDue(m: ChatMessage, nowMs: number): boolean {
	if (m.kind !== 'request' || m.answer || m.withdrawn || m.required) return false;
	const hasDefault = Boolean(m.default_option || m.questions?.some((q) => q.default_option));
	return hasDefault && Boolean(m.default_at) && ms(m.default_at!) <= nowMs;
}

function defaultLabelOf(m: ChatMessage, defaults: QuestionAnswer[]): string {
	if (m.default_option) {
		return (
			m.options?.find((o) => o.id === m.default_option)?.label.replace(/…$/, '') ?? m.default_option
		);
	}
	return questionsOf(m)
		.map((q) =>
			answerText(
				q,
				defaults.find((d) => d.question_id === q.id)
			)
		)
		.join('; ');
}

function applyDueDefault(s: ChatState, askId: string) {
	const m = s.messages.find((x) => x.id === askId)!;
	const c = s.conversations.find((x) => x.id === m.conversation_id);
	const defaults = questionsOf(m)
		.filter((q) => q.default_option)
		.map((q) => ({ question_id: q.id, option_ids: [q.default_option!] }));
	m.answer = {
		by: 'default',
		at: m.default_at!,
		...(m.default_option ? { option_id: m.default_option } : { answers: defaults })
	};
	s.seq += 1;
	const reply: ChatMessage = {
		id: `m-d${s.seq}`,
		conversation_id: m.conversation_id,
		from: OFFICE,
		kind: 'reply',
		body: `No answer by the deadline: the default applies (${defaultLabelOf(m, defaults)}).`,
		created_at: m.default_at!
	};
	if (c && c.kind !== 'dm') reply.thread_root_id = m.thread_root_id ?? m.id;
	s.messages.push(reply);
}

/**
 * Apply the default of every ask whose deadline has passed (CHAT-6): the ask is answered by `default` and the card says so.
 * The office does this itself; the app only shows it.
 */
export function tick(state: ChatState, now: string): ChatState {
	// A required ask is never closed by a default; a withdrawn one is closed already.
	const nowMs = ms(now);
	const due = state.messages.filter((m) => isDefaultDue(m, nowMs));
	const stale = state.typing.some((t) => ms(t.until) <= nowMs);
	if (!due.length && !stale) return state;
	const s = structuredClone(state);
	for (const ask of due) applyDueDefault(s, ask.id);
	s.typing = s.typing.filter((t) => ms(t.until) > nowMs);
	return s;
}
