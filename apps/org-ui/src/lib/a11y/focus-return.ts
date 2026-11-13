import { tick } from 'svelte';

/** The element an event came from: what to give focus back to when whatever it opened closes. */
export function openerFrom(e: Event): HTMLElement | null {
	return e.currentTarget instanceof HTMLElement ? e.currentTarget : null;
}

/** Whether focus can go to this element now: still in the page, not disabled, not inert. */
function usable(el: HTMLElement | null | undefined): el is HTMLElement {
	return !!el && el.isConnected && !(el as HTMLButtonElement).disabled && !el.closest('[inert]');
}

/**
 * Give focus back to what opened a menu or dialog (never to `document.activeElement`: WebKit does not focus a button
 * on a mouse click, so after a pointer open there is nothing to read back). Focus only moves when it has nowhere
 * useful to be: on the page itself, or inside `within` (the closing content). A person who has already moved on,
 * to another control, keeps it. When the opener is gone or cannot take focus, `fallback` is tried.
 */
export async function returnFocus(opener: HTMLElement | null | undefined, options: { within?: Element | null; fallback?: () => HTMLElement | null } = {}): Promise<void> {
	await tick();
	const active = document.activeElement;
	if (active && active !== document.body && active !== document.documentElement && !options.within?.contains(active)) return;
	if (usable(opener)) {
		opener.focus({ preventScroll: true });
		if (document.activeElement === opener) return;
	}
	const next = options.fallback?.();
	if (usable(next)) next.focus({ preventScroll: true });
}
