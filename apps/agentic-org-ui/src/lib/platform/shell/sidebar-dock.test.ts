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

	it('tracks 200%-text rail reflow via attachReflow and disconnects ResizeObserver on cleanup and dispose', () => {
		let containerWidth = 960;
		let rootFontPx = 16;
		const observed: Element[] = [];
		const disconnect = vi.fn();
		let resizeCallback: ResizeObserverCallback | undefined;

		class FakeResizeObserver {
			constructor(cb: ResizeObserverCallback) {
				resizeCallback = cb;
			}
			observe(target: Element) {
				observed.push(target);
			}
			unobserve() {}
			disconnect() {
				disconnect();
			}
		}

		const parentEl = {
			getBoundingClientRect: () => ({ width: containerWidth })
		} as unknown as HTMLElement;
		const nodeEl = {
			parentElement: parentEl
		} as unknown as HTMLElement;

		vi.stubGlobal('ResizeObserver', FakeResizeObserver);
		vi.stubGlobal('document', { documentElement: {} });
		vi.stubGlobal('getComputedStyle', () => ({ fontSize: `${rootFontPx}px` }));

		try {
			const dock = new SidebarDockController({ isSidebarKeyMode: () => false });
			const cleanup = dock.attachReflow(nodeEl);

			expect(observed).toEqual([nodeEl, parentEl]);
			// 960px at 16px root (60rem >= 40rem) stays expanded.
			expect(dock.textReflowRail).toBe(false);

			// 960px at 32px root (30rem < 40rem) switches to icon rail.
			rootFontPx = 32;
			resizeCallback?.([], {} as ResizeObserver);
			expect(dock.textReflowRail).toBe(true);

			// 1279px at 32px root (39.97rem < 40rem) stays in icon rail.
			containerWidth = 1279;
			resizeCallback?.([], {} as ResizeObserver);
			expect(dock.textReflowRail).toBe(true);

			// 1280px at 32px root (40rem >= 40rem) expands back.
			containerWidth = 1280;
			resizeCallback?.([], {} as ResizeObserver);
			expect(dock.textReflowRail).toBe(false);

			// Mobile (<640px) does not mark desktop rail reflow.
			containerWidth = 600;
			resizeCallback?.([], {} as ResizeObserver);
			expect(dock.textReflowRail).toBe(false);

			cleanup();
			expect(disconnect).toHaveBeenCalledTimes(1);

			// Re-attach and verify dispose() disconnects and blocks stale callbacks.
			dock.attachReflow(nodeEl);
			dock.dispose();
			expect(disconnect).toHaveBeenCalledTimes(2);
			containerWidth = 960;
			rootFontPx = 32;
			resizeCallback?.([], {} as ResizeObserver);
			expect(dock.textReflowRail).toBe(false);
		} finally {
			vi.unstubAllGlobals();
		}
	});
});
