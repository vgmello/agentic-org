import { appSession } from '$lib/platform/auth/app-session.svelte';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { orgApi } from '$lib/modules/organization/api';
import { hasRetainedAttempt } from './features/editor/kept-drafts';
import { createHttpRolesApi } from './http-api';
import type { RolesApi } from './types';

export type { RolesApi } from './types';

function signedInSession() {
	return appSession.state.kind === 'signed-in' ? appSession.state.session : undefined;
}

const gatewayRolesApi = createHttpRolesApi({
	http: {
		read: (path, signal) => appSession.controller().read(path, signal),
		submit: (path, body, signal) => appSession.controller().submit(path, body, signal)
	},
	orgApi,
	sessionScope: () => appSession.callerScope,
	sessionGeneration: () => appSession.sessionGeneration,
	isAttemptRetained: hasRetainedAttempt,
	newEntityId: () => appSession.newOperationId(),
	newCommandId: () => appSession.newOperationId()
});

appSession.onSessionEnded((caller) => {
	gatewayRolesApi.clearCaller(caller);
});

// Production roles API backed by `/api/v1/organizations/{org}/roles*` and the organization snapshot.
// Under `bun run dev:mock`, e2e/fakes/modules/roles.ts stands in for this file.
export const rolesApi: RolesApi = Object.freeze<RolesApi>({
	list: (organizationId) =>
		signedInSession() ? gatewayRolesApi.list(organizationId) : unavailableCall(),
	role: (organizationId, roleId) =>
		signedInSession() ? gatewayRolesApi.role(organizationId, roleId) : unavailableCall(),
	create: (organizationId, draft) =>
		signedInSession() ? gatewayRolesApi.create(organizationId, draft) : unavailableCall(),
	update: (organizationId, roleId, edit) =>
		signedInSession() ? gatewayRolesApi.update(organizationId, roleId, edit) : unavailableCall(),
	askHrToDraft: (organizationId, description) =>
		signedInSession()
			? gatewayRolesApi.askHrToDraft(organizationId, description)
			: unavailableCall()
});
