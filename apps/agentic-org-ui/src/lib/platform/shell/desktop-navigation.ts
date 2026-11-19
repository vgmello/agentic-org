import { navFor } from '$lib/nav';
import { focusDestinationHeading } from './focus-destination';

/** The shell supplies SvelteKit's goto; Sidebar keeps its native link contract. */
export async function selectDesktopSection(event: MouseEvent, href: string, navigate: (href: string) => Promise<void>) {
	if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey || !navFor(href)) return;
	event.preventDefault();
	const from = window.location.href;
	try {
		if (new URL(href, from).href !== from) await navigate(href);
		await focusDestinationHeading();
	} catch {
		// SvelteKit owns route errors. Keep focus on the connected desktop link.
	}
}
