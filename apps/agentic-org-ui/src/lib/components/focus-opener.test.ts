import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { captureOverlayOpener, resetPointerOpenerTrackingForTests, returnFocus } from './focus';

function walkAncestors(
	start: FakeElement | null,
	match: (el: FakeElement) => boolean
): FakeElement | null {
	let cur = start;
	while (cur) {
		if (match(cur)) return cur;
		cur = cur.parent;
	}
	return null;
}

class FakeElement {
	tagName: string;
	isConnected = true;
	disabled = false;
	focused = false;
	parent: FakeElement | null = null;
	attrs: Record<string, string> = {};
	children: FakeElement[] = [];

	constructor(tagName: string, attrs: Record<string, string> = {}) {
		this.tagName = tagName.toUpperCase();
		this.attrs = attrs;
	}

	append(child: FakeElement) {
		child.parent = this;
		this.children.push(child);
	}

	contains(other: unknown): boolean {
		let cur = other as FakeElement | null;
		while (cur) {
			if (cur === this) return true;
			cur = cur.parent;
		}
		return false;
	}

	closest(selector: string): FakeElement | null {
		return walkAncestors(this, (cur) => {
			if (selector.includes('inert') && 'inert' in cur.attrs) return true;
			if (
				selector.includes('[role="menu"]') &&
				(cur.attrs.role === 'menu' || cur.attrs.role === 'listbox')
			)
				return true;
			return Boolean(
				selector.includes('button') && (cur.tagName === 'BUTTON' || cur.attrs.role === 'button')
			);
		});
	}

	focus() {
		this.focused = true;
		(globalThis.document as unknown as { activeElement: unknown }).activeElement = this;
	}
}

describe('captureOverlayOpener & returnFocus (WebKit pointer-open support)', () => {
	const listeners = new Map<string, Set<(e: Event) => void>>();
	const body = new FakeElement('body');
	const docEl = new FakeElement('html');

	beforeEach(() => {
		listeners.clear();
		vi.stubGlobal('HTMLElement', FakeElement);
		vi.stubGlobal('Element', FakeElement);
		vi.stubGlobal('document', {
			body,
			documentElement: docEl,
			activeElement: body,
			addEventListener(type: string, fn: (e: Event) => void) {
				let set = listeners.get(type);
				if (!set) {
					set = new Set();
					listeners.set(type, set);
				}
				set.add(fn);
			},
			dispatch(type: string, target: FakeElement) {
				for (const fn of listeners.get(type) ?? []) fn({ target } as unknown as Event);
			}
		});
		resetPointerOpenerTrackingForTests();
	});

	afterEach(() => {
		resetPointerOpenerTrackingForTests();
		vi.unstubAllGlobals();
	});

	it('captures activeElement when opened by keyboard', () => {
		const button = new FakeElement('button');
		(globalThis.document as unknown as { activeElement: unknown }).activeElement = button;
		expect(captureOverlayOpener(null)).toBe(button);
	});

	it('falls back to last pointerdown interactive control when WebKit leaves activeElement on body', async () => {
		const button = new FakeElement('button');
		const icon = new FakeElement('svg');
		button.append(icon);
		// Trigger listener install once on document, then dispatch pointerdown on the button's child icon.
		captureOverlayOpener(null);
		(globalThis.document as unknown as { dispatch: (t: string, el: FakeElement) => void }).dispatch(
			'pointerdown',
			icon
		);
		// Simulate WebKit leaving document.activeElement on document.body after a mouse click.
		(globalThis.document as unknown as { activeElement: unknown }).activeElement = body;

		const captured = captureOverlayOpener(null);
		expect(captured).toBe(button);

		await returnFocus(captured);
		expect(button.focused).toBe(true);
	});

	it('does not overwrite opener with a menuitem inside a closing role="menu"', () => {
		const trigger = new FakeElement('button');
		const menu = new FakeElement('div', { role: 'menu' });
		const menuItem = new FakeElement('button', { role: 'menuitem' });
		menu.append(menuItem);

		captureOverlayOpener(null);
		const dispatch = (
			globalThis.document as unknown as { dispatch: (t: string, el: FakeElement) => void }
		).dispatch;
		dispatch('pointerdown', trigger);
		dispatch('pointerdown', menuItem);

		expect(captureOverlayOpener(null)).toBe(trigger);
	});
});
