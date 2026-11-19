// Moves focus to a page heading by id (app-shell-ux §6a): a heading takes focus as a place to read from, without a
// control ring, so it becomes programmatically focusable first.
export function focusHeading(id: string): boolean {
	const heading = document.getElementById(id);
	if (!heading) return false;
	heading.tabIndex = -1;
	heading.focus({ preventScroll: true });
	return true;
}

/**
 * After a client navigation the router resets focus and the shell focuses the top bar's h1 a tick later; this runs
 * after both, and only when nothing else has taken focus meanwhile.
 */
export function focusHeadingAfterShell(id: string) {
	setTimeout(() => {
		const active = document.activeElement;
		const idle = !active || active === document.body || active.matches('main h1, header h1');
		if (idle) focusHeading(id);
	}, 0);
}
