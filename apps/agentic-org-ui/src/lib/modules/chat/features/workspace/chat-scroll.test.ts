import { describe, expect, it } from 'vitest';
import { computeNewerBelow, syncStuckDayHeaders } from './chat-scroll';

function rect(top: number, bottom: number): DOMRect {
	return {
		top,
		bottom,
		left: 0,
		right: 400,
		width: 400,
		height: bottom - top,
		x: 0,
		y: top,
		toJSON: () => ({})
	};
}

function fakeEl(initialRect: DOMRect) {
	let currentRect = initialRect;
	const classes = new Set<string>();
	return {
		setRect(next: DOMRect) {
			currentRect = next;
		},
		getBoundingClientRect: () => currentRect,
		classList: {
			toggle(name: string, force?: boolean) {
				const on = force ?? !classes.has(name);
				if (on) classes.add(name);
				else classes.delete(name);
				return on;
			},
			contains(name: string) {
				return classes.has(name);
			}
		}
	};
}

describe('chat-scroll helpers', () => {
	it('marks older day header stuck at top and hides it once Today reaches top', () => {
		const older = fakeEl(rect(100, 124));
		const today = fakeEl(rect(240, 248));
		const list = {
			scrollTop: 120,
			getBoundingClientRect: () => rect(100, 500),
			querySelector: (sel: string) => (sel === '.chat-day.is-today' ? today : null),
			querySelectorAll: (sel: string) => (sel === '.chat-day:not(.is-today)' ? [older] : [])
		} as unknown as HTMLElement;

		syncStuckDayHeaders(list);
		expect(older.classList.contains('is-stuck')).toBe(true);
		expect(older.classList.contains('is-past')).toBe(false);

		today.setRect(rect(100, 108));
		syncStuckDayHeaders(list);
		expect(older.classList.contains('is-past')).toBe(true);
	});

	it('computes newer below fold and unseen message counts', () => {
		const m1 = fakeEl(rect(100, 220));
		const m2 = fakeEl(rect(360, 460));
		const m3 = fakeEl(rect(470, 550));
		const list = {
			getBoundingClientRect: () => rect(0, 400),
			querySelectorAll: () => [m1, m2, m3]
		} as unknown as HTMLElement;

		expect(computeNewerBelow(list)).toEqual({ hasNewer: true, unseenCount: 1 });

		m2.setRect(rect(200, 300));
		m3.setRect(rect(310, 390));
		expect(computeNewerBelow(list)).toEqual({ hasNewer: false, unseenCount: 0 });
	});
});
