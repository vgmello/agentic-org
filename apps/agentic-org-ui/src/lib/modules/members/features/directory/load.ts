// What the Members route loads besides the directory itself: the organization's structure, used only by the Add person
// flow. The production web API does not serve it yet (`unavailable`); the directory must still show its own state
// then, so this read answers `null` and the page leaves Add a person out instead of failing.
import { error } from '@sveltejs/kit';
import { toAppError } from '$lib/modules/offices/office-api';
import { orgApi } from '$lib/modules/organization/api';
import { noteShown } from '$lib/modules/organization/live';
import type { OrgSnapshot } from '$lib/proto';

type ReadView = (organizationId: string) => ReturnType<typeof orgApi.view>;

/**
 * Counts the route's loads. Every `invalidate('app:org')` (Add a person from New, a hire, a live office change)
 * reruns the route's load; the directory watches this number and re-reads its rows, so an open page shows the
 * change without a reload (offices-and-teams-ux §7.1), even while the snapshot itself is unavailable.
 */
let loads = 0;

export async function loadDirectory(
	organizationId: string,
	read: ReadView = (id) => orgApi.view(id)
): Promise<{ snapshot: OrgSnapshot | null; loads: number }> {
	const load = ++loads;
	try {
		const view = await read(organizationId);
		noteShown(organizationId, view.snapshot.position);
		return { snapshot: view.snapshot, loads: load };
	} catch (e) {
		const failure = toAppError(e);
		if (failure.source === 'api' && failure.code === 'unavailable')
			return { snapshot: null, loads: load };
		// Anything else fails the page as every org page does (loadOrgView), with the real message.
		error(failure.source === 'api' && failure.code === 'not_found' ? 404 : 500, failure.message);
	}
}
