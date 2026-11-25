import { expect, it } from 'vitest';
import {
	clearDraftsForCaller,
	discardDraft,
	discardDraftIfVersion,
	draftKey,
	keepDraft,
	keptDraft
} from './kept-drafts';

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

it('isolates retained drafts by caller and clears only the signed-out caller', () => {
	const alice = draftKey('org-a', undefined, 'inst-1:acct-alice');
	const bob = draftKey('org-a', undefined, 'inst-1:acct-bob');
	expect(alice).not.toBe(bob);
	keepDraft(alice, {
		name: 'Alice Draft',
		key: '',
		body: 'secret',
		expectedRevision: 0,
		attemptId: 'att-alice'
	});
	keepDraft(bob, {
		name: 'Bob Draft',
		key: '',
		body: 'other',
		expectedRevision: 0,
		attemptId: 'att-bob'
	});
	expect(keptDraft(bob)?.name).toBe('Bob Draft');
	clearDraftsForCaller('inst-1:acct-alice');
	expect(keptDraft(alice)).toBeUndefined();
	expect(keptDraft(bob)?.name).toBe('Bob Draft');
	clearDraftsForCaller('inst-1:acct-bob');
});

it('preserves hasUncertainAttempt so restoring original text still reconciles a 504 save', () => {
	const key = draftKey('org-a', 'role-swe', 'inst-1:acct-alice');
	keepDraft(key, {
		name: 'Software Engineer',
		key: 'swe',
		body: 'Original text',
		expectedRevision: 1,
		attemptId: 'att-504',
		hasUncertainAttempt: true
	});
	expect(keptDraft(key)).toMatchObject({
		attemptId: 'att-504',
		hasUncertainAttempt: true
	});
	clearDraftsForCaller('inst-1:acct-alice');
});

it('discards only the matching draft version so a late save never deletes newer unsaved text', () => {
	const key = draftKey('org-a', undefined, 'inst-1:acct-alice');
	const v1 = keepDraft(key, {
		name: 'Version 1',
		key: '',
		body: 'First',
		expectedRevision: 0,
		attemptId: 'att-shared'
	});
	const v2 = keepDraft(key, {
		name: 'Version 2',
		key: '',
		body: 'Newer unsaved text',
		expectedRevision: 0,
		attemptId: 'att-shared'
	});
	discardDraftIfVersion(key, v1);
	expect(keptDraft(key)?.name).toBe('Version 2');
	discardDraftIfVersion(key, v2);
	expect(keptDraft(key)).toBeUndefined();
});
