import { loadOrgView } from '$lib/org/load';

// The team page's dialogs (move, rename, delete, hire or add) work on the organization's structure.
export async function load({ depends, parent, params }) {
	depends('app:org');
	const { connection } = await parent();
	const { snapshot } = await loadOrgView(connection.organization.id);
	return { snapshot, teamId: params.teamId };
}
