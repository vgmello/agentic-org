import { appSession } from '$lib/platform/auth/app-session.svelte';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { createHttpUpdatesApi } from './http-api';
import type { UpdatesOverview } from './types';

/**
 * What Organization settings › Updates reads. Read only: an upgrade is an explicit local command on the instance's
 * machine (server-installation-and-access-ux §5, SRV-15), and the browser offers no restart-to-update before a
 * supported distribution and update flow exists (app-shell-ux §0.1).
 */
export interface UpdatesApi {
	overview(organizationId: string): Promise<UpdatesOverview>;
}

function signedInSession() {
	return appSession.state.kind === 'signed-in' ? appSession.state.session : undefined;
}

const gatewayUpdatesApi = createHttpUpdatesApi({
	http: {
		read: (path, signal) => appSession.controller().read(path, signal),
		submit: (path, body, signal) => appSession.controller().submit(path, body, signal)
	}
});

// Production updates API backed by `/api/v1/instance` and `/api/v1/organizations/{org}/offices`.
// Under `bun run dev:mock`, e2e/fakes/modules/updates.ts stands in for this file.
export const updatesApi: UpdatesApi = Object.freeze<UpdatesApi>({
	overview: (organizationId) =>
		signedInSession() ? gatewayUpdatesApi.overview(organizationId) : unavailableCall()
});
