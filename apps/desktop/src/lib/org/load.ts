import { error } from '@sveltejs/kit';
import { toAppError } from '$lib/office/api';
import { orgApi } from './api';

/**
 * An organization's structure for a page. If the office can't give it (storage unreadable, the
 * organization is gone), fail the page with the real message instead of SvelteKit's generic one.
 */
export async function loadOrgView(organizationId: string) {
	try {
		return await orgApi.view(organizationId);
	} catch (e) {
		error(500, toAppError(e).message);
	}
}
