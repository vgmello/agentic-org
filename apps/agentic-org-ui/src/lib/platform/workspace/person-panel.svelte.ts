/**
 * Pure reactive state controller for the global lazily-loaded Person Side Panel.
 * Lives in `platform/workspace` with zero module/shell/auth imports so any surface can open
 * a member's summary panel cleanly without coupling to the panel UI or pulling it into first paint.
 */
export class PersonPanelState {
	#memberId = $state<string | null>(null);
	#open = $state(false);
	#openSeq = $state(0);
	#opener = $state.raw<HTMLElement | null>(null);
	#restoreFocusOnClose = $state(true);
	#openListeners: Array<(memberId: string) => void> = [];
	#closeListeners: Array<() => void> = [];
	#sessionUnsub: (() => void) | undefined;

	get isOpen(): boolean {
		return this.#open;
	}

	get memberId(): string | null {
		return this.#memberId;
	}

	get openSeq(): number {
		return this.#openSeq;
	}

	get opener(): HTMLElement | null {
		return this.#opener;
	}

	get restoreFocusOnClose(): boolean {
		return this.#restoreFocusOnClose;
	}

	open(memberId: string, opener?: HTMLElement | null): void {
		const trimmed = memberId.trim();
		if (!trimmed) return;
		if (opener !== undefined) {
			this.#opener = opener;
		} else if (!this.#open) {
			this.#opener = null;
		}
		this.#memberId = trimmed;
		this.#open = true;
		this.#restoreFocusOnClose = true;
		this.#openSeq += 1;
		for (const listener of [...this.#openListeners]) {
			listener(trimmed);
		}
	}

	captureOpener(opener: HTMLElement | null): void {
		if (opener) this.#opener = opener;
	}

	close(restoreFocus = true): void {
		if (!this.#open && !this.#memberId) return;
		this.#restoreFocusOnClose = restoreFocus;
		this.#open = false;
		this.#memberId = null;
		this.#opener = null;
		for (const listener of [...this.#closeListeners]) {
			listener();
		}
	}

	/** Bind panel cleanup to an external session-ended hook without importing platform/auth here. */
	bindSessionEnd(subscribe: (listener: () => void) => () => void): () => void {
		this.#sessionUnsub?.();
		const unsub = subscribe(() => this.close(false));
		this.#sessionUnsub = unsub;
		return () => {
			if (this.#sessionUnsub === unsub) {
				this.#sessionUnsub = undefined;
			}
			unsub();
		};
	}

	onOpen(listener: (memberId: string) => void): () => void {
		if (!this.#openListeners.includes(listener)) {
			this.#openListeners.push(listener);
		}
		return () => {
			this.#openListeners = this.#openListeners.filter((fn) => fn !== listener);
		};
	}

	onClose(listener: () => void): () => void {
		if (!this.#closeListeners.includes(listener)) {
			this.#closeListeners.push(listener);
		}
		return () => {
			this.#closeListeners = this.#closeListeners.filter((fn) => fn !== listener);
		};
	}

	dispose(): void {
		this.#sessionUnsub?.();
		this.#sessionUnsub = undefined;
		this.close(false);
		this.#openListeners = [];
		this.#closeListeners = [];
	}
}

export const personPanel = new PersonPanelState();
