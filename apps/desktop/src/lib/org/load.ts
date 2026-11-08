import { error } from '@sveltejs/kit';
import { toAppError } from '$lib/office/api';
import { orgApi } from './api';
import { noteShown } from './live';

/**
 * An organization's structure for a page. If the office can't give it (storage unreadable, the
 * organization is gone), fail the page with the real message instead of SvelteKit's generic one.
 */
export async function loadOrgView(organizationId: string) {
	try {
		const view = await orgApi.view(organizationId);
		noteShown(organizationId, view.snapshot.position);
		return view;
	} catch (e) {
		const failure = toAppError(e);
		error(failure.source === 'api' && failure.code === 'not_found' ? 404 : 500, failure.message);
	}
}
