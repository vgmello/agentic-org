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
