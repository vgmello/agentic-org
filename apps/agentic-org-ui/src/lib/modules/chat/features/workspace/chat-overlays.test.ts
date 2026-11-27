import { describe, expect, it, vi } from 'vitest';
import type { ChatPanelKind } from '../../types';
import { ChatOverlayController } from './chat-overlays.svelte';

function makeController() {
	const dismissOpenDialogs = vi.fn();
	const onWritePlace = vi.fn();
	const ctrl: ChatOverlayController = new ChatOverlayController({
		getCompactPanels: () => false,
		getPanelKind: (): ChatPanelKind | undefined =>
			ctrl.showMembers ? 'members' : ctrl.showPins ? 'pins' : undefined,
		getOfficeRunning: () => true,
		getConversationEl: () => undefined,
		getListEl: () => undefined,
		getHeading: () => undefined,
		getSidebarToggle: () => null,
		getPanel: () => undefined,
		getDialogs: () => ({ dismissOpenDialogs }),
		getNavigation: () => undefined,
		onWritePlace,
		onSelectConversation: vi.fn(),
		onResetSearchForThread: vi.fn()
	});
	return { ctrl, dismissOpenDialogs, onWritePlace };
}

describe('ChatOverlayController', () => {
	it('separates modal overlays from Members/Pins panels for Escape vs browser Back', () => {
		const { ctrl } = makeController();
		ctrl.showMembers = true;
		expect(ctrl.hasModalOverlay()).toBe(false);
		expect(ctrl.hasOpenOverlay()).toBe(true);

		// Escape closes Members panel when no modal dialog is open.
		const preventDefault = vi.fn();
		ctrl.handleWindowKeydown({
			key: 'Escape',
			defaultPrevented: false,
			isComposing: false,
			target: null,
			preventDefault
		} as unknown as KeyboardEvent);
		expect(preventDefault).toHaveBeenCalledOnce();
		expect(ctrl.showMembers).toBe(false);

		// Browser Back (dismissAllOverlays) closes Pins panel cleanly.
		ctrl.showPins = true;
		ctrl.dismissAllOverlays();
		expect(ctrl.showPins).toBe(false);
		ctrl.dispose();
	});
});
