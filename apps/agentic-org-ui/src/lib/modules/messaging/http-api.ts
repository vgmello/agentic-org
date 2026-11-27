import type { GatewayHttpTransport } from '$lib/platform/http/gateway-errors';
import type { ChatApi } from './api';
import {
	CALL_HANDLERS,
	collectMessages,
	mergeConversations,
	safeRead,
	safeSubmit,
	type CacheState,
	type DispatchContext
} from './http-dispatch';
import { optField } from './http-wire';
import type { ChatAccepted, ChatCall, ChatCallStatus, ChatRefusal, ChatSnapshot } from './types';

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
		async snapshot(organizationId: string, signal?: AbortSignal): Promise<ChatSnapshot> {
			const base = `/api/v1/organizations/${encodeURIComponent(organizationId)}/chat`;
			const [convRes, chanRes] = await Promise.all([
				safeRead(http, `${base}/conversations`, 'Conversations could not be loaded.', signal),
				safeRead(http, `${base}/channels`, 'Channels could not be loaded.', signal).catch(() => ({
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
						'Messages could not be loaded.',
						signal
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
