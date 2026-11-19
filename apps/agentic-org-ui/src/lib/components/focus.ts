// Removing an item focuses the next one, else the previous one, else a fallback, never the body (app-shell-ux §6a).
const FOCUSABLE =
	'button:not([disabled]), a[href], input:not([disabled]), select:not([disabled]), textarea:not([disabled])';

/** Call before the item goes; the returned function, called after the list has updated, moves focus. */
export function neighbourFocus(
	opener: HTMLElement | null | undefined,
	fallback?: () => HTMLElement | null | undefined
): () => Promise<void> {
	const row = opener?.closest('li, tr');
	const next = (row?.nextElementSibling ?? row?.previousElementSibling) as
		HTMLElement | null | undefined;
	return async () => {
		await new Promise((r) => setTimeout(r, 0));
		const target = next?.isConnected ? next.querySelector<HTMLElement>(FOCUSABLE) : undefined;
		(target ?? fallback?.() ?? document.querySelector<HTMLElement>('main h2, h2'))?.focus();
	};
}

/**
 * Moves focus to a page heading by id (app-shell-ux §6a): a heading takes focus as a place to read from, so it becomes
 * programmatically focusable first. The page hides its ring (`[&_h2:focus]:outline-none` or the heading's class).
 */
export function focusHeading(id: string): boolean {
	const heading = document.getElementById(id);
	if (!heading) return false;
	heading.tabIndex = -1;
	heading.focus({ preventScroll: true });
	return true;
}

/**
 * After a client navigation the router resets focus and the shell focuses the top bar's h1 a tick later; this runs
 * after both, and only when nothing else has taken focus meanwhile. Returns a function that cancels it.
 */
export function focusHeadingAfterShell(id: string): () => void {
	const timer = setTimeout(() => {
		const active = document.activeElement;
		const idle = !active || active === document.body || active.matches('main h1, header h1');
		if (idle) focusHeading(id);
	}, 0);
	return () => clearTimeout(timer);
}

/**
 * Whether the app may still move focus after an action finishes (WCAG 3.2.2): nothing is focused (the body), or focus is
 * on one of `allowed` (the control the action started from). Once the person has moved on, focus stays where they put it.
 */
export function focusIsFree(...allowed: (Element | null | undefined)[]): boolean {
	const active = document.activeElement;
	return !active || active === document.body || allowed.includes(active);
}
