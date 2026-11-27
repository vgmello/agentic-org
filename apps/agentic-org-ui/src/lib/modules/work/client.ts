import { newId } from '$lib/modules/organization/commands';
import type { WorkAccepted } from '$lib/proto';
import { appSession } from '$lib/platform/auth/app-session.svelte';
import { unavailableCall } from '$lib/platform/unavailable-api';
import type { WorkApi } from './api';
import type { WorkCall } from './types';
import { createHttpWorkApi } from './http-api';

function signedInSession() {
	return appSession.state.kind === 'signed-in' ? appSession.state.session : undefined;
}

const gatewayWorkApi = createHttpWorkApi({
	http: {
		read: (path, signal) => appSession.controller().read(path, signal)
	}
});

// Production work reads use `/api/v1/organizations/{org}/tasks` when signed in and fail closed
// when signed out; write/detail mutations stay unavailable until the full Work adapter lands.
export const workApi: WorkApi = Object.freeze<WorkApi>({
	snapshot: (organizationId, projectId, closedSince, signal) =>
		signedInSession()
			? gatewayWorkApi.snapshot(organizationId, projectId, closedSince, signal)
			: unavailableCall(),
	get: unavailableCall,
	command: unavailableCall,
	activity: unavailableCall,
	similar: unavailableCall,
	requests: false
});

export type WorkDraft = WorkCall extends infer C
	? C extends unknown
		? Omit<C, 'change_id'>
		: never
	: never;

/** Bind one explicit API; test composition uses the same sender without a production fallback. */
export const workSender =
	(api: WorkApi) =>
	(organizationId: string) =>
	(draft: WorkDraft): Promise<WorkAccepted> =>
		api.command(organizationId, { ...draft, change_id: newId() } as WorkCall);

export const sender = workSender(workApi);
