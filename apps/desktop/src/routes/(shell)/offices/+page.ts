import { loadOrgView } from '$lib/org/load';

export async function load({ parent }) {
	const { connection } = await parent(); // the offices come with the connection (organization.offices)
	const { snapshot, sample } = await loadOrgView(connection.organization.id);
	return { offices: connection.organization.offices, snapshot, sample };
}
