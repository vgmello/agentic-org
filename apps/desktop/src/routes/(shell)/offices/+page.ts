import { orgApi } from '$lib/org/api';
import { officesApi } from '$lib/offices/api';

export async function load({ depends, parent }) {
	depends('app:offices');
	await parent(); // the connection (layout data) is part of what this page shows
	const [offices, snapshot] = await Promise.all([officesApi.list(), orgApi.snapshot()]);
	return { offices, snapshot };
}
