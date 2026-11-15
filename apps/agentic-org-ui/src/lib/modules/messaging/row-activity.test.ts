import { describe, expect, it } from 'vitest';
import { RowActivity } from './row-activity.svelte';

describe('MEM-2 row opener lifetime', () => {
	it('keeps a closed menu active until focus returns to that exact opener', () => {
		const rows = new RowActivity(), opener = new EventTarget(), other = new EventTarget();
		rows.hold('a', opener, 'menu');
		expect(rows.active('a')).toBe(true);
		expect(rows.active('b')).toBe(false);
		rows.focused(opener);
		expect(rows.active('a')).toBe(true);
		rows.closeMenu(opener);
		rows.focused(other);
		expect(rows.active('a')).toBe(true);
		rows.focused(opener);
		expect(rows.active('a')).toBe(false);
	});
	for (const owner of ['thread', 'dialog'] as const) {
		it(`keeps the ${owner} handoff when its source menu closes or briefly returns focus`, () => {
			const rows = new RowActivity(), opener = new EventTarget();
			rows.hold('a', opener, 'menu');
			rows.hold('a', opener, owner);
			rows.closeMenu(opener);
			rows.focused(opener);
			expect(rows.active('a')).toBe(true);
			rows.returning(opener, owner);
			expect(rows.active('a')).toBe(true);
			rows.focused(opener);
			expect(rows.active('a')).toBe(false);
		});
	}
	it('preserves another active opener, including a different opener of the same message', () => {
		const rows = new RowActivity(), thread = new EventTarget(), dialog = new EventTarget(), menu = new EventTarget();
		rows.hold('a', thread, 'thread'); rows.hold('a', dialog, 'dialog'); rows.hold('b', menu, 'menu');
		rows.release(thread, 'thread');
		expect(rows.active('a')).toBe(true);
		expect(rows.active('b')).toBe(true);
		rows.release(dialog, 'dialog');
		expect(rows.active('a')).toBe(false);
		expect(rows.active('b')).toBe(true);
		rows.clear();
		expect(rows.active('b')).toBe(false);
	});
	it('retains concurrent menu, dialog and thread owners of one opener', () => {
		const rows = new RowActivity(), opener = new EventTarget();
		rows.hold('a', opener, 'thread'); rows.hold('a', opener, 'menu');
		rows.closeMenu(opener); rows.focused(opener);
		expect(rows.active('a')).toBe(true);
		rows.hold('a', opener, 'dialog');
		rows.returning(opener, 'dialog'); rows.focused(opener);
		expect(rows.active('a')).toBe(true);
		rows.returning(opener, 'thread');
		expect(rows.active('a')).toBe(true);
		rows.focused(opener);
		expect(rows.active('a')).toBe(false);
	});
	it('ignores absent and unrelated focus targets and cleans an obsolete opener', () => {
		const rows = new RowActivity(), opener = new EventTarget(), absent = new EventTarget();
		rows.returning(undefined, 'thread'); rows.returning(absent, 'thread'); rows.closeMenu(absent);
		rows.focused(undefined); rows.focused(null); rows.focused(absent); rows.release(undefined, 'thread');
		expect(rows.active('a')).toBe(false);
		rows.hold('a', opener, 'thread'); rows.release(absent, 'thread');
		expect(rows.active('a')).toBe(true);
		rows.release(opener, 'thread');
		expect(rows.active('a')).toBe(false);
	});
});
