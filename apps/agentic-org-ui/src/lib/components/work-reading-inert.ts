/** Pure DOM helpers for modal inertness and focus-trap management in WorkReadingPanel. */

const FOCUSABLE_SELECTOR =
	'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

const EXTERNAL_MODAL_SELECTOR =
	'[role="dialog"]:not(.work-reading), [role="alertdialog"], [role="menu"], [data-bits-portal]';

function isSonnerLiveRegion(el: HTMLElement): boolean {
	return (
		el.tagName === 'SECTION' &&
		el.getAttribute('aria-live') === 'polite' &&
		el.getAttribute('aria-relevant') === 'additions text'
	);
}

function shouldSkipInertSibling(el: HTMLElement): boolean {
	if (el.classList.contains('work-panel-backdrop')) return true;
	if (el.hasAttribute('data-bits-portal') || el.hasAttribute('data-sonner-toaster')) return true;
	if (isSonnerLiveRegion(el) || Boolean(el.querySelector('[data-sonner-toaster]'))) return true;
	const role = el.getAttribute('role');
	return role === 'dialog' || role === 'alertdialog';
}

export function collectInertSiblings(panelEl: HTMLElement | null): HTMLElement[] {
	const inertTargets: HTMLElement[] = [];
	let cur: HTMLElement | null = panelEl;
	while (cur && cur !== document.body && cur.parentElement) {
		for (const sibling of cur.parentElement.children) {
			if (
				sibling instanceof HTMLElement &&
				sibling !== cur &&
				!inertTargets.includes(sibling) &&
				!shouldSkipInertSibling(sibling)
			) {
				inertTargets.push(sibling);
			}
		}
		cur = cur.parentElement;
	}
	return inertTargets;
}

export function focusableInside(container: HTMLElement): HTMLElement[] {
	return [...container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)].filter(
		(el) => el.offsetParent !== null && !el.closest('[inert]')
	);
}

export function isExternalModalTarget(target: EventTarget | null): boolean {
	return target instanceof Element && Boolean(target.closest(EXTERNAL_MODAL_SELECTOR));
}

export function trapPanelTabKey(event: KeyboardEvent, panelEl: HTMLElement | null): void {
	if (event.key !== 'Tab' || event.altKey || event.ctrlKey || event.metaKey || !panelEl) return;
	if (
		document.querySelector(
			'[role="dialog"]:not(.work-reading), [role="alertdialog"], [role="menu"]'
		)
	) {
		return;
	}
	const items = focusableInside(panelEl);
	if (items.length === 0) return;
	const first = items[0];
	const last = items[items.length - 1];
	const active = document.activeElement;
	if (event.shiftKey) {
		if (active === first || !panelEl.contains(active)) {
			event.preventDefault();
			last.focus();
		}
	} else if (active === last || !panelEl.contains(active)) {
		event.preventDefault();
		first.focus();
	}
}
