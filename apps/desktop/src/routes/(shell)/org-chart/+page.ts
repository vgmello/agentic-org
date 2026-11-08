import { loadOrgView } from '$lib/org/load';

export async function load({ depends, parent }) {
	depends('app:org');
	const { connection } = await parent();
	return await loadOrgView(connection.organization.id);
}
