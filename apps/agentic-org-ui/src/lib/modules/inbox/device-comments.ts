import { appSession } from '$lib/platform/auth/app-session.svelte';
import {
	inboxCommentsStorageKey,
	MAX_INBOX_COMMENT_LENGTH,
	parseInboxCommentsJson,
	type InboxCommentEntry
} from './features/triage/triage-model';

const INBOX_COMMENTS_PREFIX = 'agentic-org.inbox.comments.';
const INBOX_COMMENTS_CALLER_KEY = 'agentic-org.inbox.comments.__caller';
const COMMENT_DRAFTS_SESSION_KEY = 'agentic-org.inbox.commentDrafts';

let memoryCommentDrafts: Record<string, string> = {};
let hasMemoryCommentDrafts = false;

function ensureCallerScope(): void {
	if (typeof window === 'undefined') return;
	const caller = appSession.callerScope;
	if (!caller) return;
	try {
		const stored = window.localStorage.getItem(INBOX_COMMENTS_CALLER_KEY);
		if (stored && stored !== caller) clearInboxDeviceComments();
		window.localStorage.setItem(INBOX_COMMENTS_CALLER_KEY, caller);
	} catch {
		/* Ignore storage unavailability. */
	}
}

export function readInboxCommentDrafts(): Record<string, string> {
	ensureCallerScope();
	if (hasMemoryCommentDrafts) return { ...memoryCommentDrafts };
	hasMemoryCommentDrafts = true;
	if (typeof window === 'undefined') return { ...memoryCommentDrafts };
	try {
		const raw = window.sessionStorage.getItem(COMMENT_DRAFTS_SESSION_KEY);
		if (!raw) return { ...memoryCommentDrafts };
		const parsed: unknown = JSON.parse(raw);
		if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
			return { ...memoryCommentDrafts };
		}
		const clean: Record<string, string> = {};
		for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
			if (typeof v === 'string') clean[k] = v;
		}
		memoryCommentDrafts = clean;
		return { ...clean };
	} catch {
		return { ...memoryCommentDrafts };
	}
}

export function writeInboxCommentDrafts(next: Record<string, string>): void {
	ensureCallerScope();
	hasMemoryCommentDrafts = true;
	memoryCommentDrafts = { ...next };
	if (typeof window === 'undefined') return;
	try {
		window.sessionStorage.setItem(COMMENT_DRAFTS_SESSION_KEY, JSON.stringify(memoryCommentDrafts));
	} catch {
		/* Keep authoritative in-memory drafts when sessionStorage is unavailable or over quota. */
	}
}

export function clearInboxCommentDrafts(): void {
	memoryCommentDrafts = {};
	hasMemoryCommentDrafts = false;
	if (typeof window === 'undefined') return;
	try {
		window.sessionStorage.removeItem(COMMENT_DRAFTS_SESSION_KEY);
	} catch {
		/* Ignore storage unavailability during sign-out teardown. */
	}
}

export function readInboxDeviceComments(orgId: string, itemId: string): InboxCommentEntry[] {
	if (typeof window === 'undefined') return [];
	ensureCallerScope();
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
	ensureCallerScope();
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

/** Removes all device-local Inbox comment entries and drafts on sign-out or account switch (P1-4, TASK-226). */
export function clearInboxDeviceComments(): void {
	clearInboxCommentDrafts();
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
