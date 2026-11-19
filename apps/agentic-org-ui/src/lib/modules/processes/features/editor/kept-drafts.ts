// Text typed in the editor and left unsaved (Back, a link, the navigation) is kept for this tab's session and put back
// when the same editor opens again (app-shell-ux §6a: keep input). Cancel and a save discard it.
import type { DocumentDraft, ProcessDocument } from '../../types';

export interface KeptDraft {
	draft: DocumentDraft;
	expectedRevision: number;
	latest?: ProcessDocument;
}

const kept = new Map<string, KeptDraft>();

export const draftSlot = (organizationId: string, what: string) => `${organizationId}/${what}`;
export const keptDraft = (slot: string) => kept.get(slot);
export const keepDraft = (slot: string, value: KeptDraft) => void kept.set(slot, value);
export const discardDraft = (slot: string) => void kept.delete(slot);
