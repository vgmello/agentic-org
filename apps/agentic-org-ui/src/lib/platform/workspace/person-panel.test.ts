import { describe, expect, it, vi } from 'vitest';
import { PersonPanelState } from './person-panel.svelte';

describe('PersonPanelState', () => {
	it('opens with trimmed memberId, tracks opener, and notifies open listeners', () => {
		const panel = new PersonPanelState();
		const opened: string[] = [];
		const unsub = panel.onOpen((id) => opened.push(id));
		const fakeOpener = { tagName: 'A' } as unknown as HTMLElement;

		panel.open('  rowan  ', fakeOpener);
		expect(panel.isOpen).toBe(true);
		expect(panel.memberId).toBe('rowan');
		expect(panel.opener).toBe(fakeOpener);
		expect(panel.openSeq).toBe(1);
		expect(panel.restoreFocusOnClose).toBe(true);
		expect(opened).toEqual(['rowan']);

		unsub();
		panel.dispose();
	});

	it('ignores blank member ids', () => {
		const panel = new PersonPanelState();
		panel.open('   ');
		expect(panel.isOpen).toBe(false);
		expect(panel.memberId).toBeNull();
		expect(panel.openSeq).toBe(0);
		panel.dispose();
	});

	it('preserves existing opener when switching members while already open', () => {
		const panel = new PersonPanelState();
		const fakeOpener = { tagName: 'BUTTON' } as unknown as HTMLElement;

		panel.open('rowan', fakeOpener);
		panel.open('maya');
		expect(panel.memberId).toBe('maya');
		expect(panel.opener).toBe(fakeOpener);
		expect(panel.openSeq).toBe(2);

		panel.dispose();
	});

	it('records restoreFocusOnClose flag and clears member state on close', () => {
		const panel = new PersonPanelState();
		const onClose = vi.fn();
		panel.onClose(onClose);

		panel.open('eden');
		panel.close(false);
		expect(panel.isOpen).toBe(false);
		expect(panel.memberId).toBeNull();
		expect(panel.opener).toBeNull();
		expect(panel.restoreFocusOnClose).toBe(false);
		expect(onClose).toHaveBeenCalledTimes(1);

		// Redundant close is a no-op
		panel.close(true);
		expect(onClose).toHaveBeenCalledTimes(1);

		panel.dispose();
	});

	it('closes without restoring focus when the bound session-end hook fires', () => {
		const panel = new PersonPanelState();
		let endSession: (() => void) | undefined;
		const unsubSpy = vi.fn();
		const detach = panel.bindSessionEnd((cb) => {
			endSession = cb;
			return unsubSpy;
		});

		panel.open('maya');
		expect(panel.isOpen).toBe(true);

		endSession?.();
		expect(panel.isOpen).toBe(false);
		expect(panel.memberId).toBeNull();
		expect(panel.restoreFocusOnClose).toBe(false);

		detach();
		expect(unsubSpy).toHaveBeenCalledTimes(1);
		panel.dispose();
	});

	it('unsubscribes session listener and clears listeners on dispose', () => {
		const panel = new PersonPanelState();
		const unsubSpy = vi.fn();
		panel.bindSessionEnd(() => unsubSpy);
		const onOpen = vi.fn();
		panel.onOpen(onOpen);

		panel.open('rowan');
		panel.dispose();

		expect(unsubSpy).toHaveBeenCalledTimes(1);
		expect(panel.isOpen).toBe(false);
		panel.open('maya');
		expect(onOpen).toHaveBeenCalledTimes(1);
	});
});
