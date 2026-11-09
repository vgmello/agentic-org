import { SvelteSet } from 'svelte/reactivity';

// The agents the last import brought in, shown highlighted on the org chart for a little while (IMP-7).
export const arrived = new SvelteSet<string>();

let timer: ReturnType<typeof setTimeout> | undefined;

/** Highlight these members now; the highlight fades after `ms`. */
export function markArrived(ids: Iterable<string>, ms = 20_000): void {
	arrived.clear();
	for (const id of ids) arrived.add(id);
	clearTimeout(timer);
	timer = setTimeout(() => arrived.clear(), ms);
}
