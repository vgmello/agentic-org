// A role's page reads one role: a missing role is a page state (Role not found), not a failure to retry.
import { toAppError } from '$lib/modules/offices/office-api';
import type { RoleDetail } from '../../types';

export type RoleLookup = { found: true; detail: RoleDetail } | { found: false; message: string };

export async function lookUpRole(load: () => Promise<RoleDetail>): Promise<RoleLookup> {
	try {
		return { found: true, detail: await load() };
	} catch (e) {
		const error = toAppError(e);
		if (error.code === 'not_found') return { found: false, message: error.message };
		throw e;
	}
}
