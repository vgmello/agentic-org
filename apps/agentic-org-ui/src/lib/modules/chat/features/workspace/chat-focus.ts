/** Scrolls a focused chat control into view if clipped or covered by a sticky bar/panel. */
export function revealFocus(target: HTMLElement): void {
	const rect = target.getBoundingClientRect();
	const x = rect.x + rect.width / 2;
	const inset = Math.min(1, rect.height / 2);
	const scrollMargin = Number.parseFloat(getComputedStyle(target).scrollMarginBlockStart) || 0;
	const clippedByViewport = rect.top < scrollMargin || rect.bottom > innerHeight;
	const covered = ![rect.top + inset, rect.bottom - inset].every((y) =>
		target.contains(document.elementFromPoint(x, y))
	);
	if (clippedByViewport || covered) {
		target.scrollIntoView({ block: 'center', inline: 'nearest' });
	}
}

export function focusVisible(target: HTMLElement | null | undefined, compactPanels: boolean): void {
	if (!target?.isConnected) return;
	target.focus({ preventScroll: true });
	if (compactPanels) revealFocus(target);
}
