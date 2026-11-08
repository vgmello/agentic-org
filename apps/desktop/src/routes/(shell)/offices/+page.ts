import { orgApi } from '$lib/org/api';

export async function load({ depends, parent }) {
	depends('app:offices');
	const { connection } = await parent(); // the offices come with the connection (organization.offices)
	const snapshot = await orgApi.snapshot();
	return { offices: connection.organization.offices, snapshot, sample: orgApi.sample };
}
