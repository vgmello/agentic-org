// Where the Roles screens live (app-shell-ux §0.1: Organization settings › Roles).
export const ROLES_URL = '/settings/roles';

/** A role's page, by its stable ID. */
export const roleHref = (roleId: string) => `${ROLES_URL}/${encodeURIComponent(roleId)}`;
