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

function mapActions(raw: unknown, dismissable: boolean, state: 'open' | 'handled'): InboxAction[] {
	if (state === 'handled') return [];
	const out: InboxAction[] = [];
	const entries = Array.isArray(raw) ? raw : [];
	for (const item of entries) {
		const word = item === 'reject' ? 'deny' : asStr(item);
		if (word && ACTIONS.has(word) && !out.includes(word as InboxAction)) {
			out.push(word as InboxAction);
		}
	}
	if (dismissable && !out.includes('dismiss')) out.push('dismiss');
	return out;
}

function mapChoices(raw: unknown): { id: string; label: string }[] | undefined {
	if (!Array.isArray(raw)) return undefined;
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

function sourceLabel(source: string | undefined, kind: InboxKind): string {
	if (!source) return kind === 'approval' ? 'Approval' : 'Chat ask';
	if (source === 'chat' || source === 'messaging') {
		return kind === 'approval' ? 'Chat approval' : 'Chat ask';
	}
	return source;
}

function mapHandled(r: Rec, state: 'open' | 'handled', fallbackIso: string): InboxItem['handled'] {
	if (state !== 'handled') return undefined;
	return {
		outcome: asStr(r.outcome) ?? 'Handled',
		at: msToIso(r.handled_at_ms) ?? fallbackIso
	};
}

function openLinkFor(reference: string): InboxItem['open'] {
	const msgId = extractMessageId(reference);
	return msgId
		? { path: `/chat?m=${encodeURIComponent(msgId)}`, label: 'Open in Chat' }
		: undefined;
}

function mapWireInboxItem(raw: unknown, fallbackIso: string): InboxItem | undefined {
	const r = asRec(raw);
	const reference = asStr(r?.reference);
	const summary = asStr(r?.summary);
	if (!r || !reference || !summary) return undefined;
	const rawKind = asStr(r.kind) ?? 'question';
	const kind: InboxKind = KINDS.has(rawKind) ? (rawKind as InboxKind) : 'question';
	const state: 'open' | 'handled' = r.state === 'handled' ? 'handled' : 'open';
	return {
		id: reference,
		kind,
		source: sourceLabel(asStr(r.source), kind),
		title: summary,
		who: mapPerson(r.member),
		...optField('team', mapRef(r.team)),
		...optField('project', mapRef(r.project)),
		why: asStr(r.context) ?? summary,
		...optField('recommendation', asStr(r.recommendation) || undefined),
		created_at: msToIso(r.created_at_ms) ?? fallbackIso,
		...optField('expires_at', msToIso(r.expires_at_ms)),
		actions: mapActions(r.actions, r.dismissable === true, state),
		...optField('choices', mapChoices(r.options)),
		...optField('open', openLinkFor(reference)),
		state,
		...optField('handled', mapHandled(r, state, fallbackIso))
	};
}

function requestAction(
	base: string,
	msgId: string,
	request: InboxRequest,
	choices?: { id: string; label: string }[]
): { path: string; body: unknown; outcome: string } {
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
}

/**
 * Production HTTP adapter for the unified Inbox (`/api/v1/organizations/{org}/inbox` + chat answer/done).
 * Fails closed on gateway errors and never falls back to browser mocks.
 */
export function createHttpInboxApi({
	http,
	subscribe,
	now = () => new Date().toISOString()
}: HttpInboxApiOptions): InboxApi {
	const cachedItems = new Map<string, Map<string, InboxItem>>();
	const localListeners = new Map<string, Set<() => void>>();

	function notify(orgId: string) {
		for (const listener of localListeners.get(orgId) ?? []) listener();
	}

	return Object.freeze({
		async list(organizationId: string): Promise<InboxItem[]> {
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
			const items = rawItems
				.map((entry) => mapWireInboxItem(entry, asOf))
				.filter((item): item is InboxItem => item !== undefined);
			cachedItems.set(organizationId, new Map(items.map((item) => [item.id, item])));
			return items;
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
			const existing = cachedItems.get(organizationId)?.get(itemId);
			const action = requestAction(base, msgId, request, existing?.choices);
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

			const updated: InboxItem = existing
				? {
						...existing,
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
			cachedItems.get(organizationId)?.set(itemId, updated);
			notify(organizationId);
			return updated;
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
