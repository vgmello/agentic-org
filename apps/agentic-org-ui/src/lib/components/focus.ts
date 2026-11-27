import { tick } from 'svelte';

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
	let cancelled = false;
	let timer: ReturnType<typeof setTimeout> | undefined;
	void tick().then(() => {
		if (cancelled) return;
		timer = setTimeout(() => {
			if (cancelled) return;
			const active = document.activeElement;
			const idle =
				!active ||
				active === document.body ||
				active.matches('#shell-page-title, main h1, main h2, header h1');
			if (idle) focusHeading(id);
		}, 0);
	});
	return () => {
		cancelled = true;
		clearTimeout(timer);
	};
}

/**
 * Whether the app may still move focus after an action finishes (WCAG 3.2.2): nothing is focused (the body), or focus is
 * on one of `allowed` (the control the action started from). Once the person has moved on, focus stays where they put it.
 */
export function focusIsFree(...allowed: (Element | null | undefined)[]): boolean {
	const active = document.activeElement;
	return !active || active === document.body || allowed.includes(active);
}

/** The element an event came from: what to give focus back to when whatever it opened closes. */
export function openerFrom(e: Event): HTMLElement | null {
	return e.currentTarget instanceof HTMLElement ? e.currentTarget : null;
}

/** Whether focus can go to this element now: still in the page, not disabled, not inert. */
function usable(el: HTMLElement | null | undefined): el is HTMLElement {
	return !!el && el.isConnected && !(el as HTMLButtonElement).disabled && !el.closest('[inert]');
}

const INTERACTIVE_OPENER =
	'button, a[href], [role="button"], [tabindex]:not([tabindex="-1"]), input, select, textarea, summary';
const POINTER_OPENER_MAX_AGE_MS = 1500;
let lastPointerEl: HTMLElement | null = null;
let lastPointerAt = 0;
let lastKeydownAt = 0;
let pointerTrackingInstalled = false;

function onDocumentPointerDown(event: Event): void {
	const raw = event.target;
	const target = raw instanceof Element ? raw : null;
	if (!target) return;
	// Do not overwrite the trigger button when clicking inside a dropdown/listbox menu item that will unmount;
	// refresh the trigger timestamp so reading a menu for > 1.5 s still returns focus to its trigger on WebKit.
	if (target.closest('[role="menu"], [role="listbox"]')) {
		if (usable(lastPointerEl)) lastPointerAt = Date.now();
		return;
	}
	const interactive = target.closest<HTMLElement>(INTERACTIVE_OPENER);
	if (interactive) {
		lastPointerEl = interactive;
		lastPointerAt = Date.now();
	}
}

function onDocumentKeyDown(): void {
	lastKeydownAt = Date.now();
}

/**
 * Installs capture-phase pointer/keyboard listeners on `document` so the very first pointer-opened
 * (including lazy-loaded) dialog or sheet knows which control opened it in WebKit.
 */
export function installOverlayOpenerTracking(): void {
	if (pointerTrackingInstalled || typeof document === 'undefined') return;
	pointerTrackingInstalled = true;
	document.addEventListener('pointerdown', onDocumentPointerDown, { capture: true, passive: true });
	document.addEventListener('keydown', onDocumentKeyDown, { capture: true, passive: true });
}

if (typeof document !== 'undefined') installOverlayOpenerTracking();

/** Reset tracked pointer opener state (for unit tests). */
export function resetPointerOpenerTrackingForTests(): void {
	lastPointerEl = null;
	lastPointerAt = 0;
	lastKeydownAt = 0;
	pointerTrackingInstalled = false;
}

function isActiveOpener(el: Element | null, within?: Element | null): el is HTMLElement {
	return (
		el instanceof HTMLElement &&
		el !== document.body &&
		el !== document.documentElement &&
		usable(el) &&
		!el.closest('[role="menu"], [role="listbox"]') &&
		!within?.contains(el)
	);
}

function isRecentPointerOpener(el: HTMLElement | null, within?: Element | null): el is HTMLElement {
	return (
		!!el &&
		Date.now() - lastPointerAt <= POINTER_OPENER_MAX_AGE_MS &&
		usable(el) &&
		!within?.contains(el)
	);
}

/**
 * Captures the control that opened an overlay across both keyboard and WebKit mouse interactions.
 * WebKit leaves `document.activeElement` on `document.body` (or a previously focused heading/control)
 * after a pointer click on a button, so when the latest interaction was a pointer click we prefer
 * the clicked interactive control recorded on `pointerdown`.
 */
export function captureOverlayOpener(within?: Element | null): HTMLElement | null {
	if (typeof document === 'undefined') return null;
	installOverlayOpenerTracking();
	if (
		lastPointerAt > 0 &&
		lastPointerAt >= lastKeydownAt &&
		isRecentPointerOpener(lastPointerEl, within)
	) {
		return lastPointerEl;
	}
	if (isActiveOpener(document.activeElement, within)) return document.activeElement;
	return isRecentPointerOpener(lastPointerEl, within) ? lastPointerEl : null;
}

/**
 * Give focus back to what opened a menu or dialog (never to `document.activeElement`: WebKit does not focus a button
 * on a mouse click, so after a pointer open there is nothing to read back). Focus only moves when it has nowhere
 * useful to be: on the page itself, or inside `within` (the closing content). A person who has already moved on,
 * to another control, keeps it. When the opener is gone or cannot take focus, `fallback` is tried.
 */
export async function returnFocus(
	opener: HTMLElement | null | undefined,
	options: { within?: Element | null; fallback?: () => HTMLElement | null } = {}
): Promise<void> {
	await tick();
	const active = document.activeElement;
	if (
		active &&
		active !== document.body &&
		active !== document.documentElement &&
		!options.within?.contains(active)
	)
		return;
	if (usable(opener)) {
		opener.focus({ preventScroll: true });
		if (document.activeElement === opener) return;
	}
	const next = options.fallback?.();
	if (usable(next)) next.focus({ preventScroll: true });
}
