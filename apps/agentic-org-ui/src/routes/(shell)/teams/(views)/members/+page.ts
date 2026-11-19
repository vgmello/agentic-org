import { loadDirectory } from '$lib/modules/members';

// The directory reads membersApi.directory itself; the route loads only the structure Add a person needs.
export async function load({ depends, parent }) {
	depends('app:org');
	const { connection } = await parent();
	return loadDirectory(connection.organization.id);
}
