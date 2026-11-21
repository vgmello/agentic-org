import { appSession } from '$lib/platform/auth/app-session.svelte';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { chatSender, type ChatApi, type ChatDraft } from '$lib/modules/messaging/api';
import { createHttpChatApi } from './http-api';

export { chatSender, type ChatApi, type ChatDraft };

function signedInSession() {
	return appSession.state.kind === 'signed-in' ? appSession.state.session : undefined;
}

function subscribeChatFeeds(organizationId: string, listener: () => void): () => void {
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
	subscribe: subscribeChatFeeds,
	viewerId: (organizationId) =>
		signedInSession()?.organizations.find((o) => o.org_id === organizationId)?.employee_id
});

/**
 * Production Chat API backed by the browser gateway (`/api/v1/organizations/{org}/chat/*`).
 * Fails closed when no signed-in gateway session exists.
 * Under `bun run dev:mock`, `e2e/fakes/modules/chat.ts` stands in for this file by name.
 */
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
