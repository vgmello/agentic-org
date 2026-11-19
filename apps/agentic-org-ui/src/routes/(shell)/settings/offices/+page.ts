import { loadOfficeCounts } from '$lib/modules/offices';

export async function load({ depends, parent }) {
	depends('app:offices');
	const { connection } = await parent(); // the offices come with the connection (organization.offices)
	// The list doesn't wait for the counts (see loadOfficeCounts).
	return {
		offices: connection.organization.offices,
		counts: loadOfficeCounts(connection.organization.id)
	};
}
