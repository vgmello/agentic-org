import { toAppError } from '$lib/office/api';
import { orgApi } from '$lib/org/api';

/** What the agent counts need: the organization's structure, or why it couldn't be read. */
export type Counts = { snapshot: Awaited<ReturnType<typeof orgApi.snapshot>>; sample: boolean } | { error: string };

export async function load({ depends, parent }) {
	depends('app:offices');
	const { connection } = await parent(); // the offices come with the connection (organization.offices)
	// The list doesn't wait for the counts, and a failure to count shows in the page, not as an error page.
	const counts: Promise<Counts> = orgApi
		.snapshot()
		.then((snapshot) => ({ snapshot, sample: orgApi.sample }))
		.catch((e) => ({ error: toAppError(e).message }));
	return { offices: connection.organization.offices, counts };
}
