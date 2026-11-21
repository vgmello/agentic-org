import { appSession } from '$lib/platform/auth/app-session.svelte';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { orgApi } from '$lib/modules/organization/api';
import { createHttpShellApi } from './http-api';
import type { SearchResults, ShellNavigation } from './types';

/** What the app shell reads for itself: the navigation's counts and lists, and top-bar search. */
export interface ShellApi {
	navigation(organizationId: string): Promise<ShellNavigation>;
	/** Up to five hits per group for `query` (already trimmed, never empty). */
	search(organizationId: string, query: string): Promise<SearchResults>;
}

function signedInSession() {
	return appSession.state.kind === 'signed-in' ? appSession.state.session : undefined;
}

const gatewayShellApi = createHttpShellApi({
	http: {
		read: (path, signal) => appSession.controller().read(path, signal),
		submit: (path, body, signal) => appSession.controller().submit(path, body, signal)
	},
	orgApi
});

// Production shell API backed by live org snapshot and projects routes.
// Under `bun run dev:mock`, e2e/fakes/modules/shell.ts stands in for this file.
export const shellApi: ShellApi = Object.freeze<ShellApi>({
	navigation: (orgId) =>
		signedInSession() ? gatewayShellApi.navigation(orgId) : unavailableCall(),
	search: (orgId, query) =>
		signedInSession() ? gatewayShellApi.search(orgId, query) : unavailableCall()
});
