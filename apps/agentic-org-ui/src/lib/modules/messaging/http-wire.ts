import {
	asApiErrorCode,
	gatewayStatusCode,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import { asBool, asNum, asRec, asStr, asStrList, type Rec } from '$lib/platform/http/decode';
import { decodeMessageAttachments } from './http-wire-attachments';
import {
	ANNOUNCEMENT_REASONS,
	type AnnouncementReason,
	type Answer,
	type AskOption,
	type AskQuestion,
	type AutoChannel,
	type ChannelAccess,
	type ChatCall,
	type ChatErrorCode,
	type ChatMessage,
	type ChatRefusal,
	type ChatSnapshot,
	type Conversation,
	type ConversationKind,
	type MessageKind,
	type QuestionAnswer,
	type QuestionType,
	type Reaction,
	type Withdrawal
} from './types';

export type DirectoryEntry = NonNullable<ChatSnapshot['directory']>[number];

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

export const optField = <K extends string, V>(
	key: K,
	value: V | undefined
): Partial<Record<K, V>> =>
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

export function unwrapChatReply(reply: GatewayReply, fallbackMessage: string): Rec {
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
		...optField('answers', normalizeQuestionAnswers(r.answers)),
		...optField('attachment_meta', decodeMessageAttachments(r.attachments))
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

export function mapConversation(
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

export function mapMessage(
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
		...optField('attachment_meta', decodeMessageAttachments(r.attachments)),
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

export function buildAskQuestions(
	call: Extract<ChatCall, { method: 'msg.send' }>
): WireAskQuestion[] {
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
