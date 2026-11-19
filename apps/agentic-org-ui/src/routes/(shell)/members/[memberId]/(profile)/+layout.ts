import { loadOrgView } from '$lib/org/load';

// The organization's structure, for the shared Edit, Move home team and Offboard dialogs on the profile.
export async function load({ depends, parent }) {
	depends('app:org');
	const { connection } = await parent();
	return await loadOrgView(connection.organization.id);
}
