import { appSession } from '$lib/platform/auth/app-session.svelte';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { orgApi } from '$lib/modules/organization/api';
import { workItemsApi } from '$lib/modules/workitems/api';
import { createHttpSearchApi } from './http-api';
import type { SearchPageResults } from './types';

/** The results page behind "See all results" and "Why is…" knowledge search (WHY-4). */
export interface SearchPageApi {
	results(organizationId: string, query: string): Promise<SearchPageResults>;
}

function signedInSession() {
	return appSession.state.kind === 'signed-in' ? appSession.state.session : undefined;
}

const gatewaySearchApi = createHttpSearchApi({
	http: {
		read: (path, signal) => appSession.controller().read(path, signal),
		submit: (path, body, signal) => appSession.controller().submit(path, body, signal)
	},
	orgApi,
	workItemsApi
});

// Production search API combining live projects, work items, members, and organization processes.
// Under `bun run dev:mock`, e2e/fakes/modules/search.ts stands in for this file.
export const searchApi: SearchPageApi = Object.freeze<SearchPageApi>({
	results: (organizationId, query) =>
		signedInSession() ? gatewaySearchApi.results(organizationId, query) : unavailableCall()
});
