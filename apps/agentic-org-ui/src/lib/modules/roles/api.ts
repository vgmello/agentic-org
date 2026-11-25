import { appSession } from '$lib/platform/auth/app-session.svelte';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { orgApi } from '$lib/modules/organization/api';
import { createHttpRolesApi } from './http-api';
import type { Role, RoleDetail, RoleDraft, RoleEdit, RolesOverview } from './types';

/**
 * What the Roles screens read and change (roles-ux §1). Every change is authorized and revision-checked by the
 * server. Refusals reject with an AppError; a refusal about one editor field also carries `field`
 * (`name`, `key` or `body`), and a revision conflict (`conflict`) carries `latest`, the role as it is now.
 */
export interface RolesApi {
	list(organizationId: string): Promise<RolesOverview>;
	/** Rejects with `not_found` when there is no such role. */
	role(organizationId: string, roleId: string): Promise<RoleDetail>;
	create(organizationId: string, draft: RoleDraft): Promise<Role>;
	/** Writes revision `expectedRevision + 1`; rejects with `conflict` and `latest` when the role moved on. */
	update(organizationId: string, roleId: string, edit: RoleEdit): Promise<Role>;
	/** The HR lead drafts a role from a description. Nothing is created: the person reviews the draft first. */
	askHrToDraft(organizationId: string, description: string): Promise<RoleDraft>;
}

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
