import { formatUuidV7 } from '$lib/platform/http/clock';
import {
	asApiErrorCode,
	gatewayStatusCode,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import type { AppError } from '$lib/proto';
import type { InboxApi } from './api';
import type { InboxAction, InboxItem, InboxKind, InboxPerson, InboxRequest } from './types';

type Rec = Record<string, unknown>;

const KINDS: ReadonlySet<string> = new Set(['approval', 'question', 'decision', 'problem']);
const ACTIONS: ReadonlySet<string> = new Set(['approve', 'deny', 'reply', 'choose', 'dismiss']);

const asRec = (v: unknown): Rec | undefined =>
	v && typeof v === 'object' && !Array.isArray(v) ? (v as Rec) : undefined;
const asStr = (v: unknown): string | undefined => (typeof v === 'string' ? v : undefined);
const asNum = (v: unknown): number | undefined =>
	typeof v === 'number' && Number.isFinite(v) ? v : undefined;
const optField = <K extends string, V>(key: K, value: V | undefined): Partial<Record<K, V>> =>
	value === undefined ? {} : ({ [key]: value } as Partial<Record<K, V>>);
const msToIso = (v: unknown): string | undefined => {
	const n = asNum(v);
	return n !== undefined ? new Date(n).toISOString() : undefined;
};
const defaultNewId = () =>
	formatUuidV7(Date.now(), globalThis.crypto.getRandomValues(new Uint8Array(10)));

/** Extracts `<msg_id>` from a wire inbox reference (`<office_id>:msg:<msg_id>`). */
export function extractMessageId(reference: string): string | undefined {
	const match = /^[^:]+:msg:(.+)$/.exec(reference);
	return match?.[1] || undefined;
}

function unwrapInboxReply(reply: GatewayReply, fallbackMessage: string): Rec {
	const root = asRec(reply.body);
	if (reply.status !== 200 || root?.ok === false || root?.error) {
		const err = asRec(root?.error);
		throw {
			source: 'api',
			code: err ? asApiErrorCode(err.code) : gatewayStatusCode(reply.status),
			message: asStr(err?.message) || fallbackMessage
		} satisfies AppError;
	}
	const result = asRec(root?.result);
	if (!result) {
		throw { source: 'api', code: 'unavailable', message: fallbackMessage } satisfies AppError;
	}
	return result;
}

function mapPerson(raw: unknown): InboxPerson {
	if (typeof raw === 'string' && raw) return { id: raw, name: raw, kind: 'agent' };
	const r = asRec(raw);
	const id = asStr(r?.id) ?? 'unknown';
	const name = asStr(r?.name) ?? asStr(r?.display_name) ?? id;
	const kind = r?.kind === 'person' ? 'person' : 'agent';
	return { id, name, kind };
}

function mapRef(raw: unknown): { id: string; name: string } | undefined {
	if (typeof raw === 'string' && raw) return { id: raw, name: raw };
	const r = asRec(raw);
	const id = asStr(r?.id);
	return id ? { id, name: asStr(r?.name) ?? id } : undefined;
}

function mapChoices(raw: unknown): { id: string; label: string }[] | undefined {
	if (!Array.isArray(raw)) return undefined;
	// Only emit choices when wire options carry explicit `{ id, label }` pairs; bare label strings
	// must never be sent as `option_id` to `/chat/answer`.
	const list = raw
		.map((entry) => {
			const r = asRec(entry);
			const id = asStr(r?.id);
			const label = asStr(r?.label);
			return id && label ? { id, label } : undefined;
		})
		.filter((x): x is { id: string; label: string } => x !== undefined);
	return list.length ? list : undefined;
}

function hasRawStringOptions(raw: unknown): boolean {
	return Array.isArray(raw) && raw.some((entry) => typeof entry === 'string' && entry.length > 0);
}

function decodeOpenTarget(
	target: string | undefined,
	id: string | undefined,
	label: string
): InboxItem['open'] {
	if (!id) return undefined;
	if (target === 'message' || target === 'ask' || target === 'chat') {
		return { path: `/chat?m=${encodeURIComponent(id)}`, label };
	}
	if (target === 'conversation') {
		return { path: `/chat?c=${encodeURIComponent(id)}`, label };
	}
	return undefined;
}

function extractOpenFromActions(raw: unknown, reference: string): InboxItem['open'] {
	if (Array.isArray(raw)) {
		for (const entry of raw) {
			const r = asRec(entry);
			if (asStr(r?.type) !== 'open') continue;
			const open = asRec(r?.open);
			const mapped = decodeOpenTarget(
				asStr(open?.target),
				asStr(open?.id),
				asStr(r?.label) || 'Open in Chat'
			);
			if (mapped) return mapped;
		}
	}
	const msgId = extractMessageId(reference);
	return msgId
		? { path: `/chat?m=${encodeURIComponent(msgId)}`, label: 'Open in Chat' }
		: undefined;
}

function deriveCallAction(
	action: Rec,
	source: string | undefined,
	kind: InboxKind,
	hasChoiceIds: boolean,
	hasBareOptions: boolean
): InboxAction | undefined {
	const op = asStr(action.op);
	const params = asRec(action.params);
	if (op === 'msg.done' || op === 'chat.done') return 'dismiss';
	if (op === 'msg.reply' || op === 'chat.reply') return 'reply';
	if (op === 'msg.answer' || op === 'chat.answer') {
		const optId = asStr(params?.option_id);
		if (optId === 'approve') return 'approve';
		if (optId === 'reject' || optId === 'deny') return 'deny';
		if (kind === 'approval' || source === 'approval') return 'approve';
		if (hasChoiceIds) return 'choose';
		return hasBareOptions ? undefined : 'reply';
	}
	return undefined;
}

function deriveFallbackActions(
	source: string | undefined,
	kind: InboxKind,
	hasChoiceIds: boolean,
	hasBareOptions: boolean
): InboxAction[] {
	if (source === 'approval' || kind === 'approval') return ['approve', 'deny'];
	if (source === 'mailbox') return ['reply'];
	if (source === 'ask' || kind === 'question' || kind === 'decision') {
		if (hasChoiceIds) return ['choose'];
		return hasBareOptions ? [] : ['reply'];
	}
	return [];
}

function addUnique(list: InboxAction[], action: InboxAction | undefined) {
	if (action && !list.includes(action)) list.push(action);
}

function mapActions(params: {
	raw: unknown;
	source: string | undefined;
	kind: InboxKind;
	hasChoiceIds: boolean;
	hasBareOptions: boolean;
	dismissable: boolean;
	state: 'open' | 'snoozed' | 'handled';
}): InboxAction[] {
	const { raw, source, kind, hasChoiceIds, hasBareOptions, dismissable, state } = params;
	if (state === 'handled') return [];
	const out: InboxAction[] = [];
	const entries = Array.isArray(raw) ? raw : [];
	for (const item of entries) {
		if (typeof item === 'string') {
			const word = item === 'reject' ? 'deny' : item;
			if (ACTIONS.has(word)) addUnique(out, word as InboxAction);
			continue;
		}
		const rec = asRec(item);
		if (asStr(rec?.type) === 'call') {
			addUnique(out, deriveCallAction(rec!, source, kind, hasChoiceIds, hasBareOptions));
		}
	}
	if (out.length === 0) {
		for (const action of deriveFallbackActions(source, kind, hasChoiceIds, hasBareOptions)) {
			addUnique(out, action);
		}
	}
	if (kind === 'approval' && out.includes('approve')) addUnique(out, 'deny');
	if (dismissable) addUnique(out, 'dismiss');
	return out;
}

function sourceLabel(source: string | undefined, kind: InboxKind): string {
	if (!source) return kind === 'approval' ? 'Approval' : 'Chat ask';
	if (source === 'approval') return 'Chat approval';
	if (source === 'ask') return 'Chat ask';
	if (source === 'mailbox') return 'Chat message';
	if (source === 'chat' || source === 'messaging') {
		return kind === 'approval' ? 'Chat approval' : 'Chat ask';
	}
	return source;
}

function mapHandled(
	r: Rec,
	state: 'open' | 'snoozed' | 'handled',
	fallbackIso: string
): InboxItem['handled'] {
	if (state !== 'handled') return undefined;
	return {
		outcome: asStr(r.outcome) ?? 'Handled',
		at: msToIso(r.handled_at_ms) ?? fallbackIso
	};
}

function mapWireInboxItem(
	raw: unknown,
	fallbackIso: string
): { item: InboxItem; rawSource?: string } | undefined {
	const r = asRec(raw);
	const reference = asStr(r?.ref) ?? asStr(r?.reference);
	const summary = asStr(r?.summary);
	if (!r || !reference || !summary) return undefined;
	const rawKind = asStr(r.kind) ?? 'question';
	const kind: InboxKind = KINDS.has(rawKind) ? (rawKind as InboxKind) : 'question';
	const state: 'open' | 'snoozed' | 'handled' =
		r.state === 'handled' ? 'handled' : r.state === 'snoozed' ? 'snoozed' : 'open';
	const rawSource = asStr(r.source);
	const choices = mapChoices(r.options);
	const actions = mapActions({
		raw: r.actions,
		source: rawSource,
		kind,
		hasChoiceIds: !!choices?.length,
		hasBareOptions: hasRawStringOptions(r.options),
		dismissable: r.dismissable === true,
		state
	});
	return {
		rawSource,
		item: {
			id: reference,
			kind,
			source: sourceLabel(rawSource, kind),
			title: summary,
			who: mapPerson(r.member),
			...optField('team', mapRef(r.team)),
			...optField('project', mapRef(r.project)),
			why: asStr(r.context) ?? summary,
			...optField('recommendation', asStr(r.recommendation) || undefined),
			created_at: msToIso(r.created_at_ms) ?? fallbackIso,
			...optField('expires_at', msToIso(r.expires_at_ms)),
			actions,
			...optField('choices', choices),
			...optField('open', extractOpenFromActions(r.actions, reference)),
			snoozeable: Boolean(extractMessageId(reference)),
			state,
			...optField('snoozed_until', asStr(r.snoozed_until)),
			...optField('snooze_note', asStr(r.snooze_note) || undefined),
			...optField('handled', mapHandled(r, state, fallbackIso))
		}
	};
}

function mapSnoozedChatInboxItem(
	raw: unknown,
	fallbackIso: string,
	existing?: InboxItem
): { item: InboxItem; rawSource?: string } | undefined {
	const r = asRec(raw);
	const id = asStr(r?.id);
	if (!r || !id || r.state !== 'snoozed') return undefined;
	const rawKind = asStr(r.kind) ?? 'msg';
	const kind: InboxKind = existing?.kind ?? 'question';
	const fromId = asStr(r.from_member_id) ?? 'unknown';
	const fromName = asStr(r.from_display_name) ?? fromId;
	const subject = asStr(r.subject)?.trim();
	const reason = asStr(r.reason)?.trim();
	const title = subject || existing?.title || reason || `Snoozed message from ${fromName}`;
	const reference = existing?.id ?? `chat:msg:${id}`;
	const expiresAt = asStr(r.expires_at) ?? existing?.expires_at;
	const conversationId = asStr(r.conversation_id)?.trim();
	const chatPath = conversationId
		? `/chat?c=${encodeURIComponent(conversationId)}&m=${encodeURIComponent(id)}`
		: `/chat?m=${encodeURIComponent(id)}`;
	const open =
		conversationId && (!existing?.open || !/[?&]c=/.test(existing.open.path))
			? { path: chatPath, label: existing?.open?.label ?? 'Open in Chat' }
			: (existing?.open ?? { path: chatPath, label: 'Open in Chat' });
	return {
		rawSource: rawKind === 'request' ? 'ask' : 'mailbox',
		item: {
			id: reference,
			kind,
			source: existing?.source ?? (rawKind === 'request' ? 'Chat ask' : 'Chat message'),
			title,
			who: existing?.who ?? { id: fromId, name: fromName, kind: 'agent' },
			...optField('team', existing?.team),
			...optField('project', existing?.project),
			why: reason ?? existing?.why ?? subject ?? 'Snoozed chat message waiting to return.',
			...optField('recommendation', existing?.recommendation),
			created_at: asStr(r.created_at) ?? existing?.created_at ?? fallbackIso,
			...optField('expires_at', expiresAt),
			actions: existing?.actions.length ? existing.actions : ['reply', 'dismiss'],
			open,
			snoozeable: true,
			state: 'snoozed',
			...optField('snoozed_until', asStr(r.snoozed_until)),
			...optField('snooze_note', asStr(r.snooze_note) || undefined)
		}
	};
}

function requestAction(params: {
	base: string;
	msgId: string;
	request: InboxRequest;
	rawSource?: string;
	choices?: { id: string; label: string }[];
	newId: () => string;
}): { path: string; body: unknown; outcome: string } {
	const { base, msgId, request, rawSource, choices, newId } = params;
	switch (request.type) {
		case 'approve':
			return {
				path: `${base}/chat/answer`,
				body: { ask_id: msgId, option_id: 'approve' },
				outcome: 'Approved'
			};
		case 'deny':
			return {
				path: `${base}/chat/answer`,
				body: { ask_id: msgId, option_id: 'reject', text: request.reason },
				outcome: `Denied: ${request.reason}`
			};
		case 'choose': {
			const label = choices?.find((c) => c.id === request.choice)?.label ?? request.choice;
			return {
				path: `${base}/chat/answer`,
				body: { ask_id: msgId, option_id: request.choice },
				outcome: `Decided: ${label}`
			};
		}
		case 'reply':
			if (rawSource === 'mailbox') {
				return {
					path: `${base}/chat/reply`,
					body: { id: newId(), to_id: msgId, body: request.text },
					outcome: 'Replied'
				};
			}
			return {
				path: `${base}/chat/answer`,
				body: { ask_id: msgId, text: request.text },
				outcome: 'Answered'
			};
		case 'dismiss':
			return {
				path: `${base}/chat/done`,
				body: { id: msgId },
				outcome: 'Dismissed'
			};
	}
}

export interface HttpInboxApiOptions {
	http: GatewayHttpTransport;
	subscribe?: (organizationId: string, onChange: () => void) => () => void;
	now?: () => string;
	newId?: () => string;
}

/**
 * Production HTTP adapter for the unified Inbox (`/api/v1/organizations/{org}/inbox` + chat answer/reply/done).
 * Fails closed on gateway errors and never falls back to browser mocks.
 */
export function createHttpInboxApi({
	http,
	subscribe,
	now = () => new Date().toISOString(),
	newId = defaultNewId
}: HttpInboxApiOptions): InboxApi {
	const cachedItems = new Map<string, Map<string, { item: InboxItem; rawSource?: string }>>();
	const snoozedErrors = new Map<string, string>();
	const listSeqByOrg = new Map<string, number>();
	const localListeners = new Map<string, Set<() => void>>();

	function notify(orgId: string) {
		for (const listener of localListeners.get(orgId) ?? []) listener();
	}

	return Object.freeze({
		snoozedError(organizationId: string): string | undefined {
			return snoozedErrors.get(organizationId);
		},

		async list(organizationId: string): Promise<InboxItem[]> {
			const seq = (listSeqByOrg.get(organizationId) ?? 0) + 1;
			listSeqByOrg.set(organizationId, seq);

			let reply: GatewayReply;
			try {
				reply = await http.read(
					`/api/v1/organizations/${encodeURIComponent(organizationId)}/inbox`
				);
			} catch {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'The Inbox could not be reached.'
				} satisfies AppError;
			}
			const result = unwrapInboxReply(reply, 'The Inbox could not be loaded.');
			const asOf = asStr(result.as_of) ?? now();
			const rawItems = Array.isArray(result.items) ? result.items : [];
			const mapped = rawItems
				.map((entry) => mapWireInboxItem(entry, asOf))
				.filter((x): x is { item: InboxItem; rawSource?: string } => x !== undefined);

			// Paginate snoozed chat inbox items (`TASK-81`) using the real gateway `{ result }` envelope.
			// Keep the active queue usable on transient snoozed-list errors while surfacing a retryable error.
			const prevCache = cachedItems.get(organizationId);
			const prevByMsgId = new Map<string, InboxItem>();
			for (const entry of prevCache?.values() ?? []) {
				const mid = extractMessageId(entry.item.id);
				if (mid) prevByMsgId.set(mid, entry.item);
			}
			const seenMsgIds = new Set(
				mapped.map((m) => extractMessageId(m.item.id)).filter((id): id is string => Boolean(id))
			);
			const chatInboxPath = `/api/v1/organizations/${encodeURIComponent(organizationId)}/chat/inbox`;
			let cursor: number | undefined;
			let truncated = false;
			let nextSnoozedError: string | undefined;
			const MAX_SNOOZED_PAGES = 20;
			try {
				for (let page = 0; page < MAX_SNOOZED_PAGES; page += 1) {
					const snoozedReply = await http.submit(chatInboxPath, {
						state: 'snoozed',
						limit: 100,
						...(cursor !== undefined ? { cursor } : {})
					});
					if (!snoozedReply) break;
					if (snoozedReply.status === 404 || snoozedReply.status === 501) break;
					const snoozedResult = unwrapInboxReply(
						snoozedReply,
						'Snoozed items could not be loaded.'
					);
					const snoozedRaw = Array.isArray(snoozedResult.items) ? snoozedResult.items : [];
					for (const entry of snoozedRaw) {
						const rawId = asStr(asRec(entry)?.id);
						const mappedSnoozed = mapSnoozedChatInboxItem(
							entry,
							asOf,
							rawId ? prevByMsgId.get(rawId) : undefined
						);
						if (!mappedSnoozed) continue;
						const msgId = extractMessageId(mappedSnoozed.item.id);
						if (msgId && seenMsgIds.has(msgId)) continue;
						if (msgId) seenMsgIds.add(msgId);
						mapped.push(mappedSnoozed);
					}
					const nextCursor = asNum(snoozedResult.next_cursor);
					if (nextCursor === undefined || nextCursor === cursor) break;
					if (page === MAX_SNOOZED_PAGES - 1) {
						truncated = true;
						break;
					}
					cursor = nextCursor;
				}
				if (truncated) {
					nextSnoozedError = 'Showing the first 2,000 snoozed items; more exist on the server.';
				}
			} catch (err) {
				const appErr = asRec(err);
				const code = asStr(appErr?.code);
				if (code !== 'unsupported' && code !== 'not_found') {
					nextSnoozedError = asStr(appErr?.message) || 'Could not load snoozed items. Try again.';
				}
			}

			// Guard cache and snoozedError against stale overlapping list() refreshes.
			if (listSeqByOrg.get(organizationId) === seq) {
				if (nextSnoozedError) snoozedErrors.set(organizationId, nextSnoozedError);
				else snoozedErrors.delete(organizationId);
				cachedItems.set(organizationId, new Map(mapped.map((m) => [m.item.id, m])));
			}
			return mapped.map((m) => m.item);
		},

		async act(organizationId: string, itemId: string, request: InboxRequest): Promise<InboxItem> {
			const msgId = extractMessageId(itemId);
			if (!msgId) {
				throw {
					source: 'api',
					code: 'unsupported',
					message: 'This inbox item cannot be acted on from the browser yet.'
				} satisfies AppError;
			}
			const base = `/api/v1/organizations/${encodeURIComponent(organizationId)}`;
			const cached = cachedItems.get(organizationId)?.get(itemId);
			const action = requestAction({
				base,
				msgId,
				request,
				rawSource: cached?.rawSource,
				choices: cached?.item.choices,
				newId
			});
			let reply: GatewayReply;
			try {
				reply = await http.submit(action.path, action.body);
			} catch {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'The Inbox action could not be reached.'
				} satisfies AppError;
			}
			unwrapInboxReply(reply, 'The Inbox action was refused.');

			const updated: InboxItem = cached
				? {
						...cached.item,
						state: 'handled',
						actions: [],
						handled: { outcome: action.outcome, by: 'you', at: now() }
					}
				: {
						id: itemId,
						kind: request.type === 'approve' || request.type === 'deny' ? 'approval' : 'question',
						source: 'Chat ask',
						title: itemId,
						who: { id: 'unknown', name: 'Unknown', kind: 'agent' },
						why: '',
						created_at: now(),
						actions: [],
						state: 'handled',
						handled: { outcome: action.outcome, by: 'you', at: now() }
					};
			cachedItems.get(organizationId)?.set(itemId, { item: updated, rawSource: cached?.rawSource });
			notify(organizationId);
			return updated;
		},

		async defer(
			organizationId: string,
			itemId: string,
			untilIso: string,
			note?: string
		): Promise<InboxItem> {
			const msgId = extractMessageId(itemId);
			if (!msgId) {
				throw {
					source: 'api',
					code: 'unsupported',
					message: 'This inbox item cannot be snoozed from the browser yet.'
				} satisfies AppError;
			}
			const base = `/api/v1/organizations/${encodeURIComponent(organizationId)}`;
			const trimmedNote = note?.trim();
			let reply: GatewayReply;
			try {
				reply = await http.submit(`${base}/chat/defer`, {
					id: msgId,
					until: { type: 'until', at: untilIso },
					...(trimmedNote ? { note: trimmedNote } : {})
				});
			} catch {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'The Inbox snooze action could not be reached.'
				} satisfies AppError;
			}
			const result = unwrapInboxReply(reply, 'Could not snooze this inbox item.');
			const cached = cachedItems.get(organizationId)?.get(itemId);
			const snoozedUntil = asStr(result.snoozed_until) ?? untilIso;
			const snoozeNote = asStr(result.snooze_note) ?? (trimmedNote || undefined);
			const updated: InboxItem = cached
				? {
						...cached.item,
						state: 'snoozed',
						snoozed_until: snoozedUntil,
						...(snoozeNote ? { snooze_note: snoozeNote } : { snooze_note: undefined })
					}
				: {
						id: itemId,
						kind: 'question',
						source: 'Chat message',
						title: itemId,
						who: { id: 'unknown', name: 'Unknown', kind: 'agent' },
						why: '',
						created_at: now(),
						actions: ['reply', 'dismiss'],
						state: 'snoozed',
						snoozed_until: snoozedUntil,
						...(snoozeNote ? { snooze_note: snoozeNote } : {})
					};
			cachedItems.get(organizationId)?.set(itemId, { item: updated, rawSource: cached?.rawSource });
			notify(organizationId);
			return updated;
		},

		async unsnooze(organizationId: string, itemId: string): Promise<InboxItem> {
			const msgId = extractMessageId(itemId);
			if (!msgId) {
				throw {
					source: 'api',
					code: 'unsupported',
					message: 'This inbox item cannot be unsnoozed from the browser yet.'
				} satisfies AppError;
			}
			const base = `/api/v1/organizations/${encodeURIComponent(organizationId)}`;
			let reply: GatewayReply;
			try {
				reply = await http.submit(`${base}/chat/unsnooze`, { id: msgId });
			} catch {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'The Inbox un-snooze action could not be reached.'
				} satisfies AppError;
			}
			unwrapInboxReply(reply, 'Could not un-snooze this inbox item.');
			const cached = cachedItems.get(organizationId)?.get(itemId);
			const baseItem: InboxItem = cached
				? { ...cached.item }
				: {
						id: itemId,
						kind: 'question',
						source: 'Chat message',
						title: itemId,
						who: { id: 'unknown', name: 'Unknown', kind: 'agent' },
						why: '',
						created_at: now(),
						actions: ['reply', 'dismiss'],
						state: 'open'
					};
			delete baseItem.snoozed_until;
			delete baseItem.snooze_note;
			baseItem.state = 'open';
			cachedItems
				.get(organizationId)
				?.set(itemId, { item: baseItem, rawSource: cached?.rawSource });
			notify(organizationId);
			return baseItem;
		},

		async subscribe(organizationId: string, onChange: () => void): Promise<() => void> {
			let set = localListeners.get(organizationId);
			if (!set) {
				set = new Set();
				localListeners.set(organizationId, set);
			}
			set.add(onChange);
			const stopStream = subscribe?.(organizationId, onChange) ?? (() => {});
			return () => {
				set?.delete(onChange);
				stopStream();
			};
		}
	});
}
