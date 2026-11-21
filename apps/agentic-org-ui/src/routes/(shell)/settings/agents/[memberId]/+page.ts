import { loadOrgView } from '$lib/modules/organization/load';

// The organization's structure, for Pause, Resume and Offboard and Offboard's preview, as on the member profile.
export async function load({ depends, parent }) {
	depends('app:org');
	const { connection } = await parent();
	return await loadOrgView(connection.organization.id);
}
