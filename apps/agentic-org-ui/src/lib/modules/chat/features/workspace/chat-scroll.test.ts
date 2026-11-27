import { describe, expect, it } from 'vitest';
import {
	computeNewerBelow,
	scrollMessageBelowStickyHeader,
	syncStuckDayHeaders
} from './chat-scroll';

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

	it('keeps only the latest pinned older-day header visible when multiple older days stick', () => {
		const day1 = fakeEl(rect(100, 128));
		const day2 = fakeEl(rect(100, 128));
		const day3 = fakeEl(rect(260, 288));
		const list = {
			scrollTop: 240,
			getBoundingClientRect: () => rect(100, 500),
			querySelector: () => null,
			querySelectorAll: (sel: string) =>
				sel === '.chat-day:not(.is-today)' ? [day1, day2, day3] : []
		} as unknown as HTMLElement;

		syncStuckDayHeaders(list);
		expect(day1.classList.contains('is-stuck')).toBe(false);
		expect(day1.classList.contains('is-past')).toBe(true);
		expect(day2.classList.contains('is-stuck')).toBe(true);
		expect(day2.classList.contains('is-past')).toBe(false);
		expect(day3.classList.contains('is-stuck')).toBe(false);
		expect(day3.classList.contains('is-past')).toBe(false);
	});

	it('resolves a cold deep-link container via getter after tick and clears sticky band', async () => {
		const day = { ...fakeEl(rect(100, 128)), offsetHeight: 28 };
		const msg = {
			offsetHeight: 60,
			getBoundingClientRect: () => rect(420, 480)
		};
		let mounted: HTMLElement | undefined;
		const list = {
			scrollTop: 0,
			clientHeight: 400,
			getBoundingClientRect: () => rect(100, 500),
			querySelector: (sel: string) => {
				if (sel.includes('m-deep')) return msg;
				if (sel === '.chat-day:not(.is-today)') return day;
				return null;
			},
			querySelectorAll: (sel: string) => (sel === '.chat-day:not(.is-today)' ? [day] : [])
		} as unknown as HTMLElement;

		const prevCss = globalThis.CSS;
		globalThis.CSS = { escape: (v: string) => v } as typeof CSS;
		try {
			scrollMessageBelowStickyHeader(() => mounted, 'm-deep');
			mounted = list;
			await Promise.resolve();
			await Promise.resolve();
			// elTop = 420 - 100 + 0 = 320; desiredOffset = max(36, (400-60)/3 = 113.33) -> scrollTop = 207
			expect(list.scrollTop).toBe(207);
		} finally {
			globalThis.CSS = prevCss;
		}
	});
});
