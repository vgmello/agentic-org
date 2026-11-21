import {
	asApiErrorCode,
	gatewayStatusCode,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import {
	ANNOUNCEMENT_REASONS,
	type AnnouncementReason,
	type Answer,
	type AskOption,
	type AskQuestion,
	type AutoChannel,
	type ChannelAccess,
	type ChatAccepted,
	type ChatCall,
	type ChatCallStatus,
	type ChatErrorCode,
	type ChatMessage,
	type ChatRefusal,
	type ChatSnapshot,
	type Conversation,
	type ConversationKind,
	type MessageKind,
	type Mute,
	type MuteMode,
	type QuestionAnswer,
	type QuestionType,
	type Reaction,
	type Withdrawal
} from './types';
import type { ChatApi } from './api';

type Rec = Record<string, unknown>;
type DirectoryEntry = NonNullable<ChatSnapshot['directory']>[number];

const CHAT_ERROR_CODES: ReadonlySet<string> = new Set<ChatErrorCode>([
	'kind_not_allowed',
	'reason_required',
	'conversation_archived',
	'too_many_members',
	'not_in_conversation',
	'not_authorized',
	'not_found',
	'invalid_request',
	'already_exists',
	'unavailable'
]);

const CONV_KINDS: ReadonlySet<string> = new Set(['dm', 'group_dm', 'channel']);
const MSG_KINDS: ReadonlySet<string> = new Set([
	'msg',
	'request',
	'handoff',
	'announcement',
	'reply'
]);
const REASONS: ReadonlySet<string> = new Set(ANNOUNCEMENT_REASONS);
const QUESTION_TYPES: ReadonlySet<string> = new Set(['choice', 'yes_no', 'free_text']);
const CHANNEL_ACCESS: ReadonlySet<string> = new Set(['open', 'invite', 'restricted']);

const asRec = (v: unknown): Rec | undefined =>
	v && typeof v === 'object' && !Array.isArray(v) ? (v as Rec) : undefined;
const asStr = (v: unknown): string | undefined => (typeof v === 'string' ? v : undefined);
const asBool = (v: unknown): boolean | undefined => (typeof v === 'boolean' ? v : undefined);
const asNum = (v: unknown): number | undefined =>
	typeof v === 'number' && Number.isFinite(v) ? v : undefined;
const asStrList = (v: unknown): string[] =>
	Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : [];
const optField = <K extends string, V>(key: K, value: V | undefined): Partial<Record<K, V>> =>
	value === undefined ? {} : ({ [key]: value } as Partial<Record<K, V>>);

function asIso(v: unknown, fallback = new Date(0).toISOString()): string {
	if (typeof v === 'string' && v) return v;
	if (typeof v === 'number' && Number.isFinite(v)) return new Date(v).toISOString();
	return fallback;
}

function toChatErrorCode(code: unknown, status: number): ChatErrorCode {
	if (typeof code === 'string' && CHAT_ERROR_CODES.has(code)) return code as ChatErrorCode;
	const mapped = asApiErrorCode(code);
	if (CHAT_ERROR_CODES.has(mapped)) return mapped as ChatErrorCode;
	const byStatus = gatewayStatusCode(status);
	return CHAT_ERROR_CODES.has(byStatus) ? (byStatus as ChatErrorCode) : 'unavailable';
}

function throwChatRefusal(body: unknown, status: number, fallbackMessage: string): never {
	const root = asRec(body);
	const err = asRec(root?.error);
	throw {
		source: 'api',
		code: toChatErrorCode(err?.code, status),
		message: asStr(err?.message) || fallbackMessage
	} satisfies ChatRefusal;
}

function unwrapChatReply(reply: GatewayReply, fallbackMessage: string): Rec {
	const root = asRec(reply.body);
	if (reply.status !== 200 || root?.ok === false || root?.error) {
		throwChatRefusal(reply.body, reply.status, fallbackMessage);
	}
	const result = asRec(root?.result);
	if (!result) {
		throw { source: 'api', code: 'unavailable', message: fallbackMessage } satisfies ChatRefusal;
	}
	return result;
}

function normalizeOptions(raw: unknown): AskOption[] | undefined {
	if (!Array.isArray(raw)) return undefined;
	const options: AskOption[] = [];
	for (const entry of raw) {
		const r = asRec(entry);
		const id = asStr(r?.id);
		const label = asStr(r?.label);
		if (!id || !label) continue;
		const description = asStr(r?.description);
		options.push({ id, label, ...optField('description', description || undefined) });
	}
	return options.length ? options : undefined;
}

function normalizeQuestion(raw: unknown): AskQuestion | undefined {
	const r = asRec(raw);
	const id = asStr(r?.id);
	const prompt = asStr(r?.prompt);
	const rawType = asStr(r?.question_type);
	if (!id || !prompt || !rawType || !QUESTION_TYPES.has(rawType)) return undefined;
	const def = asStr(r?.default_option) || asStr(r?.default_value) || undefined;
	return {
		id,
		question_type: rawType as QuestionType,
		prompt,
		...optField('options', normalizeOptions(r?.options)),
		...optField('allow_multiple', asBool(r?.allow_multiple) || undefined),
		...optField('default_option', def),
		...optField('default_value', def),
		...optField('optional', asBool(r?.optional) || undefined)
	};
}

function normalizeQuestions(raw: unknown): AskQuestion[] | undefined {
	if (!Array.isArray(raw)) return undefined;
	const list = raw.map(normalizeQuestion).filter((q): q is AskQuestion => q !== undefined);
	return list.length ? list : undefined;
}

function normalizeQuestionAnswers(raw: unknown): QuestionAnswer[] | undefined {
	if (!Array.isArray(raw)) return undefined;
	const out: QuestionAnswer[] = [];
	for (const entry of raw) {
		const r = asRec(entry);
		const question_id = asStr(r?.question_id);
		if (!question_id) continue;
		const option_ids = Array.isArray(r?.option_ids) ? asStrList(r.option_ids) : undefined;
		out.push({
			question_id,
			...optField('option_ids', option_ids),
			...optField('text', asStr(r?.text))
		});
	}
	return out;
}

function normalizeAnswer(raw: unknown): Answer | undefined {
	const r = asRec(raw);
	const by = asStr(r?.by);
	if (!r || !by) return undefined;
	return {
		by,
		at: asIso(r.at),
		...optField('option_id', asStr(r.option_id)),
		...optField('text', asStr(r.text)),
		...optField('answers', normalizeQuestionAnswers(r.answers))
	};
}

function normalizeWithdrawal(raw: unknown): Withdrawal | undefined {
	const r = asRec(raw);
	const by = asStr(r?.by);
	if (!r || !by) return undefined;
	return { by, at: asIso(r.at), ...optField('reason', asStr(r.reason) || undefined) };
}

function normalizeReactionList(raw: unknown[]): Partial<Record<Reaction, string[]>> {
	const out: Partial<Record<Reaction, string[]>> = {};
	for (const item of raw) {
		const r = asRec(item);
		const reaction = asStr(r?.reaction);
		const actor = asStr(r?.employee_id) ?? asStr(r?.member_id);
		if ((reaction !== 'seen' && reaction !== 'looking') || !actor) continue;
		out[reaction] = [...(out[reaction] ?? []), actor];
	}
	return out;
}

function normalizeReactionRecord(rec: Rec): Partial<Record<Reaction, string[]>> {
	const out: Partial<Record<Reaction, string[]>> = {};
	for (const reaction of ['seen', 'looking'] as const) {
		const list = asStrList(rec[reaction]);
		if (list.length) out[reaction] = list;
	}
	return out;
}

function normalizeReactions(raw: unknown): Partial<Record<Reaction, string[]>> | undefined {
	const rec = asRec(raw);
	const out = Array.isArray(raw)
		? normalizeReactionList(raw)
		: rec
			? normalizeReactionRecord(rec)
			: {};
	return Object.keys(out).length ? out : undefined;
}

function normalizeAutoScope(r: Rec): AutoChannel | undefined {
	const scope = asStr(r.auto_scope);
	if (scope === 'organization') return { type: 'general' };
	if (scope === 'team') {
		const teamId = asStr(r.team_id);
		return teamId ? { type: 'team', team_id: teamId } : undefined;
	}
	if (scope === 'project') {
		const projectId = asStr(r.project_id);
		return projectId ? { type: 'project', project_id: projectId } : undefined;
	}
	return undefined;
}

function normalizeAutoChannel(r: Rec): AutoChannel | undefined {
	const flat = normalizeAutoScope(r);
	if (flat) return flat;
	const auto = asRec(r.auto);
	if (!auto) return undefined;
	if (auto.type === 'general') return { type: 'general' };
	if (auto.type === 'project' && typeof auto.project_id === 'string') {
		return { type: 'project', project_id: auto.project_id };
	}
	if (auto.type === 'team' && typeof auto.team_id === 'string') {
		return { type: 'team', team_id: auto.team_id };
	}
	return undefined;
}

function extractActiveMembers(raw: unknown): string[] {
	if (!Array.isArray(raw)) return [];
	const out: string[] = [];
	for (const entry of raw) {
		if (typeof entry === 'string') {
			if (entry) out.push(entry);
			continue;
		}
		const rec = asRec(entry);
		const id = asStr(rec?.employee_id);
		if (id && !asStr(rec?.left_at)) out.push(id);
	}
	return out;
}

function extractPinIds(raw: unknown): string[] {
	if (!Array.isArray(raw)) return [];
	const out: string[] = [];
	for (const entry of raw) {
		if (typeof entry === 'string' && entry) out.push(entry);
		else {
			const id = asStr(asRec(entry)?.message_id);
			if (id) out.push(id);
		}
	}
	return out;
}

function resolveConvKind(
	rawKind: string | undefined,
	fallback: ConversationKind
): ConversationKind {
	if (!rawKind) return fallback;
	return CONV_KINDS.has(rawKind) ? (rawKind as ConversationKind) : 'unknown';
}

interface MappedConversation {
	conversation: Conversation;
	readAt?: string;
	lastReadMessageId?: string;
	pinIds: string[];
}

function mapConversation(
	raw: unknown,
	fallbackKind: ConversationKind = 'channel'
): MappedConversation | undefined {
	const r = asRec(raw);
	const id = asStr(r?.id);
	if (!r || !id) return undefined;
	const rawAccess = asStr(r.access);
	const access =
		rawAccess && CHANNEL_ACCESS.has(rawAccess) ? (rawAccess as ChannelAccess) : undefined;
	const members = Array.isArray(r.members)
		? extractActiveMembers(r.members)
		: extractActiveMembers(r.participants);
	const conversation: Conversation = {
		id,
		kind: resolveConvKind(asStr(r.kind), fallbackKind),
		...optField('name', asStr(r.name) || undefined),
		...optField('purpose', asStr(r.purpose) || undefined),
		...optField('access', access),
		members,
		...optField('archived', r.archived === true ? true : undefined),
		...optField('auto', normalizeAutoChannel(r)),
		created_at: asIso(r.created_at ?? r.created_at_ms)
	};
	return {
		conversation,
		...optField('readAt', asStr(r.read_at) ?? asStr(r.last_read_at)),
		...optField('lastReadMessageId', asStr(r.last_read_message_id)),
		pinIds: extractPinIds(r.pins)
	};
}

function extractCardFields(meta: Rec | undefined): Partial<ChatMessage> {
	const card = asRec(meta?.card);
	const rawReason = asStr(meta?.reason);
	const reason =
		rawReason && REASONS.has(rawReason) ? (rawReason as AnnouncementReason) : undefined;
	return {
		...optField('questions', normalizeQuestions(card?.questions)),
		...optField('options', normalizeOptions(card?.options)),
		...optField('default_option', asStr(card?.default_option) || undefined),
		...optField('default_at', asStr(card?.default_at) || undefined),
		...optField('wait_until', asStr(card?.wait_until) || undefined),
		...optField('required', card?.required === true ? true : undefined),
		...optField('approval', card?.approval === true ? true : undefined),
		...optField('addressed_to', asStr(card?.addressed_to) ?? asStr(meta?.addressed_to)),
		...optField('blocks', asStr(card?.blocks) ?? asStr(meta?.blocks)),
		...optField('linked_ref', asStr(card?.linked_ref) ?? asStr(meta?.linked_ref)),
		...optField('reason', reason)
	};
}

function isMemberSenderKind(kind: string | undefined): boolean {
	return kind === 'agent' || kind === 'person' || kind === 'employee';
}

function resolveSender(r: Rec): { from: string; entry?: DirectoryEntry } {
	const senderKind = asStr(r.sender_kind);
	const senderId = asStr(r.sender_employee_id);
	const senderName = asStr(r.sender_display_name);
	const from =
		senderKind === 'office' || senderKind === 'system'
			? 'office'
			: (senderId ?? senderName ?? 'unknown');
	if (!senderId || !senderName || !isMemberSenderKind(senderKind)) {
		return { from };
	}
	return {
		from,
		entry: {
			id: senderId,
			kind: senderKind === 'person' ? 'person' : 'agent',
			name: senderName,
			handle: senderName.toLowerCase().replace(/[^a-z0-9]+/g, '-') || senderId
		}
	};
}

function resolveAcceptance(raw: unknown): Pick<ChatMessage, 'accepted_by' | 'accepted_at'> {
	const acc = asRec(raw);
	const by = asStr(acc?.accepted_by) ?? asStr(acc?.by);
	if (!acc || !by) return {};
	return { accepted_by: by, accepted_at: asIso(acc.accepted_at ?? acc.at) };
}

function resolveThreadRoot(r: Rec, id: string): string | undefined {
	const replyTo = asStr(r.reply_to);
	if (replyTo) return replyTo;
	const threadId = asStr(r.thread_id);
	return threadId && threadId !== id ? threadId : undefined;
}

function mapMessage(
	raw: unknown,
	fallbackConvId: string,
	pinnedIds: ReadonlySet<string>
): { message: ChatMessage; seq: number; sender?: DirectoryEntry } | undefined {
	const r = asRec(raw);
	const id = asStr(r?.id);
	if (!r || !id) return undefined;
	const rawKind = asStr(r.kind) ?? 'msg';
	const kind: MessageKind = MSG_KINDS.has(rawKind) ? (rawKind as MessageKind) : 'unknown';
	const { from, entry: sender } = resolveSender(r);
	const message: ChatMessage = {
		id,
		conversation_id: asStr(r.conversation_id) ?? fallbackConvId,
		...optField('thread_root_id', resolveThreadRoot(r, id)),
		...optField('also_in_channel', r.also_in_channel === true ? true : undefined),
		from,
		kind,
		body: asStr(r.body) ?? '',
		created_at: asIso(r.created_at ?? r.created_at_ms),
		...extractCardFields(asRec(r.meta)),
		...optField('answer', normalizeAnswer(r.answer)),
		...optField('withdrawn', normalizeWithdrawal(r.withdrawal)),
		...resolveAcceptance(r.acceptance),
		...optField('reactions', normalizeReactions(r.reactions)),
		...optField('pinned', pinnedIds.has(id) ? true : undefined)
	};
	return { message, seq: asNum(r.origin_sequence) ?? 0, sender };
}

interface WireAskQuestion {
	id: string;
	question_type: QuestionType;
	prompt: string;
	options?: AskOption[];
	allow_multiple?: boolean;
	default_value?: string;
	optional?: boolean;
}

function serializeAskQuestion(q: AskQuestion): WireAskQuestion {
	const defaultValue = q.default_option || q.default_value || undefined;
	return {
		id: q.id,
		question_type: q.question_type,
		prompt: q.prompt,
		...optField('options', q.options),
		...optField('allow_multiple', q.allow_multiple || undefined),
		...optField('default_value', defaultValue),
		...optField('optional', q.optional || undefined)
	};
}

const APPROVAL_OPTIONS: AskOption[] = [
	{ id: 'approve', label: 'Approve' },
	{ id: 'reject', label: 'Reject' }
];

function buildAskQuestions(call: Extract<ChatCall, { method: 'msg.send' }>): WireAskQuestion[] {
	if (call.questions?.length) return call.questions.map(serializeAskQuestion);
	if (call.options?.length) {
		return [
			serializeAskQuestion({
				id: 'q1',
				question_type: 'choice',
				prompt: call.body,
				options: call.options,
				...optField('default_option', call.default_option || undefined)
			})
		];
	}
	if (call.approval) {
		return [
			{
				id: 'q1',
				question_type: 'choice',
				prompt: call.body,
				options: APPROVAL_OPTIONS
			}
		];
	}
	return [{ id: 'q1', question_type: 'free_text', prompt: call.body }];
}

async function safeRead(
	http: GatewayHttpTransport,
	path: string,
	fallbackMessage: string
): Promise<Rec> {
	let reply: GatewayReply;
	try {
		reply = await http.read(path);
	} catch {
		throw { source: 'api', code: 'unavailable', message: fallbackMessage } satisfies ChatRefusal;
	}
	return unwrapChatReply(reply, fallbackMessage);
}

async function safeSubmit(
	http: GatewayHttpTransport,
	path: string,
	body: unknown,
	fallbackMessage: string
): Promise<Rec> {
	let reply: GatewayReply;
	try {
		reply = await http.submit(path, body);
	} catch {
		throw { source: 'api', code: 'unavailable', message: fallbackMessage } satisfies ChatRefusal;
	}
	return unwrapChatReply(reply, fallbackMessage);
}

async function dispatchAsk(
	base: string,
	http: GatewayHttpTransport,
	call: Extract<ChatCall, { method: 'msg.send' }>
): Promise<ChatAccepted> {
	const res = await safeSubmit(
		http,
		`${base}/ask`,
		{
			id: call.change_id,
			to: call.conversation_id,
			body: call.body,
			questions: buildAskQuestions(call),
			...optField('addressed_to', call.addressed_to),
			...optField('required', call.required),
			...optField('blocks', call.blocks),
			...optField('approval', call.approval)
		},
		'The ask could not be sent.'
	);
	return {
		id: asStr(res.ask_id) ?? asStr(res.id) ?? call.change_id,
		conversation_id: asStr(res.conversation_id) ?? call.conversation_id
	};
}

async function dispatchOrdinaryMessage(
	base: string,
	http: GatewayHttpTransport,
	call: Extract<ChatCall, { method: 'msg.send' }>
): Promise<ChatAccepted> {
	const meta: Rec = {
		...optField('addressed_to', call.addressed_to),
		...optField('blocks', call.blocks),
		...optField('linked_ref', call.linked_ref)
	};
	const res = await safeSubmit(
		http,
		`${base}/messages`,
		{
			id: call.change_id,
			to: call.conversation_id,
			kind: call.kind,
			body: call.body,
			...optField('reply_to', call.thread_root_id ?? call.reply_to),
			...optField('also_in_channel', call.also_in_channel || undefined),
			...optField('reason', call.reason),
			...optField('meta', Object.keys(meta).length ? meta : undefined)
		},
		'The message could not be sent.'
	);
	return {
		id: asStr(res.id) ?? call.change_id,
		conversation_id: asStr(res.conversation_id) ?? call.conversation_id
	};
}

interface CacheState {
	reactions: Map<string, Partial<Record<Reaction, string[]>>>;
	lastMessageByConv: Map<string, string>;
	applied: Map<string, ChatAccepted>;
	mutes: Map<string, Mute>;
}

interface DispatchContext {
	base: string;
	http: GatewayHttpTransport;
	cache: CacheState;
	viewerId?: string;
}

type CallHandler<M extends ChatCall['method']> = (
	ctx: DispatchContext,
	call: Extract<ChatCall, { method: M }>
) => Promise<ChatAccepted>;

const CALL_HANDLERS: Partial<{ [M in ChatCall['method']]: CallHandler<M> }> = {
	'msg.send': ({ base, http }, call) =>
		call.kind === 'request'
			? dispatchAsk(base, http, call)
			: dispatchOrdinaryMessage(base, http, call),
	'chat.answer': async ({ base, http }, call) => {
		const res = await safeSubmit(
			http,
			`${base}/answer`,
			{
				ask_id: call.ask_id,
				...optField('answers', call.answers),
				...optField('option_id', call.option_id),
				...optField('text', call.text)
			},
			'The answer could not be recorded.'
		);
		return { id: asStr(res.ask_id) ?? call.ask_id, conversation_id: asStr(res.conversation_id) };
	},
	'chat.withdraw': async ({ base, http }, call) => {
		const res = await safeSubmit(
			http,
			`${base}/withdraw`,
			{ ask_id: call.ask_id, ...optField('reason', call.reason) },
			'The ask could not be withdrawn.'
		);
		return { id: asStr(res.ask_id) ?? call.ask_id, conversation_id: asStr(res.conversation_id) };
	},
	'chat.accept_handoff': async ({ base, http }, call) => {
		const res = await safeSubmit(
			http,
			`${base}/accept`,
			{ id: call.message_id },
			'The hand-off could not be accepted.'
		);
		return { id: asStr(res.id) ?? call.message_id };
	},
	'chat.react': async ({ base, http, cache, viewerId }, call) => {
		const current = cache.reactions.get(call.message_id)?.[call.reaction] ?? [];
		const remove = viewerId ? current.includes(viewerId) : false;
		await safeSubmit(
			http,
			`${base}/react`,
			{ message_id: call.message_id, reaction: call.reaction, remove },
			'The reaction could not be updated.'
		);
		return { id: call.message_id };
	},
	'chat.mark_read': async ({ base, http, cache }, call) => {
		const lastId = cache.lastMessageByConv.get(call.conversation_id);
		await safeSubmit(
			http,
			`${base}/read`,
			{ conversation_id: call.conversation_id, ...optField('last_read_message_id', lastId) },
			'Read state could not be updated.'
		);
		return { conversation_id: call.conversation_id };
	},
	'chat.mute': async ({ base, http, cache }, call) => {
		await safeSubmit(
			http,
			`${base}/mute`,
			{
				conversation_id: call.conversation_id,
				muted_employee_id: call.member_id,
				mute_mode: call.mode
			},
			'Mute setting could not be updated.'
		);
		const key = `${call.conversation_id}:${call.member_id}`;
		if (call.mode === 'none') cache.mutes.delete(key);
		else {
			cache.mutes.set(key, {
				conversation_id: call.conversation_id,
				member_id: call.member_id,
				mode: call.mode as MuteMode
			});
		}
		return { conversation_id: call.conversation_id };
	},
	'channel.create': async ({ base, http }, call) => {
		const res = await safeSubmit(
			http,
			`${base}/channels`,
			{ name: call.name, members: call.members },
			'The channel could not be created.'
		);
		const id = asStr(asRec(res.channel)?.id);
		return { id, conversation_id: id };
	},
	'channel.join': async ({ base, http }, call) => {
		await safeSubmit(
			http,
			`${base}/channels/join`,
			{ channel: call.id },
			'Could not join the channel.'
		);
		return { id: call.id, conversation_id: call.id };
	},
	'dm.open': async ({ base, http }, call) => {
		const res = await safeSubmit(
			http,
			`${base}/dm`,
			{ participants: call.members },
			'The conversation could not be opened.'
		);
		const id = asStr(asRec(res.conversation)?.id);
		return { id, conversation_id: id };
	}
};

function mergeConversations(convRes: Rec, chanRes: Rec) {
	const byId = new Map<string, Conversation>();
	const readAt: Record<string, string> = {};
	const lastReadByConv = new Map<string, string>();
	const pinnedIds = new Set<string>();
	const joinedIds: string[] = [];
	const rawConvs = Array.isArray(convRes.conversations) ? convRes.conversations : [];
	for (const entry of rawConvs) {
		const mapped = mapConversation(entry, 'dm');
		if (!mapped) continue;
		const cid = mapped.conversation.id;
		byId.set(cid, mapped.conversation);
		joinedIds.push(cid);
		if (mapped.readAt) readAt[cid] = mapped.readAt;
		if (mapped.lastReadMessageId) lastReadByConv.set(cid, mapped.lastReadMessageId);
		for (const pinId of mapped.pinIds) pinnedIds.add(pinId);
	}
	const rawChans = Array.isArray(chanRes.channels) ? chanRes.channels : [];
	for (const entry of rawChans) {
		const mapped = mapConversation(entry, 'channel');
		if (mapped && !byId.has(mapped.conversation.id)) {
			byId.set(mapped.conversation.id, mapped.conversation);
		}
	}
	return { conversations: [...byId.values()], joinedIds, readAt, lastReadByConv, pinnedIds };
}

function collectMessages(
	lists: { convId: string; items: unknown[] }[],
	cache: CacheState,
	pinnedIds: ReadonlySet<string>,
	lastReadByConv: ReadonlyMap<string, string>,
	readAt: Record<string, string>
) {
	cache.reactions.clear();
	cache.lastMessageByConv.clear();
	const messages: ChatMessage[] = [];
	const dirMap = new Map<string, DirectoryEntry>();
	let maxSeq = 0;
	for (const { convId, items } of lists) {
		const readMsgId = lastReadByConv.get(convId);
		for (const item of items) {
			const mapped = mapMessage(item, convId, pinnedIds);
			if (!mapped) continue;
			messages.push(mapped.message);
			maxSeq = Math.max(maxSeq, mapped.seq);
			if (mapped.message.reactions)
				cache.reactions.set(mapped.message.id, mapped.message.reactions);
			cache.lastMessageByConv.set(convId, mapped.message.id);
			if (readMsgId && mapped.message.id === readMsgId && !readAt[convId]) {
				readAt[convId] = mapped.message.created_at;
			}
			if (mapped.sender) dirMap.set(mapped.sender.id, mapped.sender);
		}
	}
	return { messages, maxSeq, directory: dirMap.size ? [...dirMap.values()] : undefined };
}

export interface HttpChatApiOptions {
	http: GatewayHttpTransport;
	subscribe?: (organizationId: string, listener: () => void) => () => void;
	viewerId?: (organizationId: string) => string | undefined;
}

/**
 * Production HTTP adapter for Chat (`/api/v1/organizations/{org}/chat/*`).
 * Fails closed on gateway refusal or malformed envelopes and never falls back to browser mocks.
 */
export function createHttpChatApi({ http, subscribe, viewerId }: HttpChatApiOptions): ChatApi {
	const caches = new Map<string, CacheState>();
	const localListeners = new Map<string, Set<() => void>>();

	function cacheFor(orgId: string): CacheState {
		let state = caches.get(orgId);
		if (!state) {
			state = {
				reactions: new Map(),
				lastMessageByConv: new Map(),
				applied: new Map(),
				mutes: new Map()
			};
			caches.set(orgId, state);
		}
		return state;
	}

	function notifyLocal(orgId: string) {
		for (const listener of localListeners.get(orgId) ?? []) listener();
	}

	return Object.freeze({
		async snapshot(organizationId: string): Promise<ChatSnapshot> {
			const base = `/api/v1/organizations/${encodeURIComponent(organizationId)}/chat`;
			const [convRes, chanRes] = await Promise.all([
				safeRead(http, `${base}/conversations`, 'Conversations could not be loaded.'),
				safeRead(http, `${base}/channels`, 'Channels could not be loaded.').catch(() => ({
					channels: []
				}))
			]);
			const { conversations, joinedIds, readAt, lastReadByConv, pinnedIds } = mergeConversations(
				convRes,
				chanRes
			);
			const lists = await Promise.all(
				joinedIds.map((convId) =>
					safeSubmit(
						http,
						`${base}/messages/list`,
						{ conversation: convId, limit: 100 },
						'Messages could not be loaded.'
					).then((res) => ({ convId, items: Array.isArray(res.messages) ? res.messages : [] }))
				)
			);
			const cache = cacheFor(organizationId);
			const { messages, maxSeq, directory } = collectMessages(
				lists,
				cache,
				pinnedIds,
				lastReadByConv,
				readAt
			);
			return {
				state: {
					seq: maxSeq,
					conversations,
					messages,
					mutes: [...cache.mutes.values()],
					read_at: readAt,
					presence: {},
					typing: [],
					applied: Object.fromEntries(cache.applied)
				},
				sample: false,
				office_running: true,
				...optField('directory', directory)
			};
		},

		async command(organizationId: string, call: ChatCall): Promise<ChatAccepted> {
			const handler = CALL_HANDLERS[call.method] as
				((ctx: DispatchContext, c: ChatCall) => Promise<ChatAccepted>) | undefined;
			if (!handler) {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'This chat action is not available over the browser gateway yet.'
				} satisfies ChatRefusal;
			}
			const base = `/api/v1/organizations/${encodeURIComponent(organizationId)}/chat`;
			const cache = cacheFor(organizationId);
			const accepted = await handler(
				{ base, http, cache, viewerId: viewerId?.(organizationId) },
				call
			);
			cache.applied.set(call.change_id, accepted);
			notifyLocal(organizationId);
			return accepted;
		},

		async status(organizationId: string, changeId: string): Promise<ChatCallStatus> {
			const cached = cacheFor(organizationId).applied.get(changeId);
			if (cached) return { state: 'applied', accepted: cached };
			const snap = await this.snapshot(organizationId);
			const found = snap.state.messages.find((m) => m.id === changeId);
			return found
				? { state: 'applied', accepted: { id: found.id, conversation_id: found.conversation_id } }
				: { state: 'not_found' };
		},

		subscribe(organizationId: string, listener: () => void): () => void {
			let set = localListeners.get(organizationId);
			if (!set) {
				set = new Set();
				localListeners.set(organizationId, set);
			}
			set.add(listener);
			const stopStream = subscribe?.(organizationId, listener) ?? (() => {});
			return () => {
				set?.delete(listener);
				stopStream();
			};
		}
	});
}
