import { tick } from 'svelte';

export async function focusDestinationHeading() {
	await tick();
	const heading = document.querySelector<HTMLElement>('main h1');
	if (heading) {
		heading.tabIndex = -1;
		heading.focus({ preventScroll: true });
	}
}
