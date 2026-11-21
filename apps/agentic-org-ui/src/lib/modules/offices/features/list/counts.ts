// How many agents each office holds comes from the organization's structure, read beside the list: the list never
// waits for it, and a failure to count shows in the page, not as an error page.
import { toAppError } from '$lib/proto';
import { orgApi } from '$lib/modules/organization/api';
import type { OrgSnapshot } from '$lib/proto';

/** The organization's structure, or why it couldn't be read. */
export type Counts = { snapshot: OrgSnapshot; sample: boolean } | { error: string };

export const loadOfficeCounts = (organizationId: string): Promise<Counts> =>
	orgApi
		.view(organizationId)
		.then(({ snapshot, sample }) => ({ snapshot, sample }))
		.catch((e) => ({ error: toAppError(e).message }));
