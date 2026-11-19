import { expect, it } from 'vitest';
import { discardDraft, draftKey, keepDraft, keptDraft } from './kept-drafts';

it('keeps a draft per organization and role until it is discarded', () => {
	const edit = draftKey('org-a', 'role-swe');
	const fresh = draftKey('org-a');
	expect(edit).not.toBe(draftKey('org-b', 'role-swe'));
	expect(fresh).not.toBe(edit);
	keepDraft(edit, { name: 'Lead', key: 'swe', body: 'x', expectedRevision: 3 });
	expect(keptDraft(edit)).toEqual({ name: 'Lead', key: 'swe', body: 'x', expectedRevision: 3 });
	expect(keptDraft(fresh)).toBeUndefined();
	expect(keptDraft(draftKey('org-b', 'role-swe'))).toBeUndefined();
	discardDraft(edit);
	expect(keptDraft(edit)).toBeUndefined();
});

it('hands out copies, so editing the restored text does not change what is kept', () => {
	const key = draftKey('org-a', 'role-qa');
	keepDraft(key, { name: 'QA', key: '', body: 'one', expectedRevision: 1 });
	const restored = keptDraft(key)!;
	restored.body = 'changed';
	expect(keptDraft(key)!.body).toBe('one');
	discardDraft(key);
});
