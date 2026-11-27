import { tick } from 'svelte';

export async function focusDestinationHeading() {
	await tick();
	const heading =
		document.querySelector<HTMLElement>('#shell-page-title') ??
		document.querySelector<HTMLElement>('main h1') ??
		document.querySelector<HTMLElement>('main h2');
	if (heading) {
		if (!heading.hasAttribute?.('tabindex')) heading.tabIndex = -1;
		heading.focus({ preventScroll: true });
	}
}
