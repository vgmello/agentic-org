import { afterEach, describe, expect, it, vi } from 'vitest';
import { SidebarDockController } from './sidebar-dock.svelte';

afterEach(() => {
	vi.useRealTimers();
});

describe('SidebarDockController', () => {
	it('expands on hover/focus/menu/keyMode when auto-hide is enabled and clears timers on dispose', () => {
		vi.useFakeTimers();
		let navCollapsed = false;
		let keyMode = false;
		const dock = new SidebarDockController({
			isSidebarKeyMode: () => keyMode,
			getNavCollapsed: () => navCollapsed,
			toggleNavCollapsed: () => {
				navCollapsed = !navCollapsed;
			}
		});

		expect(dock.pinned).toBe(true);
		expect(dock.collapsed).toBe(false);

		dock.togglePin();
		expect(dock.pinned).toBe(false);
		expect(dock.collapsed).toBe(true);

		dock.onPointerEnter();
		expect(dock.collapsed).toBe(false);

		dock.onPointerLeave();
		expect(dock.collapsed).toBe(false);
		vi.advanceTimersByTime(240);
		expect(dock.collapsed).toBe(true);

		keyMode = true;
		expect(dock.collapsed).toBe(false);
		keyMode = false;
		expect(dock.collapsed).toBe(true);

		dock.openForKeyMode();
		expect(dock.focused).toBe(true);
		expect(dock.collapsed).toBe(false);

		dock.onMenuOpenChange(true, null);
		expect(dock.menuOpen).toBe(true);
		dock.onMenuOpenChange(false, null);
		expect(dock.menuOpen).toBe(false);

		// Pending leave timer must not fire after dispose().
		dock.onPointerEnter();
		dock.onPointerLeave();
		dock.dispose();
		vi.advanceTimersByTime(500);
		expect(dock.hovered).toBe(true);
	});
});
