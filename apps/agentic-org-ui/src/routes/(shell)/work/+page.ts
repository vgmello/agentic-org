import { loadOrgView } from '$lib/modules/organization/load';

export async function load({ depends, parent }) {
	depends('app:org');
	const { connection } = await parent();
	const { snapshot, sample } = await loadOrgView(connection.organization.id);
	return { connection, snapshot, sample };
}
