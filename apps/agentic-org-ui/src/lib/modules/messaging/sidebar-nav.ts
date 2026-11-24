export type SidebarNavDirection = 'up' | 'down' | 'home' | 'end';

export interface SidebarNavKeyLike {
	key: string;
	altKey: boolean;
	ctrlKey: boolean;
	metaKey: boolean;
	shiftKey?: boolean;
	isComposing?: boolean;
	keyCode?: number;
}

/**
 * Maps ArrowUp/ArrowDown/Home/End inside the Chat conversation directory (`poc` commit `c64b187a`).
 * Ignores modified chords (including Shift) and active IME composition (`isComposing || keyCode === 229`).
 */
export function sidebarNavAction(event: SidebarNavKeyLike): SidebarNavDirection | null {
	if (event.isComposing || event.keyCode === 229) return null;
	if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return null;
	switch (event.key) {
		case 'ArrowDown':
			return 'down';
		case 'ArrowUp':
			return 'up';
		case 'Home':
			return 'home';
		case 'End':
			return 'end';
		default:
			return null;
	}
}

/**
 * Computes the target index for linear keyboard navigation across channel/DM/A2A items,
 * clamping cleanly at the start and end of the list. Returns `-1` when no move applies.
 */
export function nextSidebarIndex(
	current: number,
	count: number,
	direction: SidebarNavDirection
): number {
	if (count <= 0 || current < 0 || current >= count) return -1;
	switch (direction) {
		case 'home':
			return 0;
		case 'end':
			return count - 1;
		case 'down':
			return current + 1 < count ? current + 1 : -1;
		case 'up':
			return current - 1 >= 0 ? current - 1 : -1;
	}
}

/** Whether the event target is a text-entry control whose arrow/home/end keys must not be hijacked. */
export function isTextEntryTarget(target: EventTarget | null): boolean {
	if (!target || typeof target !== 'object') return false;
	const el = target as { tagName?: string; isContentEditable?: boolean };
	if (el.isContentEditable) return true;
	const tag = el.tagName?.toUpperCase();
	return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT';
}
