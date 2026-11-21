import { loadOrgView } from '$lib/modules/organization/load';

// The team page's dialogs (move, rename, delete, hire or add) work on the organization's structure. Every
// `invalidate('app:org')` reruns this load; `loads` counts the runs so the page re-reads the team and its tab.
let loads = 0;

export async function load({ depends, parent, params }) {
	depends('app:org');
	const { connection } = await parent();
	const { snapshot } = await loadOrgView(connection.organization.id);
	return { snapshot, teamId: params.teamId, loads: ++loads };
}
