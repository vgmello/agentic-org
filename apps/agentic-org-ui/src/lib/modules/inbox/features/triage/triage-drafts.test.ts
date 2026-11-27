import { describe, expect, it } from 'vitest';
import { TriageDraftStore } from './triage-drafts.svelte';

describe('TriageDraftStore controller', () => {
	it('isolates drafts per organization and item and clears action drafts independently', () => {
		const store = new TriageDraftStore();
		store.setDeny('org-1', 'item-1', 'Not now');
		store.setReply('org-1', 'item-1', 'Need numbers');
		store.setComment('org-1', 'item-1', 'Local note');
		store.setDeny('org-2', 'item-1', 'Other org');

		expect(store.getDeny('org-1', 'item-1')).toBe('Not now');
		expect(store.getReply('org-1', 'item-1')).toBe('Need numbers');
		expect(store.getComment('org-1', 'item-1')).toBe('Local note');
		expect(store.getDeny('org-2', 'item-1')).toBe('Other org');

		store.clearActionDrafts('org-1', 'item-1');
		expect(store.getDeny('org-1', 'item-1')).toBe('');
		expect(store.getReply('org-1', 'item-1')).toBe('');
		expect(store.getComment('org-1', 'item-1')).toBe('Local note');

		store.clearComment('org-1', 'item-1');
		expect(store.getComment('org-1', 'item-1')).toBe('');
	});
});
