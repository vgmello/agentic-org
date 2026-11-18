import { unavailableCall } from '$lib/platform/unavailable-api';
import type { SearchResults } from '$lib/modules/shell/types';

/** The results page behind "See all results": every match per group, not the top-bar's first five. */
export interface SearchPageApi {
	results(organizationId: string, query: string): Promise<SearchResults>;
}

// Not served by the web API yet. Under `bun run dev:mock`, e2e/fakes/modules/search.ts stands in for this file.
export const searchApi: SearchPageApi = { results: unavailableCall };
