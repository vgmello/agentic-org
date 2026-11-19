import { unavailableCall } from '$lib/platform/unavailable-api';
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

// The web API does not serve roles yet: every call refuses, and each screen says so.
// Under `bun run dev:mock`, e2e/fakes/modules/roles.ts stands in for this file.
export const rolesApi: RolesApi = {
	list: unavailableCall,
	role: unavailableCall,
	create: unavailableCall,
	update: unavailableCall,
	askHrToDraft: unavailableCall
};
