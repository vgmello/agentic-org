import {
	inboxCommentsStorageKey,
	MAX_INBOX_COMMENT_LENGTH,
	parseInboxCommentsJson,
	type InboxCommentEntry
} from './features/triage/triage-model';

const INBOX_COMMENTS_PREFIX = 'agentic-org.inbox.comments.';

export function readInboxDeviceComments(orgId: string, itemId: string): InboxCommentEntry[] {
	if (typeof window === 'undefined') return [];
	try {
		return parseInboxCommentsJson(
			window.localStorage.getItem(inboxCommentsStorageKey(orgId, itemId))
		);
	} catch {
		return [];
	}
}

export function appendInboxDeviceComment(
	orgId: string,
	itemId: string,
	rawText: string
): InboxCommentEntry[] | undefined {
	const text = rawText.trim().slice(0, MAX_INBOX_COMMENT_LENGTH);
	const id =
		typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
			? crypto.randomUUID()
			: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
	const next = [...readInboxDeviceComments(orgId, itemId), { id, text, at: Date.now() }];
	try {
		window.localStorage.setItem(inboxCommentsStorageKey(orgId, itemId), JSON.stringify(next));
		return next;
	} catch {
		return undefined;
	}
}

/** Removes all device-local Inbox comment entries on sign-out or account switch (P1-4, TASK-226). */
export function clearInboxDeviceComments(): void {
	if (typeof window === 'undefined') return;
	try {
		const toRemove: string[] = [];
		for (let i = 0; i < window.localStorage.length; i++) {
			const key = window.localStorage.key(i);
			if (key?.startsWith(INBOX_COMMENTS_PREFIX)) toRemove.push(key);
		}
		for (const key of toRemove) {
			window.localStorage.removeItem(key);
		}
	} catch {
		/* Ignore storage unavailability during sign-out teardown. */
	}
}
