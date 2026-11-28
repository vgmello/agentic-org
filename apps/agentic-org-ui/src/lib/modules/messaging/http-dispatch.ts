import type { GatewayHttpTransport, GatewayReply } from '$lib/platform/http/gateway-errors';
import { asRec, asStr, type Rec } from '$lib/platform/http/decode';
import type {
	ChatAccepted,
	ChatCall,
	ChatMessage,
	ChatRefusal,
	Conversation,
	Mute,
	MuteMode,
	Reaction
} from './types';
import {
	buildAskQuestions,
	mapConversation,
	mapMessage,
	optField,
	unwrapChatReply,
	type DirectoryEntry
} from './http-wire';
import {
	attachmentFilterFields,
	attachmentIdsField,
	type MessageAttachmentFilter
} from './http-wire-attachments';

export interface CacheState {
	reactions: Map<string, Partial<Record<Reaction, string[]>>>;
	lastMessageByConv: Map<string, string>;
	applied: Map<string, ChatAccepted>;
	mutes: Map<string, Mute>;
}

export interface DispatchContext {
	base: string;
	http: GatewayHttpTransport;
	cache: CacheState;
	viewerId?: string;
}

export async function safeRead(
	http: GatewayHttpTransport,
	path: string,
	fallbackMessage: string,
	signal?: AbortSignal
): Promise<Rec> {
	let reply: GatewayReply;
	try {
		reply = await (signal ? http.read(path, signal) : http.read(path));
	} catch (err) {
		if (signal?.aborted) throw err;
		throw { source: 'api', code: 'unavailable', message: fallbackMessage } satisfies ChatRefusal;
	}
	return unwrapChatReply(reply, fallbackMessage);
}

export async function safeSubmit(
	http: GatewayHttpTransport,
	path: string,
	body: unknown,
	fallbackMessage: string,
	signal?: AbortSignal
): Promise<Rec> {
	let reply: GatewayReply;
	try {
		reply = await (signal ? http.submit(path, body, signal) : http.submit(path, body));
	} catch (err) {
		if (signal?.aborted) throw err;
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
			...optField('approval', call.approval),
			...attachmentIdsField(call.attachment_ids)
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
			...optField('meta', Object.keys(meta).length ? meta : undefined),
			...attachmentIdsField(call.attachment_ids)
		},
		'The message could not be sent.'
	);
	return {
		id: asStr(res.id) ?? call.change_id,
		conversation_id: asStr(res.conversation_id) ?? call.conversation_id
	};
}

export function buildMessageListBody(
	conversation: string,
	limit = 100,
	filter?: MessageAttachmentFilter
): Rec {
	return { conversation, limit, ...attachmentFilterFields(filter) };
}

export function buildChatInboxBody(
	params: { include_all?: boolean; limit?: number; cursor?: number } & MessageAttachmentFilter = {}
): Rec {
	return {
		...optField('include_all', params.include_all || undefined),
		...optField('limit', params.limit),
		...optField('cursor', params.cursor),
		...attachmentFilterFields(params)
	};
}

type CallHandler<M extends ChatCall['method']> = (
	ctx: DispatchContext,
	call: Extract<ChatCall, { method: M }>
) => Promise<ChatAccepted>;

export const CALL_HANDLERS: Partial<{ [M in ChatCall['method']]: CallHandler<M> }> = {
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
				...optField('text', call.text),
				...attachmentIdsField(call.attachment_ids)
			},
			'The answer could not be recorded.'
		);
		return { id: asStr(res.ask_id) ?? call.ask_id, conversation_id: asStr(res.conversation_id) };
	},
	'msg.reply': async ({ base, http }, call) => {
		const res = await safeSubmit(
			http,
			`${base}/reply`,
			{
				id: call.change_id,
				to_id: call.to_id,
				body: call.body,
				...attachmentIdsField(call.attachment_ids)
			},
			'The reply could not be sent.'
		);
		return {
			id: asStr(res.id) ?? call.change_id,
			conversation_id: asStr(res.conversation_id)
		};
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

export function mergeConversations(convRes: Rec, chanRes: Rec) {
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

export function collectMessages(
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
