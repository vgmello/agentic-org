import { uiPrefs } from '$lib/platform/workspace/ui-preferences.svelte';

export interface SidebarDockOptions {
	isSidebarKeyMode: () => boolean;
	getNavCollapsed?: () => boolean;
	toggleNavCollapsed?: () => void;
}

/**
 * Manages desktop sidebar auto-hide hover/focus/menu timers and pinned rail expansion.
 * Ensures every timer and requestAnimationFrame is cancelled cleanly on `dispose()`.
 */
export class SidebarDockController {
	readonly #opts: SidebarDockOptions;
	hovered = $state(false);
	focused = $state(false);
	menuOpen = $state(false);
	textReflowRail = $state(false);

	#hoverTimer: ReturnType<typeof setTimeout> | undefined;
	#focusTimer: ReturnType<typeof setTimeout> | undefined;
	#focusRaf: number | undefined;
	#reflowObserver: ResizeObserver | undefined;
	#disposed = false;

	constructor(options: SidebarDockOptions) {
		this.#opts = options;
	}

	#navCollapsed(): boolean {
		return this.#opts.getNavCollapsed ? this.#opts.getNavCollapsed() : uiPrefs.navCollapsed;
	}

	get pinned(): boolean {
		return !this.#navCollapsed();
	}

	get collapsed(): boolean {
		return (
			this.#navCollapsed() &&
			!this.hovered &&
			!this.focused &&
			!this.menuOpen &&
			!this.#opts.isSidebarKeyMode()
		);
	}

	#cancelFocusRaf(): void {
		if (this.#focusRaf !== undefined && typeof cancelAnimationFrame !== 'undefined') {
			cancelAnimationFrame(this.#focusRaf);
			this.#focusRaf = undefined;
		}
	}

	#syncFocusContainment(container: HTMLElement | null): void {
		if (this.#disposed || this.menuOpen) return;
		const active = typeof document !== 'undefined' ? document.activeElement : null;
		this.focused = Boolean(active instanceof Node && container?.contains(active));
	}

	onMenuOpenChange(open: boolean, container: HTMLElement | null): void {
		clearTimeout(this.#focusTimer);
		this.#cancelFocusRaf();
		this.menuOpen = open;
		if (!open && !this.#disposed) {
			// Recompute focus containment after Bits UI focus restoration or outside click settles
			// without cancelling any pending pointer-leave collapse timer.
			queueMicrotask(() => this.#syncFocusContainment(container));
			if (typeof requestAnimationFrame !== 'undefined') {
				this.#focusRaf = requestAnimationFrame(() => {
					this.#focusRaf = undefined;
					this.#syncFocusContainment(container);
				});
			}
		}
	}

	onPointerEnter(): void {
		clearTimeout(this.#hoverTimer);
		this.hovered = true;
	}

	onPointerLeave(): void {
		clearTimeout(this.#hoverTimer);
		this.#hoverTimer = setTimeout(() => {
			if (!this.#disposed) this.hovered = false;
		}, 240);
	}

	onFocusIn(event: FocusEvent, container?: HTMLElement | null): void {
		const target = event.target;
		if (!(target instanceof HTMLElement) || !target.matches(':focus-visible')) return;
		clearTimeout(this.#focusTimer);
		this.focused = true;
		if (!container || typeof requestAnimationFrame === 'undefined') return;
		// If expanding from rail mode unmounts the focused rail-only trigger (no focusout fires),
		// re-verify containment on the next animation frame so `focused` cannot leak true.
		this.#cancelFocusRaf();
		this.#focusRaf = requestAnimationFrame(() => {
			this.#focusRaf = undefined;
			this.#syncFocusContainment(container);
		});
	}

	onFocusOut(event: FocusEvent, container: HTMLElement | null): void {
		if (this.menuOpen) return;
		const next = event.relatedTarget;
		if (next instanceof Node && container?.contains(next)) return;
		clearTimeout(this.#focusTimer);
		this.#cancelFocusRaf();
		this.#focusTimer = setTimeout(() => {
			if (!this.#disposed && !this.menuOpen) this.focused = false;
		}, 160);
	}

	openForKeyMode(): void {
		if (this.#navCollapsed()) {
			clearTimeout(this.#focusTimer);
			this.focused = true;
		}
	}

	togglePin(): void {
		clearTimeout(this.#hoverTimer);
		clearTimeout(this.#focusTimer);
		if (this.#opts.toggleNavCollapsed) this.#opts.toggleNavCollapsed();
		else uiPrefs.toggleNavCollapsed();
		if (this.#navCollapsed()) {
			this.hovered = false;
			this.focused = false;
		}
	}

	attachReflow = (node: HTMLElement): (() => void) => {
		const measure = () => {
			if (this.#disposed || typeof document === 'undefined') return;
			const containerWidth = node.parentElement?.getBoundingClientRect().width ?? window.innerWidth;
			const rootFont = parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
			this.textReflowRail = containerWidth >= 640 && containerWidth < 40 * rootFont;
		};
		measure();
		if (typeof ResizeObserver === 'undefined') return () => undefined;
		const observer = new ResizeObserver(measure);
		this.#reflowObserver = observer;
		observer.observe(node);
		if (node.parentElement) observer.observe(node.parentElement);
		return () => {
			observer.disconnect();
			if (this.#reflowObserver === observer) this.#reflowObserver = undefined;
		};
	};

	dispose(): void {
		this.#disposed = true;
		clearTimeout(this.#hoverTimer);
		clearTimeout(this.#focusTimer);
		this.#hoverTimer = undefined;
		this.#focusTimer = undefined;
		this.#reflowObserver?.disconnect();
		this.#reflowObserver = undefined;
		if (this.#focusRaf !== undefined && typeof cancelAnimationFrame !== 'undefined') {
			cancelAnimationFrame(this.#focusRaf);
			this.#focusRaf = undefined;
		}
	}
}
