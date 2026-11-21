import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { openerFrom, returnFocus } from './focus';

/** The few parts of the DOM returnFocus reads, enough to run it without a browser. */
class FakeEl {
	connected = true;
	disabled = false;
	inert = false;
	focusable = true;
	focused: { preventScroll?: boolean } | undefined;
	children: FakeEl[] = [];
	get isConnected() {
		return this.connected;
	}
	closest(selector: string) {
		return selector === '[inert]' && this.inert ? this : null;
	}
	contains(other: FakeEl) {
		return other === this || this.children.includes(other);
	}
	focus(options?: { preventScroll?: boolean }) {
		if (!this.focusable) return;
		this.focused = options;
		doc.activeElement = this;
	}
}
const doc: { activeElement: FakeEl | null; body: FakeEl; documentElement: FakeEl } = {
	activeElement: null,
	body: new FakeEl(),
	documentElement: new FakeEl()
};

describe('returnFocus', () => {
	beforeEach(() => {
		doc.activeElement = doc.body;
		(globalThis as { document?: unknown }).document = doc;
	});
	afterEach(() => {
		delete (globalThis as { document?: unknown }).document;
	});

	it('moves focus from the page to the opener, without scrolling', async () => {
		const opener = new FakeEl();
		await returnFocus(opener as unknown as HTMLElement);
		expect(doc.activeElement).toBe(opener);
		expect(opener.focused).toEqual({ preventScroll: true });
	});

	it('moves focus from inside the closing content', async () => {
		const opener = new FakeEl();
		const inside = new FakeEl();
		const content = new FakeEl();
		content.children.push(inside);
		doc.activeElement = inside;
		await returnFocus(opener as unknown as HTMLElement, {
			within: content as unknown as Element
		});
		expect(doc.activeElement).toBe(opener);
	});

	it('leaves focus alone when a person has already moved it to another control', async () => {
		const opener = new FakeEl();
		const elsewhere = new FakeEl();
		doc.activeElement = elsewhere;
		await returnFocus(opener as unknown as HTMLElement, {
			within: new FakeEl() as unknown as Element
		});
		expect(doc.activeElement).toBe(elsewhere);
		expect(opener.focused).toBeUndefined();
	});

	it('waits a tick, so a focus that something else moves in that tick wins', async () => {
		const opener = new FakeEl();
		const elsewhere = new FakeEl();
		queueMicrotask(() => {
			doc.activeElement = elsewhere;
		});
		await returnFocus(opener as unknown as HTMLElement);
		expect(opener.focused).toBeUndefined();
		expect(doc.activeElement).toBe(elsewhere);
	});

	it('uses the fallback when the opener is gone, disabled, inert or cannot take focus', async () => {
		for (const make of [
			(o: FakeEl) => (o.connected = false),
			(o: FakeEl) => (o.disabled = true),
			(o: FakeEl) => (o.inert = true),
			(o: FakeEl) => (o.focusable = false)
		]) {
			doc.activeElement = doc.body;
			const opener = new FakeEl();
			make(opener);
			const fallback = new FakeEl();
			await returnFocus(opener as unknown as HTMLElement, {
				fallback: () => fallback as unknown as HTMLElement
			});
			expect(doc.activeElement).toBe(fallback);
		}
	});

	it('does nothing when neither opener nor fallback is usable', async () => {
		const opener = new FakeEl();
		opener.connected = false;
		const fallback = new FakeEl();
		fallback.disabled = true;
		await returnFocus(opener as unknown as HTMLElement, {
			fallback: () => fallback as unknown as HTMLElement
		});
		expect(doc.activeElement).toBe(doc.body);
	});

	it('reads an HTMLElement from currentTarget and returns null otherwise', () => {
		class FakeHtml {}
		(globalThis as { HTMLElement?: unknown }).HTMLElement = FakeHtml;
		try {
			const target = new FakeHtml();
			expect(openerFrom({ currentTarget: target } as unknown as Event)).toBe(target);
			expect(openerFrom({ currentTarget: {} } as unknown as Event)).toBeNull();
			expect(openerFrom({ currentTarget: null } as unknown as Event)).toBeNull();
		} finally {
			delete (globalThis as { HTMLElement?: unknown }).HTMLElement;
		}
	});
});
