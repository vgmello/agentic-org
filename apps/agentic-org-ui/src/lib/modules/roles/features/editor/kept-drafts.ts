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

const kept = new Map<string, KeptDraft>();

/** One editor per caller, organization and role; a new role has one slot per caller+organization. */
export const draftKey = (organizationId: string, roleId?: string, caller = '') =>
	`${caller}\u0000${organizationId}\u0000${roleId ?? ''}`;

export function keepDraft(key: string, draft: KeptDraft) {
	kept.set(key, structuredClone(draft));
}

/** The kept draft, if any; it stays kept until saved or discarded. */
export function keptDraft(key: string): KeptDraft | undefined {
	const draft = kept.get(key);
	return draft ? structuredClone(draft) : undefined;
}

export function discardDraft(key: string) {
	kept.delete(key);
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
