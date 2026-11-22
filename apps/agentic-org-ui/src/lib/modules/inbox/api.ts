import { unavailableCall } from '$lib/platform/unavailable-api';
import type { InboxItem, InboxRequest } from './types';

/** The owner's unified Inbox (our-org-in-the-app §6.1): one item per source, cleared when its source resolves. */
export interface InboxApi {
	/** Every item, open and handled, in no particular order. */
	list(organizationId: string): Promise<InboxItem[]>;
	/**
	 * Does what the owner asked, through the item's source. Resolves with the item as it now is (handled).
	 * Rejects with the source's own reason when it refuses; the item then stays open.
	 */
	act(organizationId: string, itemId: string, request: InboxRequest): Promise<InboxItem>;
	/** Told when the durable result of any source changes (cleared here, in another client or in the CLI). Resolves to the function that stops listening. */
	subscribe(organizationId: string, onChange: () => void): Promise<() => void>;
}

// The web API does not serve the Inbox yet: every call refuses, and the navigation hides the Inbox item. Under
// `bun run dev:mock`, e2e/fakes/modules/inbox.ts stands in for this file.
export const inboxApi: InboxApi = {
	list: unavailableCall,
	act: unavailableCall,
	subscribe: unavailableCall
};
