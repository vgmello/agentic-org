/**
 * Keyboard shortcut & overlay guard helpers for `ExecutiveCommandBar.svelte`.
 */

const BLOCKING_OVERLAY_QUERY =
	'[role="menu"], [role="dialog"]:not(.work-reading), [role="alertdialog"], [role="listbox"], aside[aria-label="Thread"], aside[aria-label="Members"], [data-chat-panel], [data-slot="sheet-content"]';

const BLOCKING_TARGET_CLOSEST =
	'[role="menu"], [role="dialog"], [role="alertdialog"], [role="listbox"], aside[aria-label], [data-chat-panel], [data-slot="sheet-content"]';

export function hasBlockingOverlay(target: EventTarget | null): boolean {
	if (document.querySelector(BLOCKING_OVERLAY_QUERY)) return true;
	return target instanceof HTMLElement && Boolean(target.closest(BLOCKING_TARGET_CLOSEST));
}

export function isChatHashShortcut(event: KeyboardEvent, isChat: boolean): boolean {
	if (!isChat || event.key !== '#' || event.metaKey) return false;
	const altGr = Boolean(event.getModifierState?.('AltGraph'));
	return altGr || (!event.ctrlKey && !event.altKey);
}
