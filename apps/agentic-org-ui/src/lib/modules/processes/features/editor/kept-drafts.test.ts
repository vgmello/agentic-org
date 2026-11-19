import { describe, expect, it } from 'vitest';
import { discardDraft, draftSlot, keepDraft, keptDraft } from './kept-drafts';

describe('kept editor drafts', () => {
	it('keeps typed text per organization and editor, and forgets it when discarded', () => {
		const slot = draftSlot('org-1', 'doc-1');
		const draft = {
			kind: 'process' as const,
			title: 'Code review',
			status: 'draft' as const,
			overridable: true,
			bodyMarkdown: 'text'
		};
		keepDraft(slot, { draft, expectedRevision: 2 });
		expect(keptDraft(slot)?.draft.title).toBe('Code review');
		expect(keptDraft(draftSlot('org-2', 'doc-1'))).toBeUndefined();
		discardDraft(slot);
		expect(keptDraft(slot)).toBeUndefined();
	});
});
