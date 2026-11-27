import { appSession } from '$lib/platform/auth/app-session.svelte';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { createHttpInboxApi } from './http-api';
import type { InboxItem, InboxRequest } from './types';

/** The owner's unified Inbox (our-org-in-the-app §6.1): one item per source, cleared when its source resolves. */
export interface InboxApi {
	/** Every item, open and handled, in no particular order. */
	list(organizationId: string, signal?: AbortSignal): Promise<InboxItem[]>;
	/**
	 * Does what the owner asked, through the item's source. Resolves with the item as it now is (handled).
	 * Rejects with the source's own reason when it refuses; the item then stays open.
	 */
	act(organizationId: string, itemId: string, request: InboxRequest): Promise<InboxItem>;
	/**
	 * Defers (snoozes) an inbox item until an RFC 3339 future timestamp (`TASK-81`).
	 * Resolves with the updated snoozed item; rejects when the item cannot be snoozed or the timestamp is invalid.
	 */
	defer(
		organizationId: string,
		itemId: string,
		untilIso: string,
		note?: string
	): Promise<InboxItem>;
	/** Releases a snoozed inbox item back to the open queue early (`TASK-81`). */
	unsnooze(organizationId: string, itemId: string): Promise<InboxItem>;
	/** Optional error message from the last snoozed-list fetch when active Inbox loaded but Snoozed failed. */
	snoozedError?(organizationId: string): string | undefined;
	/** Told when the durable result of any source changes (cleared here, in another client or in the CLI). Resolves to the function that stops listening. */
	subscribe(organizationId: string, onChange: () => void): Promise<() => void>;
}

function signedInSession() {
	return appSession.state.kind === 'signed-in' ? appSession.state.session : undefined;
}

function subscribeInboxFeeds(organizationId: string, onChange: () => void): () => void {
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
			{ scope, feed: 'organization' }
		],
		{
			onChange: () => onChange(),
			onResync: () => onChange()
		}
	);
}

const gatewayInboxApi = createHttpInboxApi({
	http: {
		read: (path, signal) => appSession.controller().read(path, signal),
		submit: (path, body, signal) => appSession.controller().submit(path, body, signal)
	},
	subscribe: subscribeInboxFeeds,
	newId: () => appSession.newOperationId()
});

/**
 * Production Inbox API backed by the browser gateway (`/api/v1/organizations/{org}/inbox`).
 * Fails closed when no signed-in gateway session exists.
 * Under `bun run dev:mock`, `e2e/fakes/modules/inbox.ts` stands in for this file.
 */
export const inboxApi: InboxApi = Object.freeze<InboxApi>({
	list: (organizationId, signal) =>
		signedInSession() ? gatewayInboxApi.list(organizationId, signal) : unavailableCall(),
	act: (organizationId, itemId, request) =>
		signedInSession() ? gatewayInboxApi.act(organizationId, itemId, request) : unavailableCall(),
	defer: (organizationId, itemId, untilIso, note) =>
		signedInSession()
			? gatewayInboxApi.defer(organizationId, itemId, untilIso, note)
			: unavailableCall(),
	unsnooze: (organizationId, itemId) =>
		signedInSession() ? gatewayInboxApi.unsnooze(organizationId, itemId) : unavailableCall(),
	snoozedError: (organizationId) =>
		signedInSession() ? gatewayInboxApi.snoozedError?.(organizationId) : undefined,
	subscribe: (organizationId, onChange) =>
		signedInSession() ? gatewayInboxApi.subscribe(organizationId, onChange) : unavailableCall()
});
