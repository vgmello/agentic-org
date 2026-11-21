import { appSession } from '$lib/platform/auth/app-session.svelte';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { newId } from '$lib/modules/organization/commands';
import { createHttpChatApi } from './http-api';
import type { ChatAccepted, ChatCall, ChatCallStatus, ChatSnapshot } from './types';

/**
 * What the Chat screen needs from an office: a snapshot, the chat calls, delivery reconciliation,
 * and a signal when something changed. Production routes through the signed-in browser gateway and
 * fails closed when signed out; under `bun run dev:mock`, `e2e/fakes/modules/messaging.ts`
 * (re-exporting `e2e/fakes/modules/chat.ts`) stands in for this file.
 */
export interface ChatApi {
	snapshot(organizationId: string): Promise<ChatSnapshot>;
	/** Rejects with the error the office gave (a `ChatRefusal`, which `toAppError` passes through). */
	command(organizationId: string, call: ChatCall): Promise<ChatAccepted>;
	/** Calls `listener` whenever the conversations change. Returns the way to stop. */
	subscribe(organizationId: string, listener: () => void): () => void;
	/** What became of a call whose answer was lost, by its change id (`msg.status`, CHAT-40). */
	status(organizationId: string, changeId: string): Promise<ChatCallStatus>;
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

function signedInSession() {
	return appSession.state.kind === 'signed-in' ? appSession.state.session : undefined;
}

function subscribeMessagingFeeds(organizationId: string, listener: () => void): () => void {
	const session = signedInSession();
	if (!session) return () => {};
	const scope = {
		kind: 'organization' as const,
		instance_id: session.instance_id,
		org_id: organizationId
	};
	return appSession.controller().subscribeEvents(
		[
			{ scope, feed: 'messaging' },
			{ scope, feed: 'presence' },
			{ scope, feed: 'organization' }
		],
		{
			onChange: () => listener(),
			onResync: () => listener()
		}
	);
}

const gatewayChatApi = createHttpChatApi({
	http: {
		read: (path, signal) => appSession.controller().read(path, signal),
		submit: (path, body, signal) => appSession.controller().submit(path, body, signal)
	},
	subscribe: subscribeMessagingFeeds,
	viewerId: (organizationId) =>
		signedInSession()?.organizations.find((o) => o.org_id === organizationId)?.employee_id
});

export const chatApi: ChatApi = Object.freeze<ChatApi>({
	snapshot: (organizationId) =>
		signedInSession() ? gatewayChatApi.snapshot(organizationId) : unavailableCall(),
	command: (organizationId, call) =>
		signedInSession() ? gatewayChatApi.command(organizationId, call) : unavailableCall(),
	status: (organizationId, changeId) =>
		signedInSession() ? gatewayChatApi.status(organizationId, changeId) : unavailableCall(),
	subscribe: (organizationId, listener) =>
		signedInSession() ? gatewayChatApi.subscribe(organizationId, listener) : () => {}
});

export const sender = chatSender(chatApi);
