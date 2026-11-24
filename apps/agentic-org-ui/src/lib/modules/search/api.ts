import { unavailableCall } from '$lib/platform/unavailable-api';
import type { SearchPageResults } from './types';

/** The results page behind "See all results" and "Why is…" knowledge search (WHY-4). */
export interface SearchPageApi {
	results(organizationId: string, query: string): Promise<SearchPageResults>;
}

// Not served by the web API yet. Under `bun run dev:mock`, e2e/fakes/modules/search.ts stands in for this file.
export const searchApi: SearchPageApi = { results: unavailableCall };
