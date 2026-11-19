import { loadDirectory } from '$lib/modules/members';

// The chart reads orgChartApi itself; the route loads only the structure Add a person and Move home team need (null
// while the server doesn't serve it), and counts its loads so live changes re-read the chart.
export async function load({ depends, parent }) {
	depends('app:org');
	const { connection } = await parent();
	return loadDirectory(connection.organization.id);
}
