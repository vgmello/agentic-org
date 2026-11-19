import { loadOrgView } from '$lib/org/load';

// Until Work items (plan step 5) replaces it, the Work tab shows v1's task board and bug list, which read the
// organization view as v1's project page did.
export async function load({ depends, parent }) {
	depends('app:org');
	const { connection } = await parent();
	const { snapshot } = await loadOrgView(connection.organization.id);
	return { snapshot };
}
