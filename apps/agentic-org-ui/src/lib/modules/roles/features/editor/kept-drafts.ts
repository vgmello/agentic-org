// Unsaved editor text kept when the editor closes without a deliberate Cancel: browser Back with the editor open
// (app-shell-ux §6: Back closes the modal only) or leaving the page while it is open. §6a keeps input on failure
// and disconnect; this keeps it across Back too. Reopening the same editor restores it. Memory only, for this tab:
// nothing is written to storage, so a reload or sign-out drops it.
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
}

const kept = new Map<string, KeptDraft>();

/** One editor per organization and role; a new role has one slot per organization. */
export const draftKey = (organizationId: string, roleId?: string) =>
	`${organizationId}\u0000${roleId ?? ''}`;

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
