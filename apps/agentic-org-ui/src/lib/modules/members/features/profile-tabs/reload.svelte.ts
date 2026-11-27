// A tab's read that follows the profile: it reloads after a change made from the header (Run now, Assign task…).
import { Resource } from '$lib/components/resource.svelte';
import { profileContext } from '../profile/context.svelte';

export function createTabChangeSync(resource: Resource<unknown>, initialChanges: number) {
	let seen = initialChanges;
	return (nextChanges: number): boolean => {
		if (nextChanges === seen) return false;
		seen = nextChanges;
		void resource.reload();
		return true;
	};
}

export function tabResource<T>(
	load: (organizationId: string, memberId: string, signal?: AbortSignal) => Promise<T>
) {
	const ctx = profileContext();
	const resource = new Resource((signal) => load(ctx.organizationId, ctx.profile.id, signal));
	const sync = createTabChangeSync(resource, ctx.changes);
	$effect(() => {
		sync(ctx.changes);
	});
	return resource;
}
