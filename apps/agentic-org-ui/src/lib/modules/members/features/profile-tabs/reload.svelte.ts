// A tab's read that follows the profile: it reloads after a change made from the header (Run now, Assign task…).
import { Resource } from '$lib/components/resource.svelte';
import { profileContext } from '../profile/context.svelte';

export function tabResource<T>(load: (organizationId: string, memberId: string) => Promise<T>) {
	const ctx = profileContext();
	const resource = new Resource(() => load(ctx.organizationId, ctx.profile.id));
	let seen = ctx.changes;
	$effect(() => {
		if (ctx.changes !== seen) {
			seen = ctx.changes;
			void resource.reload();
		}
	});
	return resource;
}
