import { describe, expect, it } from 'vitest';
import { isAfter, noteChanged, noteShown } from './live';

const at = (epoch: number, seq: number) => ({ epoch, seq });
let n = 0;
/** A fresh organization id per test: the module keeps state per organization. */
const fresh = () => `org-${++n}`;
const notice = (organization_id: string, epoch: number, seq: number) => ({ organization_id, position: at(epoch, seq) });
const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

/** A reload that shows `position` once it has run (like a screen's load ending in noteShown). */
function reloader(organizationId: string, shows: () => ReturnType<typeof at>) {
	const calls = { count: 0 };
	return {
		calls,
		reload: async () => {
			calls.count++;
			await tick();
			noteShown(organizationId, shows());
		}
	};
}

describe('LIVE when the screens are behind the office', () => {
	it('orders positions by epoch, then seq', () => {
		expect(isAfter(at(1, 6), at(1, 5))).toBe(true);
		expect(isAfter(at(1, 5), at(1, 5))).toBe(false);
		expect(isAfter(at(1, 4), at(1, 5))).toBe(false);
		expect(isAfter(at(2, 1), at(1, 9))).toBe(true); // a new epoch is later whatever its seq
		expect(isAfter(at(1, 9), at(2, 1))).toBe(false);
	});

	it('reloads for a change after the one shown, not for one the screens already show', async () => {
		const id = fresh();
		noteShown(id, at(1, 5));
		const { reload, calls } = reloader(id, () => at(1, 6));
		noteChanged(notice(id, 1, 5), reload);
		noteChanged(notice(id, 1, 3), reload);
		await tick();
		expect(calls.count).toBe(0);
		noteChanged(notice(id, 1, 6), reload);
		await tick();
		await tick();
		expect(calls.count).toBe(1);
	});

	it('has nothing to refresh for an organization no screen loaded', async () => {
		const id = fresh();
		const { reload, calls } = reloader(id, () => at(9, 9));
		noteChanged(notice(id, 9, 9), reload);
		await tick();
		expect(calls.count).toBe(0);
	});

	it('answers a burst of changes with one reload at a time, and one more if the office moved on meanwhile', async () => {
		const id = fresh();
		noteShown(id, at(1, 1));
		let showing = at(1, 3); // the first reload sees the office as it was when it started
		const { reload, calls } = reloader(id, () => showing);
		for (let seq = 2; seq <= 30; seq++) noteChanged(notice(id, 1, seq), reload);
		showing = at(1, 30); // by the time the first reload ends, all 30 are in
		await tick();
		await tick();
		await tick();
		await tick();
		expect(calls.count).toBeLessThanOrEqual(2);
		expect(calls.count).toBeGreaterThanOrEqual(1);
		// ...and the screens end up showing the last one (no further reload is wanted).
		const after = calls.count;
		await tick();
		expect(calls.count).toBe(after);
	});

	it('stops when a reload shows nothing newer, and tries again on the next notice', async () => {
		const id = fresh();
		noteShown(id, at(1, 1));
		let showing = at(1, 1); // the office has not caught up
		const { reload, calls } = reloader(id, () => showing);
		noteChanged(notice(id, 1, 2), reload);
		await tick();
		await tick();
		await tick();
		expect(calls.count).toBe(1); // no loop
		showing = at(1, 3);
		noteChanged(notice(id, 1, 3), reload);
		await tick();
		await tick();
		expect(calls.count).toBe(2);
	});

	it('catches up a change told while the screens were still loading', async () => {
		const id = fresh();
		const { reload, calls } = reloader(id, () => at(1, 7));
		noteShown(id, at(1, 5));
		noteChanged(notice(id, 1, 7), reload); // starts a reload
		await tick();
		await tick();
		expect(calls.count).toBe(1);
		// A first load that ended before the change it was told about is caught up too.
		const other = fresh();
		const second = reloader(other, () => at(1, 9));
		noteChanged(notice(other, 1, 9), second.reload); // nothing shown yet: nothing to refresh
		noteShown(other, at(1, 8)); // the first load ends behind what we were told
		await tick();
		await tick();
		expect(second.calls.count).toBe(1);
	});
});
