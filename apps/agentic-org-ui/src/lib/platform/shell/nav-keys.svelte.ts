import { isEditableTarget, isPlainKey, singleKeyShortcutsOn } from './shortcuts';

export type NavKeyScope = 'sidebar' | 'inbox';

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
	'[role="dialog"]:not(.work-reading), [role="alertdialog"], [role="menu"], [role="listbox"]';

export interface NavKeyCallbacks {
	onSidebarOpen?: () => void;
	onSidebarCancel?: () => void;
	onSelectInboxTab?: (tabId: string) => void;
}

export class NavKeyController {
	mode = $state<NavKeyScope | null>(null);
	hint = $state('');
	#previousFocus: HTMLElement | null = null;
	#selectedCandidate: NavKeyCandidate | null = null;
	#selectedIndex = -1;
	#commitTimer: ReturnType<typeof setTimeout> | undefined;
	#callbacks: NavKeyCallbacks;

	constructor(callbacks: NavKeyCallbacks = {}) {
		this.#callbacks = callbacks;
	}

	cancel(restoreFocus = false) {
		clearTimeout(this.#commitTimer);
		this.#commitTimer = undefined;
		const prevMode = this.mode;
		const prevFocus = this.#previousFocus;
		this.mode = null;
		this.hint = '';
		this.#previousFocus = null;
		this.#selectedCandidate = null;
		this.#selectedIndex = -1;
		if (prevMode === 'sidebar') this.#callbacks.onSidebarCancel?.();
		if (restoreFocus) {
			if (prevFocus?.isConnected && prevFocus.offsetParent !== null) {
				prevFocus.focus({ preventScroll: true });
			} else {
				document.getElementById('shell-page-title')?.focus({ preventScroll: true });
			}
		}
	}

	#collectSidebarCandidates(): NavKeyCandidate[] {
		const nav = document.getElementById('exec-sidebar-nav');
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

	#activate(candidate: NavKeyCandidate) {
		this.cancel(false);
		if (candidate.onActivate) {
			candidate.onActivate();
			return;
		}
		candidate.element.click();
	}

	enterSidebar() {
		this.#previousFocus =
			document.activeElement instanceof HTMLElement ? document.activeElement : null;
		this.mode = 'sidebar';
		this.#selectedCandidate = null;
		this.#selectedIndex = -1;
		this.hint = "Sidebar · Type an item's first letter · Esc to cancel";
		this.#callbacks.onSidebarOpen?.();
		queueMicrotask(() => {
			if (this.mode !== 'sidebar') return;
			const candidates = this.#collectSidebarCandidates();
			const activeIdx = candidates.findIndex(
				(c) => c.element.getAttribute('aria-current') === 'page'
			);
			this.#selectedIndex = activeIdx;
			const current =
				(activeIdx >= 0 ? candidates[activeIdx].element : null) ?? candidates[0]?.element;
			current?.focus({ preventScroll: true });
		});
	}

	enterInboxCategories(): boolean {
		const group = document.getElementById('triage-categories');
		if (!group || group.offsetParent === null) return false;
		this.#previousFocus =
			document.activeElement instanceof HTMLElement ? document.activeElement : null;
		this.mode = 'inbox';
		this.#selectedCandidate = null;
		const candidates = this.#collectInboxCandidates();
		const activeIdx = candidates.findIndex(
			(c) => !c.onActivate && c.element.getAttribute('aria-pressed') === 'true'
		);
		this.#selectedIndex = activeIdx;
		this.hint = 'Inbox categories · Type a letter · Esc to cancel';
		const current =
			(activeIdx >= 0 ? candidates[activeIdx].element : null) ??
			group.querySelector<HTMLElement>('button[data-triage-tab]');
		current?.focus({ preventScroll: true });
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
			if (event.key === 'Enter' && this.#selectedCandidate) {
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
				const candidates =
					this.mode === 'sidebar'
						? this.#collectSidebarCandidates()
						: this.#collectInboxCandidates();
				const nextIdx = matchNavCandidate(candidates, event.key, this.#selectedIndex);
				if (nextIdx >= 0) {
					event.preventDefault();
					event.stopPropagation();
					const chosen = candidates[nextIdx];
					this.#selectedIndex = nextIdx;
					this.#selectedCandidate = chosen;
					chosen.element.focus({ preventScroll: true });
					const prefix = this.mode === 'sidebar' ? 'Sidebar' : 'Inbox categories';
					this.hint = `${prefix} · ${chosen.label} · Enter to open · Esc to cancel`;
					clearTimeout(this.#commitTimer);
					this.#commitTimer = setTimeout(() => {
						if (this.#selectedCandidate === chosen) {
							this.#activate(chosen);
						}
					}, 650);
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
			if (this.enterInboxCategories()) {
				event.preventDefault();
			}
		}
	};

	handlePointerDown = () => {
		if (this.mode) this.cancel(false);
	};

	handleFocusIn = (event: FocusEvent) => {
		if (!this.mode) return;
		const target = event.target;
		if (!(target instanceof Node)) return;
		if (this.mode === 'sidebar') {
			const nav = document.getElementById('exec-sidebar-nav');
			if (nav && !nav.contains(target)) {
				this.cancel(false);
			}
		} else if (this.mode === 'inbox') {
			const group = document.getElementById('triage-categories');
			if (group && !group.contains(target)) {
				this.cancel(false);
			}
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
