import { unavailableCall } from '$lib/platform/unavailable-api';
import type { SearchResults, ShellNavigation } from './types';

/** What the app shell reads for itself: the navigation's counts and lists, and top-bar search. */
export interface ShellApi {
	navigation(organizationId: string): Promise<ShellNavigation>;
	/** Up to five hits per group for `query` (already trimmed, never empty). */
	search(organizationId: string, query: string): Promise<SearchResults>;
}

// The web API does not serve these yet: every call refuses, and the shell shows no counts or lists.
// Under `bun run dev:mock`, e2e/fakes/modules/shell.ts stands in for this file.
export const shellApi: ShellApi = {
	navigation: unavailableCall,
	search: unavailableCall
};
