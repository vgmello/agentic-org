import { getContext, setContext } from 'svelte';
import { createDraftStore, type DraftStore } from './store';

const draftStoreKey = Symbol('Chat draft store');

export function provideDraftStore(currentOrganizationId: () => string): DraftStore {
	return setContext(draftStoreKey, createDraftStore(currentOrganizationId));
}

export function useDraftStore(): DraftStore {
	const store = getContext<DraftStore | undefined>(draftStoreKey);
	if (!store) throw new Error('Chat drafts require a store from the app shell.');
	return store;
}
