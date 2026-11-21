import { newId } from '$lib/org/commands';
import { unavailableCall } from '$lib/platform/unavailable-api';
import type { ChatAccepted, ChatCall, ChatSnapshot } from './types';

/**
 * What the Chat screen needs from an office: a snapshot, the chat calls, and a signal when
 * something changed. Production defaults fail closed; under `bun run dev:mock`,
 * `e2e/fakes/modules/messaging.ts` stands in for this file.
 */
export interface ChatApi {
	snapshot(organizationId: string): Promise<ChatSnapshot>;
	/** Rejects with the error the office gave (a `ChatRefusal`, which `toAppError` passes through). */
	command(organizationId: string, call: ChatCall): Promise<ChatAccepted>;
	/** Calls `listener` whenever the conversations change. Returns the way to stop. */
	subscribe(organizationId: string, listener: () => void): () => void;
}

/** A call without its change id, which `chatSender` adds (one fresh ULID per call). */
export type ChatDraft = ChatCall extends infer C
	? C extends unknown
		? Omit<C, 'change_id'>
		: never
	: never;

/** Bind one explicit API; test composition uses the same sender without a production fallback. */
export const chatSender =
	(api: ChatApi) =>
	(organizationId: string) =>
	(draft: ChatDraft): Promise<ChatAccepted> =>
		api.command(organizationId, { ...draft, change_id: newId() } as ChatCall);

// The web API does not serve chat yet: reads and writes refuse, and subscribe is a no-op.
export const chatApi: ChatApi = Object.freeze({
	snapshot: unavailableCall,
	command: unavailableCall,
	subscribe: () => () => {}
});

export const sender = chatSender(chatApi);
