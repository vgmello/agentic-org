import { createSubscriber } from 'svelte/reactivity';

/**
 * Single-key shortcuts (app-shell-ux §0.1, §0.2): one device-wide `agentic-org.singleKeyShortcuts`
 * value, `1` or `0`, never synced. Missing, unreadable or unavailable storage reads as on.
 * The shell's `N`/`Shift+N` (New) and `/` (Search) obey it; Your preferences writes it.
 */
export const SINGLE_KEY_SHORTCUTS_KEY = 'agentic-org.singleKeyShortcuts';

type Store = Pick<Storage, 'getItem' | 'setItem'>;
const browserStore = (): Store | undefined => {
	try {
		return typeof localStorage === 'undefined' ? undefined : localStorage;
	} catch {
		return undefined;
	}
};

/** The saved value; on unless it is exactly `0`. */
export function readSingleKeyShortcuts(store: Store | undefined = browserStore()): boolean {
	try {
		return store?.getItem(SINGLE_KEY_SHORTCUTS_KEY) !== '0';
	} catch {
		return true;
	}
}

const listeners = new Set<() => void>();

/** Saves the switch. Returns false when the device would not keep it (the setting then stays as it was). */
export function writeSingleKeyShortcuts(
	on: boolean,
	store: Store | undefined = browserStore()
): boolean {
	try {
		if (!store) return false;
		store.setItem(SINGLE_KEY_SHORTCUTS_KEY, on ? '1' : '0');
		return true;
	} catch {
		return false;
	} finally {
		for (const listener of listeners) listener();
	}
}

const subscribe = createSubscriber((update) => {
	listeners.add(update);
	const onStorage = (event: StorageEvent) => {
		if (event.key === SINGLE_KEY_SHORTCUTS_KEY || event.key === null) update();
	};
	window.addEventListener('storage', onStorage);
	return () => {
		listeners.delete(update);
		window.removeEventListener('storage', onStorage);
	};
});

/** Reactive in components and effects: re-reads when this tab or another one changes the switch. */
export function singleKeyShortcutsOn(): boolean {
	subscribe();
	return readSingleKeyShortcuts();
}

/** Whether a key press belongs to text editing, where single-key shortcuts never run. */
export function isEditableTarget(target: EventTarget | null): boolean {
	if (!(target instanceof HTMLElement)) return false;
	if (target.isContentEditable) return true;
	if (target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement) return true;
	if (target instanceof HTMLInputElement)
		return ![
			'button',
			'checkbox',
			'radio',
			'submit',
			'reset',
			'range',
			'color',
			'file',
			'image'
		].includes(target.type);
	return (
		target.closest(
			'[contenteditable=""], [contenteditable="true"], [role="textbox"], [role="combobox"]'
		) !== null
	);
}

/** A plain key press that may run a single-key shortcut: no Cmd/Ctrl/Alt, not composing, not repeated. */
export const isPlainKey = (event: KeyboardEvent) =>
	!event.metaKey && !event.ctrlKey && !event.altKey && !event.isComposing && !event.repeat;
