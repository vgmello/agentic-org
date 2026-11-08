import { orgApi } from '$lib/org/api';

export async function load({ depends }) {
	depends('app:org');
	return { snapshot: await orgApi.snapshot(), sample: orgApi.sample };
}
