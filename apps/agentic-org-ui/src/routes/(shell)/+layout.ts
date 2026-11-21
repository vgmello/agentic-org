import { redirect } from '@sveltejs/kit';
import { signInHref } from '$lib/platform/auth/return-target';
import { areaCapability } from '$lib/platform/shell/capabilities';
import { loadConnections } from '$lib/modules/offices/load';
import { toAppError } from '$lib/proto';

// The shell needs a remembered connection; unavailable routes skip boot/API calls so the root
// layout renders Unavailable with zero network requests.
export async function load({ depends, url }) {
	depends('app:connections');
	if (!areaCapability(url.pathname).available) {
		return { connection: undefined as never };
	}
	try {
		const connections = await loadConnections();
		if (connections.length === 0) redirect(307, '/welcome');
		// The list is oldest first: show the most recently created or opened organization.
		return { connection: connections[connections.length - 1] };
	} catch (e) {
		if (e && typeof e === 'object' && 'status' in e && 'location' in e) throw e;
		if (toAppError(e).code === 'not_authorized') {
			redirect(307, signInHref(`${url.pathname}${url.search}`, url.origin));
		}
		throw e;
	}
}
