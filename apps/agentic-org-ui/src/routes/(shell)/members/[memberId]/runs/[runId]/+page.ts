import { orgApi } from '$lib/org/api';

// The member's name for the back link, read before the page draws so the link's label never changes after it shows.
// If HQ can't say, the link reads "Runs" from the start; the log itself does not depend on it.
export async function load({ parent, params }) {
	const { connection } = await parent();
	try {
		const view = await orgApi.view(connection.organization.id);
		return { memberName: view.snapshot.members.find((m) => m.id === params.memberId)?.name };
	} catch {
		return { memberName: undefined };
	}
}
