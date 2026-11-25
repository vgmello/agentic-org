import { isEditableTarget, isPlainKey, singleKeyShortcutsOn } from './shortcuts';

export type NavKeyScope = 'sidebar' | 'inbox' | 'page';

export interface NavKeyCandidate {
	label: string;
	element: HTMLElement;
	onActivate?: () => void;
}

/** Pure letter matcher: picks the next candidate whose trimmed label starts with `letter` (case-insensitive). */
export function matchNavCandidate(
	candidates: readonly { label: string }[],
	letter: string,
	currentIndex: number
): number {
	const needle = letter.trim().toLowerCase();
	if (needle.length !== 1 || !/[a-z]/.test(needle)) return -1;
	const matching: number[] = [];
	for (let i = 0; i < candidates.length; i++) {
		const text = candidates[i].label.trim().toLowerCase();
		if (text.startsWith(needle)) matching.push(i);
	}
	if (matching.length === 0) return -1;
	const currentPos = matching.indexOf(currentIndex);
	if (currentPos >= 0) return matching[(currentPos + 1) % matching.length];
	return matching[0];
}

const MODAL_OVERLAY_SELECTOR =
	'[role="dialog"], [role="alertdialog"], [role="menu"], [role="listbox"]';

export interface NavKeyCallbacks {
	onSidebarOpen?: () => void;
	onSidebarCancel?: () => void;
	onSelectInboxTab?: (tabId: string) => void;
}

export class NavKeyController {
	mode = $state<NavKeyScope | null>(null);
	hint = $state('');
	#previousFocus: HTMLElement | null = null;
	#activePageScope: HTMLElement | null = null;
	#selectedCandidate: NavKeyCandidate | null = null;
	#selectedIndex = -1;
	#commitTimer: ReturnType<typeof setTimeout> | undefined;
	#openingFrame: number | undefined;
	#openingSidebar = false;
	#openingInbox = false;
	#pendingLetter: string | null = null;
	#callbacks: NavKeyCallbacks;

	constructor(callbacks: NavKeyCallbacks = {}) {
		this.#callbacks = callbacks;
	}

	#clearSelection() {
		clearTimeout(this.#commitTimer);
		this.#commitTimer = undefined;
		this.#selectedCandidate = null;
	}

	cancel(restoreFocus = false) {
		this.#clearSelection();
		if (this.#openingFrame !== undefined && typeof cancelAnimationFrame !== 'undefined') {
			cancelAnimationFrame(this.#openingFrame);
			this.#openingFrame = undefined;
		}
		this.#openingSidebar = false;
		this.#openingInbox = false;
		this.#pendingLetter = null;
		const prevMode = this.mode;
		const prevFocus = this.#previousFocus;
		this.mode = null;
		this.hint = '';
		this.#previousFocus = null;
		this.#activePageScope = null;
		this.#selectedIndex = -1;
		if (prevMode === 'sidebar') this.#callbacks.onSidebarCancel?.();
		if (restoreFocus) {
			if (prevFocus?.isConnected && prevFocus.offsetParent !== null) {
				prevFocus.focus({ preventScroll: true });
				prevFocus.scrollIntoView?.({ block: 'nearest', inline: 'nearest' });
			} else {
				document.getElementById('shell-page-title')?.focus({ preventScroll: true });
			}
		}
	}

	#modeHint(mode: 'sidebar' | 'inbox' | 'page'): string {
		if (mode === 'sidebar') return "Sidebar · Type an item's first letter · Esc to cancel";
		if (mode === 'inbox') return 'Inbox categories · Type a letter · Esc to cancel';
		return "Page menu · Type an item's first letter · Esc to cancel";
	}

	#findVisibleSidebarNav(): HTMLElement | null {
		const navs = [...document.querySelectorAll<HTMLElement>('[data-exec-sidebar-nav]')];
		return navs.find((el) => el.offsetParent !== null) ?? null;
	}

	#findVisiblePageMenu(): HTMLElement | null {
		const scopes = [...document.querySelectorAll<HTMLElement>('main [data-page-menu]')];
		return (
			scopes.find((el) => el.offsetParent !== null && el.querySelector('[data-menu-label]')) ?? null
		);
	}

	#collectSidebarCandidates(): NavKeyCandidate[] {
		const nav = this.#findVisibleSidebarNav();
		if (!nav) return [];
		const links = [...nav.querySelectorAll<HTMLAnchorElement>('a[href]')].filter(
			(el) => el.offsetParent !== null
		);
		return links.map((el) => ({
			label: (el.dataset.navLabel ?? el.textContent ?? '').trim(),
			element: el
		}));
	}

	#collectInboxCandidates(): NavKeyCandidate[] {
		const group = document.getElementById('triage-categories');
		if (!group) return [];
		const out: NavKeyCandidate[] = [];
		for (const btn of group.querySelectorAll<HTMLButtonElement>('button[data-triage-tab]')) {
			if (btn.offsetParent === null) continue;
			out.push({
				label: (btn.dataset.triageLabel ?? btn.textContent ?? '').trim(),
				element: btn
			});
		}
		const moreTrigger = group.querySelector<HTMLButtonElement>('button[data-triage-more]');
		const overflowJson = group.dataset.overflowTabs;
		if (moreTrigger && overflowJson) {
			try {
				const extras = JSON.parse(overflowJson) as { id: string; label: string }[];
				for (const item of extras) {
					out.push({
						label: item.label,
						element: moreTrigger,
						onActivate: () => {
							group.dispatchEvent(
								new CustomEvent('inbox:select-tab', { detail: { tabId: item.id }, bubbles: true })
							);
							this.#callbacks.onSelectInboxTab?.(item.id);
						}
					});
				}
			} catch {
				// Ignore malformed metadata
			}
		}
		return out;
	}

	#collectPageMenuCandidates(): NavKeyCandidate[] {
		const scope =
			this.#activePageScope?.isConnected && this.#activePageScope.offsetParent !== null
				? this.#activePageScope
				: this.#findVisiblePageMenu();
		if (!scope) return [];
		this.#activePageScope = scope;
		const nodes = [...scope.querySelectorAll<HTMLElement>('[data-menu-label]')].filter(
			(el) => el.offsetParent !== null
		);
		return nodes.map((el) => ({
			label: (el.dataset.menuLabel ?? el.textContent ?? '').trim(),
			element: el
		}));
	}

	#activate(candidate: NavKeyCandidate) {
		const prevMode = this.mode;
		this.#clearSelection();
		if (this.#openingFrame !== undefined && typeof cancelAnimationFrame !== 'undefined') {
			cancelAnimationFrame(this.#openingFrame);
			this.#openingFrame = undefined;
		}
		this.#openingSidebar = false;
		this.#openingInbox = false;
		this.#pendingLetter = null;
		this.mode = null;
		this.hint = '';
		this.#previousFocus = null;
		this.#activePageScope = null;
		this.#selectedIndex = -1;
		// Trigger navigation / action while the element is still mounted, then close any transient sheet.
		if (candidate.onActivate) {
			candidate.onActivate();
		} else {
			candidate.element.click();
		}
		if (prevMode === 'sidebar') this.#callbacks.onSidebarCancel?.();
	}

	#selectByLetter(letter: string): boolean {
		const candidates =
			this.mode === 'sidebar'
				? this.#collectSidebarCandidates()
				: this.mode === 'inbox'
					? this.#collectInboxCandidates()
					: this.#collectPageMenuCandidates();
		const nextIdx = matchNavCandidate(candidates, letter, this.#selectedIndex);
		if (nextIdx < 0) return false;
		const chosen = candidates[nextIdx];
		this.#selectedIndex = nextIdx;
		this.#selectedCandidate = chosen;
		chosen.element.focus({ preventScroll: true });
		chosen.element.scrollIntoView?.({ block: 'nearest', inline: 'nearest' });
		const prefix =
			this.mode === 'sidebar'
				? 'Sidebar'
				: this.mode === 'inbox'
					? 'Inbox categories'
					: 'Page menu';
		this.hint = `${prefix} · ${chosen.label} · Enter to open · Esc to cancel`;
		clearTimeout(this.#commitTimer);
		this.#commitTimer = setTimeout(() => {
			if (this.#selectedCandidate === chosen) {
				this.#activate(chosen);
			}
		}, 650);
		return true;
	}

	enterSidebar() {
		this.#previousFocus =
			document.activeElement instanceof HTMLElement ? document.activeElement : null;
		this.mode = 'sidebar';
		this.#clearSelection();
		this.#selectedIndex = -1;
		this.#pendingLetter = null;
		this.#activePageScope = null;
		this.#openingSidebar = true;
		this.hint = "Sidebar · Type an item's first letter · Esc to cancel";
		this.#callbacks.onSidebarOpen?.();

		const focusVisibleSidebar = (attemptsLeft: number) => {
			if (this.mode !== 'sidebar') return;
			const candidates = this.#collectSidebarCandidates();
			if (
				candidates.length === 0 &&
				attemptsLeft > 0 &&
				typeof requestAnimationFrame !== 'undefined'
			) {
				this.#openingFrame = requestAnimationFrame(() => focusVisibleSidebar(attemptsLeft - 1));
				return;
			}
			this.#openingFrame = undefined;
			this.#openingSidebar = false;
			const activeIdx = candidates.findIndex(
				(c) => c.element.getAttribute('aria-current') === 'page'
			);
			this.#selectedIndex = activeIdx;
			if (this.#pendingLetter) {
				const queued = this.#pendingLetter;
				this.#pendingLetter = null;
				if (this.#selectByLetter(queued)) return;
			}
			const current =
				(activeIdx >= 0 ? candidates[activeIdx].element : null) ?? candidates[0]?.element;
			current?.focus({ preventScroll: true });
		};
		queueMicrotask(() => focusVisibleSidebar(10));
	}

	enterInboxCategories(): boolean {
		const group = document.getElementById('triage-categories');
		if (!group) return false;
		this.#previousFocus =
			document.activeElement instanceof HTMLElement ? document.activeElement : null;
		this.mode = 'inbox';
		this.#clearSelection();
		this.#selectedIndex = -1;
		this.#pendingLetter = null;
		this.#activePageScope = null;
		this.hint = 'Inbox categories · Type a letter · Esc to cancel';

		const focusCategories = (attemptsLeft: number) => {
			if (this.mode !== 'inbox') return;
			const currentGroup = document.getElementById('triage-categories');
			if (!currentGroup || currentGroup.offsetParent === null) {
				if (attemptsLeft > 0 && typeof requestAnimationFrame !== 'undefined') {
					this.#openingFrame = requestAnimationFrame(() => focusCategories(attemptsLeft - 1));
					return;
				}
				this.#openingInbox = false;
				this.cancel(false);
				return;
			}
			this.#openingFrame = undefined;
			this.#openingInbox = false;
			const candidates = this.#collectInboxCandidates();
			const activeIdx = candidates.findIndex(
				(c) => !c.onActivate && c.element.getAttribute('aria-pressed') === 'true'
			);
			this.#selectedIndex = activeIdx;
			if (this.#pendingLetter) {
				const queued = this.#pendingLetter;
				this.#pendingLetter = null;
				if (this.#selectByLetter(queued)) return;
			}
			const current =
				(activeIdx >= 0 ? candidates[activeIdx].element : null) ??
				currentGroup.querySelector<HTMLElement>('button[data-triage-tab]');
			current?.focus({ preventScroll: true });
		};

		if (group.offsetParent === null) {
			this.#openingInbox = true;
			group.dispatchEvent(new CustomEvent('inbox:show-categories', { bubbles: true }));
			queueMicrotask(() => focusCategories(10));
			return true;
		}
		focusCategories(0);
		return true;
	}

	enterPageMenu(): boolean {
		if (this.enterInboxCategories()) return true;
		const scope = this.#findVisiblePageMenu();
		if (!scope) return false;
		this.#activePageScope = scope;
		const candidates = this.#collectPageMenuCandidates();
		if (candidates.length === 0) {
			this.#activePageScope = null;
			return false;
		}
		this.#previousFocus =
			document.activeElement instanceof HTMLElement ? document.activeElement : null;
		this.mode = 'page';
		this.#clearSelection();
		this.#pendingLetter = null;
		this.hint = "Page menu · Type an item's first letter · Esc to cancel";

		const activeIdx = candidates.findIndex(
			(c) =>
				c.element.getAttribute('aria-current') === 'page' ||
				c.element.getAttribute('aria-pressed') === 'true' ||
				c.element.getAttribute('aria-selected') === 'true' ||
				c.element.hasAttribute('data-current-kind')
		);
		this.#selectedIndex = activeIdx;
		const current =
			(activeIdx >= 0 ? candidates[activeIdx].element : null) ?? candidates[0]?.element;
		current?.focus({ preventScroll: true });
		current?.scrollIntoView?.({ block: 'nearest', inline: 'nearest' });
		return true;
	}

	handleKeydown = (event: KeyboardEvent) => {
		if (event.defaultPrevented || event.isComposing || event.keyCode === 229) return;
		if (!singleKeyShortcutsOn()) {
			if (this.mode) this.cancel(false);
			return;
		}

		if (this.mode) {
			if (event.key === 'Escape') {
				event.preventDefault();
				event.stopPropagation();
				this.cancel(true);
				return;
			}
			if (
				event.key === 'Tab' ||
				event.key === 'ArrowDown' ||
				event.key === 'ArrowUp' ||
				event.key === 'ArrowLeft' ||
				event.key === 'ArrowRight' ||
				event.key === 'Home' ||
				event.key === 'End'
			) {
				this.#clearSelection();
				this.hint = this.#modeHint(this.mode);
				return;
			}
			if (event.key === 'Enter' && this.#selectedCandidate) {
				if (event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) {
					// Preserve native modified-Enter behavior (e.g. open link in new tab) without double-activating.
					this.cancel(false);
					return;
				}
				event.preventDefault();
				event.stopPropagation();
				this.#activate(this.#selectedCandidate);
				return;
			}
			if (
				!event.metaKey &&
				!event.ctrlKey &&
				!event.altKey &&
				event.key.length === 1 &&
				/[a-z]/i.test(event.key)
			) {
				if (this.#openingSidebar || this.#openingInbox) {
					event.preventDefault();
					event.stopPropagation();
					this.#pendingLetter = event.key;
					return;
				}
				if (this.#selectByLetter(event.key)) {
					event.preventDefault();
					event.stopPropagation();
				}
				return;
			}
			return;
		}

		if (!isPlainKey(event) || event.shiftKey) return;
		if (isEditableTarget(event.target)) return;
		if (document.querySelector(MODAL_OVERLAY_SELECTOR)) return;

		const key = event.key.toLowerCase();
		if (key === 's') {
			event.preventDefault();
			this.enterSidebar();
		} else if (key === 'm') {
			if (this.enterPageMenu()) {
				event.preventDefault();
			}
		}
	};

	handlePointerDown = (event: PointerEvent) => {
		if (!this.mode) return;
		const target = event.target;
		if (target instanceof Node) {
			if (this.mode === 'sidebar') {
				const nav = this.#findVisibleSidebarNav();
				const container = nav?.closest('aside, [role="dialog"]') ?? nav;
				if (container && container.contains(target)) {
					// Release shortcut state without closing the mobile drawer so the link click completes.
					this.#clearSelection();
					if (this.#openingFrame !== undefined && typeof cancelAnimationFrame !== 'undefined') {
						cancelAnimationFrame(this.#openingFrame);
						this.#openingFrame = undefined;
					}
					this.#openingSidebar = false;
					this.#pendingLetter = null;
					this.mode = null;
					this.hint = '';
					this.#previousFocus = null;
					this.#activePageScope = null;
					this.#selectedIndex = -1;
					return;
				}
			} else if (this.mode === 'inbox') {
				const group = document.getElementById('triage-categories');
				if (group && group.contains(target)) {
					this.#clearSelection();
					this.mode = null;
					this.hint = '';
					this.#previousFocus = null;
					this.#selectedIndex = -1;
					return;
				}
			} else if (this.mode === 'page') {
				const scope = this.#activePageScope ?? this.#findVisiblePageMenu();
				if (scope && scope.contains(target)) {
					this.#clearSelection();
					this.mode = null;
					this.hint = '';
					this.#previousFocus = null;
					this.#activePageScope = null;
					this.#selectedIndex = -1;
					return;
				}
			}
		}
		this.cancel(false);
	};

	handleFocusIn = (event: FocusEvent) => {
		if (!this.mode) return;
		const target = event.target;
		if (!(target instanceof Node)) return;
		if (this.#selectedCandidate && target !== this.#selectedCandidate.element) {
			this.#clearSelection();
		}
		if (this.mode === 'sidebar') {
			if (this.#openingSidebar) return;
			const nav = this.#findVisibleSidebarNav();
			const container = nav?.closest('aside, [role="dialog"]') ?? nav;
			if (container && container.contains(target)) {
				const candidates = this.#collectSidebarCandidates();
				const focusedIdx = candidates.findIndex((c) => c.element === target);
				if (focusedIdx >= 0) this.#selectedIndex = focusedIdx;
				return;
			}
			this.cancel(false);
		} else if (this.mode === 'inbox') {
			if (this.#openingInbox) return;
			const group = document.getElementById('triage-categories');
			if (group && group.contains(target)) {
				const candidates = this.#collectInboxCandidates();
				const focusedIdx = candidates.findIndex((c) => !c.onActivate && c.element === target);
				if (focusedIdx >= 0) this.#selectedIndex = focusedIdx;
				return;
			}
			this.cancel(false);
		} else if (this.mode === 'page') {
			const scope = this.#activePageScope ?? this.#findVisiblePageMenu();
			if (scope && scope.contains(target)) {
				const candidates = this.#collectPageMenuCandidates();
				const focusedIdx = candidates.findIndex((c) => c.element === target);
				if (focusedIdx >= 0) this.#selectedIndex = focusedIdx;
				return;
			}
			this.cancel(false);
		}
	};

	attach = () => {
		window.addEventListener('keydown', this.handleKeydown, { capture: true });
		window.addEventListener('pointerdown', this.handlePointerDown, { capture: true });
		document.addEventListener('focusin', this.handleFocusIn);
		return () => {
			window.removeEventListener('keydown', this.handleKeydown, { capture: true });
			window.removeEventListener('pointerdown', this.handlePointerDown, { capture: true });
			document.removeEventListener('focusin', this.handleFocusIn);
			this.cancel(false);
		};
	};
}
