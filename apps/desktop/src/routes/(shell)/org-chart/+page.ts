import { orgApi } from '$lib/org/api';

export async function load({ depends, parent }) {
	depends('app:org');
	const { connection } = await parent();
	return await orgApi.view(connection.organization.id);
}
