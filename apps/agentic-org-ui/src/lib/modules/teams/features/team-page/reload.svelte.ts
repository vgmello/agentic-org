// Keeps a team page read current (offices-and-teams-ux §7.1): every `invalidate('app:org')` — a live office change,
// New › Add a person, a hire, this page's own actions — reruns the route's load, which counts itself (`loads`). A read
// re-runs when that count moves, keeping what it shows until the new answer arrives. Its first load is its own.
import { untrack } from 'svelte';
import type { Resource } from '$lib/components/resource.svelte';

export function createChangeReloader(resource: Resource<unknown>, initial: number) {
	let seen = initial;
	return (next: number): boolean => {
		if (next === seen) return false;
		seen = next;
		void resource.reload();
		return true;
	};
}

export function reloadOnChange(resource: Resource<unknown>, loads: () => number) {
	const sync = createChangeReloader(resource, untrack(loads));
	$effect(() => {
		const load = loads();
		// Synchronising with the server, not deriving: the read starts here and its answer lands in the Resource.
		untrack(() => {
			sync(load);
		});
	});
}
