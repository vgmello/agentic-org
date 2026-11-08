import { beforeEach, describe, expect, it } from 'vitest';
import { isAfter, noteShown, shouldReload } from './live';

const at = (epoch: number, seq: number) => ({ epoch, seq });

describe('LIVE when the screens are behind the office', () => {
	beforeEach(() => noteShown('org', at(1, 5)));

	it('orders positions by epoch, then seq', () => {
		expect(isAfter(at(1, 6), at(1, 5))).toBe(true);
		expect(isAfter(at(1, 5), at(1, 5))).toBe(false);
		expect(isAfter(at(1, 4), at(1, 5))).toBe(false);
		expect(isAfter(at(2, 1), at(1, 9))).toBe(true); // a new epoch is later whatever its seq
		expect(isAfter(at(1, 9), at(2, 1))).toBe(false);
	});
	it('reloads for a change after the one shown, not for one the screens already show', () => {
		expect(shouldReload({ organization_id: 'org', position: at(1, 6) })).toBe(true);
		expect(shouldReload({ organization_id: 'org', position: at(1, 5) })).toBe(false);
		expect(shouldReload({ organization_id: 'org', position: at(1, 3) })).toBe(false);
	});
	it('stops reloading once the reload is shown', () => {
		noteShown('org', at(1, 6));
		expect(shouldReload({ organization_id: 'org', position: at(1, 6) })).toBe(false);
		expect(shouldReload({ organization_id: 'org', position: at(1, 7) })).toBe(true);
	});
	it('has nothing to refresh for an organization no screen loaded', () => {
		expect(shouldReload({ organization_id: 'never-loaded', position: at(9, 9) })).toBe(false);
	});
});
