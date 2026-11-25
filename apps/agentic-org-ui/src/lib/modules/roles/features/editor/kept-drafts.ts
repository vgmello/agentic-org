// Unsaved editor text kept when the editor closes without a deliberate Cancel: browser Back with the editor open
// (app-shell-ux §6: Back closes the modal only) or leaving the page while it is open. §6a keeps input on failure
// and disconnect; this keeps it across Back too. Reopening the same editor restores it. Memory only, for this tab:
// nothing is written to storage, and drafts are scoped per caller so a different account never sees an earlier
// caller's text after sign-out/sign-in in the same tab.
import { appSession } from '$lib/platform/auth/app-session.svelte';
import type { Role } from '../../types';

export interface KeptDraft {
	name: string;
	key: string;
	body: string;
	/** The revision the person was editing against: a newer one is still a conflict on save. */
	expectedRevision: number;
	/** The newer version shown after a conflict, if any. */
	latest?: Role;
	conflictMessage?: string;
	/** Logical-save token kept with an unsaved draft so retrying after an uncertain outcome replays cleanly. */
	attemptId?: string;
	/** Set after an uncertain save failure so restoring the original text still reconciles with HQ. */
	hasUncertainAttempt?: boolean;
}

type KeptEntry = {
	draft: KeptDraft;
	version: number;
};

const kept = new Map<string, KeptEntry>();
let nextVersion = 0;
let attemptReleasedListeners: ((attemptId: string) => void)[] = [];

function notifyIfAttemptReleased(attemptId: string | undefined) {
	if (!attemptId || hasRetainedAttempt(attemptId)) return;
	for (const listener of [...attemptReleasedListeners]) {
		try {
			listener(attemptId);
		} catch {
			/* Keep draft cleanup isolated from listener failures. */
		}
	}
}

/** One editor per caller, organization and role; a new role has one slot per caller+organization. */
export const draftKey = (organizationId: string, roleId?: string, caller = '') =>
	`${caller}\u0000${organizationId}\u0000${roleId ?? ''}`;

/** Stores a copy of `draft` and returns its monotonic version stamp for safe late-save cleanup. */
export function keepDraft(key: string, draft: KeptDraft): number {
	const previousAttempt = kept.get(key)?.draft.attemptId;
	const version = ++nextVersion;
	kept.set(key, { draft: structuredClone(draft), version });
	if (previousAttempt && previousAttempt !== draft.attemptId) {
		notifyIfAttemptReleased(previousAttempt);
	}
	return version;
}

/** The kept draft, if any; it stays kept until saved or discarded. */
export function keptDraft(key: string): KeptDraft | undefined {
	const entry = kept.get(key);
	return entry ? structuredClone(entry.draft) : undefined;
}

export function discardDraft(key: string) {
	const attemptId = kept.get(key)?.draft.attemptId;
	kept.delete(key);
	notifyIfAttemptReleased(attemptId);
}

/** Discards the kept draft only when its version matches `version`, preserving newer edits. */
export function discardDraftIfVersion(key: string, version: number | undefined) {
	if (version === undefined) return;
	const entry = kept.get(key);
	if (entry?.version !== version) return;
	kept.delete(key);
	notifyIfAttemptReleased(entry.draft.attemptId);
}

/** True while any retained draft still references `attemptId` so retry identity outlives detail-cache eviction. */
export function hasRetainedAttempt(attemptId: string): boolean {
	for (const entry of kept.values()) {
		if (entry.draft.attemptId === attemptId) return true;
	}
	return false;
}

/** Fires when the last retained draft referencing an `attemptId` is discarded or replaced. */
export function onAttemptReleased(listener: (attemptId: string) => void): () => void {
	if (!attemptReleasedListeners.includes(listener)) attemptReleasedListeners.push(listener);
	return () => {
		attemptReleasedListeners = attemptReleasedListeners.filter((entry) => entry !== listener);
	};
}

/** Removes all retained drafts belonging to `caller` (or all drafts if `caller` is omitted). */
export function clearDraftsForCaller(caller?: string) {
	if (caller === undefined) {
		kept.clear();
		return;
	}
	const prefix = `${caller}\u0000`;
	for (const key of kept.keys()) {
		if (key.startsWith(prefix)) kept.delete(key);
	}
}

appSession.onSessionEnded((caller) => {
	clearDraftsForCaller(caller);
});
